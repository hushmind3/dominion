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
#[cfg(test)]
mod tests{
    use super::*;
    struct ModelJournal{
        seen:std::sync::Mutex<Vec<crate::Snapshot>>,
        cancel_after_intent:bool,
        directive:Option<String>,
        cancel:std::sync::atomic::AtomicBool
    }
    impl crate::Journal for ModelJournal {
        fn save(&self,snapshot:&crate::Snapshot)->Result<(),String>{
            self.seen.lock().unwrap().push(snapshot.clone());
            if self.cancel_after_intent && snapshot.model_in_flight {
                self.cancel.store(true,std::sync::atomic::Ordering::SeqCst);
            }
            Ok(())
        }
        fn cancelled(&self,_:&str)->bool{
            self.cancel.load(std::sync::atomic::Ordering::SeqCst)
        }
        fn directive(&self,_:&str)->Result<Option<String>,String>{
            Ok(self.directive.clone())
        }
    }
    fn snapshot()->crate::Snapshot{
        crate::Snapshot::from_request(&crate::RunRequest{
            id:"model-protected".into(),instruction:"Answer".into(),
            owner_instructions:String::new(),strategy:crate::Strategy::Balanced,
            limits:crate::Limits::default(),allowed_tools:Vec::new(),
            checks:Vec::new(),require_evidence:false
        })
    }
    fn ready<T>(f:impl std::future::Future<Output=T>)->T{
        use std::future::Future;
        let waker=std::task::Waker::noop();
        let mut cx=std::task::Context::from_waker(waker);
        let mut future=Box::pin(f);
        match Future::poll(future.as_mut(),&mut cx){
            std::task::Poll::Ready(v)=>v,
            std::task::Poll::Pending=>panic!("expected an immediate test future")
        }
    }
    #[test] fn model_dispatch_denies_post_journal_cancel_without_making_api_call(){
        let journal=ModelJournal{seen:std::sync::Mutex::new(Vec::new()),
            cancel_after_intent:true,directive:None,
            cancel:std::sync::atomic::AtomicBool::new(false)};
        let mut run=snapshot();
        let called=std::sync::atomic::AtomicBool::new(false);
        let outcome=ready(dispatch_model_turn(&journal,&mut run,None,
            ||async{
                called.store(true,std::sync::atomic::Ordering::SeqCst);
                Ok::<_,String>("model text")
            })).unwrap();
        assert!(matches!(outcome,ModelAttempt::Cancelled));
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!run.model_in_flight);
        assert_eq!(journal.seen.lock().unwrap().len(),2);
    }
    #[test] fn successful_model_call_preserves_uncertain_marker_until_owner_settlement(){
        let journal=ModelJournal{seen:std::sync::Mutex::new(Vec::new()),
            cancel_after_intent:false,directive:None,
            cancel:std::sync::atomic::AtomicBool::new(false)};
        let mut run=snapshot();
        let outcome=ready(dispatch_model_turn(&journal,&mut run,None,
            ||async{Ok::<_,String>("verified text")})).unwrap();
        assert!(matches!(outcome,ModelAttempt::Completed("verified text")));
        assert!(run.model_in_flight);
        assert_eq!(journal.seen.lock().unwrap().len(),1);
        run.model_calls+=1;
        run.model_in_flight=false;
        crate::Journal::save(&journal,&run).unwrap();
        assert_eq!(journal.seen.lock().unwrap()[1].model_calls,1);
    }
    #[test] fn failed_paid_call_stays_unknown_and_owner_change_never_dispatches(){
        let journal=ModelJournal{seen:std::sync::Mutex::new(Vec::new()),
            cancel_after_intent:false,directive:Some("corrected".into()),
            cancel:std::sync::atomic::AtomicBool::new(false)};
        let mut run=snapshot();
        let corrected=ready(dispatch_model_turn(&journal,&mut run,None,
            ||async{Ok::<_,String>(())})).unwrap();
        assert!(matches!(corrected,ModelAttempt::OwnerChanged));
        assert!(!run.model_in_flight);
        let failed=ready(dispatch_model_turn(&journal,&mut run,Some("corrected"),
            ||async{Err::<(),String>("connection died after charge".into())})).unwrap();
        assert!(matches!(failed,ModelAttempt::Uncertain(_)));
        assert!(run.model_in_flight);
    }
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
    #[test]fn resume_rejects_impossible_checkpoints(){
        assert!(HarnessDrive::resume(5,6).is_err());
        assert!(HarnessDrive::new(0).is_err());
        assert!(HarnessDrive::new(201).is_err());
    }
}
