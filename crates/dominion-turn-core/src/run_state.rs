//! Shared semantic state contract across model turns and room jobs.
//! Approval and persistence remain the responsibility of the host.
use serde_json::{json,Value};

// One provider-independent lifecycle reducer for Chat, Room and SDK hosts.
// Storage CAS and actual approvals remain in the product adapter.
pub const QUEUED: &str = "queued";
pub const RUNNING: &str = "running";
pub const WAITING_APPROVAL: &str = "waiting_approval";
pub const VERIFYING: &str = "verifying";
pub const FINALIZING: &str = "finalizing";
pub const SUCCEEDED: &str = "succeeded";
pub const FAILED: &str = "failed";
pub const CANCELLED: &str = "cancelled";
pub const INTERRUPTED: &str = "interrupted";

pub fn is_terminal(phase: &str) -> bool {
    matches!(phase, SUCCEEDED | FAILED | CANCELLED | INTERRUPTED)
}

/// Returns the next phase, or None for an illegal transition. Repeating the
/// current phase is treated as idempotent.
pub fn transition(current: &str, event: &str) -> Option<&'static str> {
    // A corrupted or future-version phase is not an ordinary failure.
    // Reject it instead of converting it to a terminal "failed" receipt,
    // which would hide a bad replay/migration from the owning host.
    if !matches!(current, QUEUED | RUNNING | WAITING_APPROVAL | VERIFYING
        | FINALIZING | SUCCEEDED | FAILED | CANCELLED | INTERRUPTED) {
        return None;
    }
    let next = match event {
        "start" => match current {
            QUEUED | RUNNING => RUNNING,
            _ => return None,
        },
        "approval_requested" => match current {
            RUNNING | WAITING_APPROVAL => WAITING_APPROVAL,
            _ => return None,
        },
        "approval_resolved" => match current {
            WAITING_APPROVAL | RUNNING => RUNNING,
            _ => return None,
        },
        "verify" => match current {
            RUNNING | VERIFYING => VERIFYING,
            _ => return None,
        },
        "finalize" => match current {
            RUNNING | VERIFYING | WAITING_APPROVAL | FINALIZING => FINALIZING,
            _ => return None,
        },
        "succeed" => match current {
            FINALIZING | RUNNING | VERIFYING | SUCCEEDED => SUCCEEDED,
            _ => return None,
        },
        "fail" => match current {
            _ if is_terminal(current) => return None,
            _ => FAILED,
        },
        "cancel" => match current {
            _ if current == CANCELLED => CANCELLED,
            _ if is_terminal(current) => return None,
            _ => CANCELLED,
        },
        "interrupt" => match current {
            _ if current == INTERRUPTED => INTERRUPTED,
            _ if is_terminal(current) => return None,
            _ => INTERRUPTED,
        },
        _ => return None,
    };
    Some(next)
}

pub fn phase_for_status(status: &str) -> &'static str {
    match status {
        "completed" => SUCCEEDED,
        "cancelled" => CANCELLED,
        "interrupted" => INTERRUPTED,
        _ => FAILED,
    }
}

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum RunState{Queued,Running,Completed,Failed,Cancelled,Interrupted,NeedsReview,Skipped}
/// Decide terminal meaning in the same model-independent core used
/// by the chat host. A missing final answer is never completed work.
pub fn terminal_result<T,E>(result:&Result<Option<T>,E>,cancelled:bool)->RunState{
    if cancelled{return RunState::Cancelled;}
    match result{
        Ok(Some(_))=>RunState::Completed,
        Ok(None)|Err(_)=>RunState::Failed,
    }
}

