//! Single, portable admission cursor for each agent turn lane.
//! The Host retains streaming wire framing; Algorithm owns decision policy.
//! Neither is allowed to implement a different step/cancel/model-call clock.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum DriveAdmission {Next(usize),Cancelled,Exhausted}
#[derive(Clone,Debug)]
pub struct HarnessDrive {
    step:usize,
    max_steps:usize,
}
impl HarnessDrive {
    /// `completed_steps` comes from a trusted durable Snapshot, not an LLM.
    pub fn resume(max_steps:usize,completed_steps:usize)->Result<Self,String>{
        if max_steps==0||completed_steps>max_steps||max_steps>200{
            return Err("Invalid shared Harness step range".into());
        }
        Ok(Self{step:completed_steps,max_steps})
    }
    pub fn new(max_steps:usize)->Result<Self,String>{
        Self::resume(max_steps,0)
    }
    /// Called BEFORE requesting a provider generation. A cancellation or
    /// hard budget never yields a Next permit.
    pub fn admit(&mut self,cancelled:bool,model_calls:usize,max_model_calls:usize)
        ->DriveAdmission{
        if cancelled{return DriveAdmission::Cancelled;}
        if self.step>=self.max_steps||model_calls>=max_model_calls{
            return DriveAdmission::Exhausted;
        }
        let step=self.step;
        self.step+=1;
        DriveAdmission::Next(step)
    }
    pub fn completed_steps(&self)->usize{self.step}
}
/// A single write-ahead model-dispatch operator for every native, Chat,
/// Room and streamed provider route. The Host supplies only a model adapter.
pub enum ModelAttempt<T> {
    Completed(T),
    Cancelled,
    OwnerChanged,
    /// No replay is permitted: the provider may have already consumed tokens.
    Uncertain(String),
}

/// Persist an in-flight marker BEFORE allowing a paid model call. Recheck
/// cancellation / owner correction after persistence, then invoke the model.
/// On success the caller MUST durably settle usage and the complete model
/// observation BEFORE clearing the in-flight marker. On error preserve it:
/// replaying a partially observed paid generation would double-spend.
pub async fn dispatch_model_turn<T,F,Fut>(
    journal:&dyn crate::Journal,
    snapshot:&mut crate::Snapshot,
    expected_directive:Option<&str>,
    invoke:F,
)->Result<ModelAttempt<T>,String>
where F:FnOnce()->Fut,Fut:std::future::Future<Output=Result<T,String>>
{
    snapshot.model_in_flight=true;
    journal.save(snapshot)?;
    match crate::dispatch_fence(journal,&snapshot.id,expected_directive)?{
        crate::DispatchFence::Cancelled=>{
            snapshot.model_in_flight=false;
            journal.save(snapshot)?;
            return Ok(ModelAttempt::Cancelled);
        },
        crate::DispatchFence::OwnerChanged=>{
            snapshot.model_in_flight=false;
            journal.save(snapshot)?;
            return Ok(ModelAttempt::OwnerChanged);
        },
        crate::DispatchFence::Proceed=>{}
    }
    match invoke().await {
        Ok(result)=>Ok(ModelAttempt::Completed(result)),
        Err(error)=>Ok(ModelAttempt::Uncertain(error)),
    }
}
/// The only step-loop implementation for native ReAct and streamed Chat.
/// Adapters supply their policy-specific turn body and cancellation settlement,
/// while admission ordering and the execution clock stay in one Rust source.
#[macro_export]
macro_rules! harness_steps {
    (
        $label:lifetime : $drive:ident,
        running = $running:expr,
        cancelled = $cancelled:expr,
        calls = $calls:expr,
        limit = $limit:expr,
        on_cancel = $on_cancel:block,
        |$step:ident| $body:block
    ) => {
        $label: while $running {
            let $step=match $drive.admit($cancelled,$calls,$limit){
                $crate::DriveAdmission::Next(next)=>next,
                $crate::DriveAdmission::Cancelled=>$on_cancel,
                $crate::DriveAdmission::Exhausted=>break $label,
            };
            $body
        }
    };
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn shared_cursor_is_deterministic_across_chat_and_native_resume(){
        let mut first=HarnessDrive::new(3).unwrap();
        assert_eq!(first.admit(false,0,3),DriveAdmission::Next(0));
        assert_eq!(first.admit(false,1,3),DriveAdmission::Next(1));
        let mut recovered=HarnessDrive::resume(3,first.completed_steps()).unwrap();
        assert_eq!(recovered.admit(false,2,3),DriveAdmission::Next(2));
        assert_eq!(recovered.admit(false,2,3),DriveAdmission::Exhausted);
        assert_eq!(recovered.admit(true,2,3),DriveAdmission::Cancelled);
    }
    #[test]fn denied_generation_never_consumes_an_admission(){
        let mut drive=HarnessDrive::new(1).unwrap();
        assert_eq!(drive.admit(true,0,1),DriveAdmission::Cancelled);
        assert_eq!(drive.completed_steps(),0);
        assert_eq!(drive.admit(false,1,1),DriveAdmission::Exhausted);
        assert_eq!(drive.completed_steps(),0);
        assert_eq!(drive.admit(false,0,1),DriveAdmission::Next(0));
        assert_eq!(drive.completed_steps(),1);
    }
    #[test]fn both_loop_styles_share_one_control_flow_source(){
        let mut drive=HarnessDrive::new(8).unwrap();
        let mut seen=Vec::new();
        let mut running=true;
        harness_steps!('steps:drive,running=running,
            cancelled=false,calls=seen.len(),limit=8,
            on_cancel={panic!("should not cancel")},
            |step|{
                seen.push(step);
                if step==0{continue 'steps;}
                if step==2{running=false;}
            }
        );
        assert_eq!(seen,vec![0,1,2]);
        assert_eq!(drive.completed_steps(),3);
    }
    #[test]fn resume_rejects_impossible_checkpoints(){
        assert!(HarnessDrive::resume(5,6).is_err());
        assert!(HarnessDrive::new(0).is_err());
        assert!(HarnessDrive::new(201).is_err());
    }
}
