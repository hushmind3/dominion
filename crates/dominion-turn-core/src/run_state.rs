//! Shared semantic state contract across model turns and room jobs.
//! Approval and persistence remain the responsibility of the host.
use serde_json::{json,Value};
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum RunState{Queued,Running,Completed,Failed,Cancelled,NeedsReview,Skipped}
impl RunState{
    pub const fn as_str(self)->&'static str{
        match self{
            Self::Queued=>"queued",Self::Running=>"running",
            Self::Completed=>"completed",Self::Failed=>"failed",
            Self::Cancelled=>"cancelled",Self::NeedsReview=>"needs_review",
            Self::Skipped=>"skipped",
        }
    }
    pub fn parse(value:&str)->Option<Self>{
        match value{
            "queued"|"assigned"=>Some(Self::Queued),
            "running"=>Some(Self::Running),
            "completed"=>Some(Self::Completed),
            "failed"=>Some(Self::Failed),
            "cancelled"=>Some(Self::Cancelled),
            "needs_review"=>Some(Self::NeedsReview),
            "skipped"=>Some(Self::Skipped),_=>None,
        }
    }
    pub fn permits(self,next:Self)->bool{
        use RunState::*;
        match (self,next){
            (Queued,Running|Completed|Failed|Cancelled|Skipped|NeedsReview)
            |(Running,Completed|Failed|Cancelled|NeedsReview)
            |(NeedsReview,Running|Cancelled|Failed)=>true,
            (a,b) if a==b=>true,_=>false,
        }
    }
}
#[derive(Debug,Clone,PartialEq)]
pub struct RunEvent{
    pub run_id:String,
    pub source_id:String,
    pub event_seq:u64,
    pub state:RunState,
    pub actor_id:Option<String>,
}
impl RunEvent{
    pub fn into_value(self)->Value{
        json!({"schemaVersion":1,"type":"run_state",
            "runId":self.run_id,"sourceMessageId":self.source_id,
            "eventSeq":self.event_seq,"state":self.state.as_str(),
            "agentId":self.actor_id})
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn terminal_states_never_reopen_and_review_requires_explicit_recovery(){
        for terminal in [RunState::Completed,RunState::Failed,
            RunState::Cancelled,RunState::Skipped]{
            assert!(!terminal.permits(RunState::Running));
            assert!(!terminal.permits(RunState::NeedsReview));
        }
        assert!(RunState::Queued.permits(RunState::Running));
        assert!(RunState::Running.permits(RunState::NeedsReview));
        assert!(RunState::NeedsReview.permits(RunState::Running));
    }
    #[test]
    fn event_envelope_is_versioned_and_source_addressed(){
        let event=RunEvent{run_id:"run".into(),source_id:"owner".into(),
            event_seq:7,state:RunState::NeedsReview,actor_id:Some("a".into())
        }.into_value();
        assert_eq!(event["schemaVersion"],1);
        assert_eq!(event["eventSeq"],7);
        assert_eq!(event["state"],"needs_review");
        assert_eq!(RunState::parse("assigned"),Some(RunState::Queued));
    }
}