impl RunState{
    pub const fn as_str(self)->&'static str{
        match self{
            Self::Queued=>"queued",Self::Running=>"running",
            Self::Completed=>"completed",Self::Failed=>"failed",
            Self::Cancelled=>"cancelled",Self::Interrupted=>"interrupted",Self::NeedsReview=>"needs_review",
            Self::Skipped=>"skipped",
        }
    }
    pub fn parse(value:&str)->Option<Self>{
        match value{
            "queued"|"assigned"=>Some(Self::Queued),
            "running"=>Some(Self::Running),
            "completed"|"succeeded"=>Some(Self::Completed),
            "failed"=>Some(Self::Failed),
            "cancelled"=>Some(Self::Cancelled),
            "interrupted"=>Some(Self::Interrupted),
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
    /// Project durable product records into one typed, source-addressed
    /// contract. Missing owner-message identity stays null, never an invented
    /// user message or job id. All clients can consume the same state names.
    pub fn from_record(kind:&str,record:&Value)->Option<Self>{
        if !matches!(kind,"execution"|"job"|"room_work_assignment"|"task_team_work"){
            return None;
        }
        let run_id=record["id"].as_str().filter(|id|!id.is_empty())?;
        let status=record["status"].as_str()?;
        let state=if kind=="execution" && status=="running"
            && record["phase"]=="queued" {
            RunState::Queued
        }else{
            RunState::parse(status)?
        };
        let source_id=record["sourceMessageId"].as_str()
            .or_else(||record["input"]["sourceMessageId"].as_str())
            .unwrap_or("").to_string();
        let actor_id=record["agentId"].as_str()
            .or_else(||record["input"]["agentId"].as_str())
            .map(str::to_string);
        Some(Self{run_id:run_id.to_string(),source_id,event_seq:0,state,actor_id})
    }

    pub fn into_value(self)->Value{
        let source=if self.source_id.is_empty(){Value::Null}else{json!(self.source_id)};
        json!({"schemaVersion":1,"type":"run_state",
            "runId":self.run_id,"sourceMessageId":source,
            "eventSeq":self.event_seq,"state":self.state.as_str(),
            "agentId":self.actor_id})
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn invalid_phase_never_becomes_a_valid_terminal_receipt(){
        for bad in ["", "awaiting", "completed", "needs_review", "ruNNing"] {
            for event in ["fail", "cancel", "interrupt", "start", "succeed"] {
                assert_eq!(transition(bad,event),None,"{bad} + {event}");
            }
        }
        assert_eq!(transition(RUNNING,"fail"),Some(FAILED));
        assert_eq!(transition(WAITING_APPROVAL,"cancel"),Some(CANCELLED));
    }
    #[test]
    fn terminal_states_never_reopen_and_review_requires_explicit_recovery(){
        for terminal in [RunState::Completed,RunState::Failed,
            RunState::Cancelled,RunState::Interrupted,RunState::Skipped]{
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
    #[test]
    fn terminal_result_requires_a_delivered_answer_and_honors_cancellation(){
        assert_eq!(terminal_result(&Ok::<Option<u8>,String>(Some(1)),false),
            RunState::Completed);
        assert_eq!(terminal_result(&Ok::<Option<u8>,String>(None),false),
            RunState::Failed);
        assert_eq!(terminal_result(&Err::<Option<u8>,String>("failed".into()),false),
            RunState::Failed);
        assert_eq!(terminal_result(&Ok::<Option<u8>,String>(Some(1)),true),
            RunState::Cancelled);
    }

    #[test]
    fn task_team_worker_projects_into_the_same_typed_run_events(){
        let assigned=RunEvent::from_record("task_team_work",&json!({
            "id":"work-1","status":"assigned","executionId":"exec-1",
            "nodeId":"read-1","agentId":"worker-1"
        })).unwrap().into_value();
        assert_eq!(assigned["state"],"queued");
        assert_eq!(assigned["agentId"],"worker-1");
        assert!(assigned["sourceMessageId"].is_null());
        let blocked=RunEvent::from_record("task_team_work",&json!({
            "id":"work-1","status":"needs_review"
        })).unwrap().into_value();
        assert_eq!(blocked["state"],"needs_review");
    }
    #[test]
    fn durable_job_chat_and_room_records_project_into_one_run_schema(){
        let job=RunEvent::from_record("job",&json!({
            "id":"job-1","status":"queued","input":{"agentId":"agent-1"}
        })).unwrap().into_value();
        assert_eq!(job["state"],"queued");
        assert!(job["sourceMessageId"].is_null());
        assert_eq!(job["agentId"],"agent-1");
        let chat=RunEvent::from_record("execution",&json!({
            "id":"chat-1","status":"completed","sourceMessageId":"owner-1"
        })).unwrap().into_value();
        assert_eq!(chat["state"],"completed");
        assert_eq!(chat["sourceMessageId"],"owner-1");
        let room=RunEvent::from_record("room_work_assignment",&json!({
            "id":"room-1","status":"assigned","sourceMessageId":"owner-2"
        })).unwrap().into_value();
        assert_eq!(room["state"],"queued");
        assert_eq!(RunState::parse("interrupted"),Some(RunState::Interrupted));
        assert!(RunEvent::from_record("provider",&json!({"id":"p","status":"running"})).is_none());
    }

}
