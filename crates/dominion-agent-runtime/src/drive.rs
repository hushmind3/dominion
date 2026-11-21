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
    #[test]fn resume_rejects_impossible_checkpoints(){
        assert!(HarnessDrive::resume(5,6).is_err());
        assert!(HarnessDrive::new(0).is_err());
        assert!(HarnessDrive::new(201).is_err());
    }
}
