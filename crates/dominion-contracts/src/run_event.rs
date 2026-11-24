//! Versioned execution identity shared by the native event journal,
//! SDK and product transports. No provider, HTTP or room dependencies.
use serde::{Deserialize, Serialize};

/// Canonical execution lifecycle names. It is intentionally separate from
/// room presentation status (`idle`, `silence`) and job scheduling status.
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum RunLifecycle {
    Queued,Running,AwaitingApproval,Paused,NeedsReview,Interrupted,
    Completed,Failed,Cancelled,
}
impl RunLifecycle {
    pub fn parse(value:&str)->Option<Self>{
        Some(match value{
            "queued"=>Self::Queued,
            "running"=>Self::Running,
            "awaiting_approval"=>Self::AwaitingApproval,
            "paused"=>Self::Paused,
            "needs_review"=>Self::NeedsReview,
            "interrupted"=>Self::Interrupted,
            "completed"=>Self::Completed,
            "failed"=>Self::Failed,
            "cancelled"=>Self::Cancelled,
            _=>return None,
        })
    }
}

/// Additive reference placed on existing durable event envelopes. Consumers
/// can link Chat, Rooms, Task, SDK and approvals by execution ID and event
/// sequence without guessing from UI-specific record IDs.
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RunEventRef {
    pub schema_version:u16,
    pub execution_id:String,
    #[serde(skip_serializing_if="Option::is_none")]
    pub session_id:Option<String>,
    /// Product job identity; it is never mistaken for the native execution.
    #[serde(skip_serializing_if="Option::is_none")]
    pub job_id:Option<String>,
    /// Lead native execution for a separately running child native worker.
    #[serde(skip_serializing_if="Option::is_none")]
    pub parent_execution_id:Option<String>,
    /// Separate external CLI run/receipt ID. Not a native execution.
    #[serde(skip_serializing_if="Option::is_none")]
    pub external_run_id:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub task_id:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub room_id:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub agent_id:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub source_message_id:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub lifecycle:Option<RunLifecycle>,
}
impl RunEventRef {
    pub const SCHEMA_VERSION:u16=1;
    pub fn new(id:&str)->Option<Self>{
        if id.trim().is_empty()||id.len()>256{return None;}
        Some(Self{schema_version:Self::SCHEMA_VERSION,
            execution_id:id.to_string(),session_id:None,
            job_id:None,parent_execution_id:None,external_run_id:None,task_id:None,
            room_id:None,agent_id:None,source_message_id:None,
            lifecycle:None})
    }
}


/// One committed event in the durable, globally ordered SQLite event journal.
/// `seq` is a global cursor; `run_ref.execution_id` is the stable run key.
/// Historical record/message payload fields remain available for old clients.
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RunEventEnvelope {
    pub seq:u64,
    #[serde(rename="type")]
    pub event_type:String,
    pub run_ref:RunEventRef,
    #[serde(flatten)]
    pub payload:std::collections::BTreeMap<String,serde_json::Value>,
}

/// Bounded, loss-aware execution timeline. A missing event is never invented.
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RunEventPage {
    pub schema_version:u16,
    pub execution_id:String,
    pub after:u64,
    pub next_after:u64,
    pub has_more:bool,
    pub earliest_available_seq:Option<u64>,
    pub history_truncated:bool,
    pub events:Vec<RunEventEnvelope>,
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn versioned_execution_reference_and_lifecycle_roundtrip(){
        let mut value=RunEventRef::new("exec-1").unwrap();
        value.session_id=Some("room:r".into());
        value.room_id=Some("r".into());
        value.lifecycle=RunLifecycle::parse("needs_review");
        let json=serde_json::to_value(&value).unwrap();
        assert_eq!(json["schemaVersion"],1);
        assert_eq!(json["executionId"],"exec-1");
        assert_eq!(json["lifecycle"],"needs_review");
        assert!(json.get("agentId").is_none());
        assert_eq!(serde_json::from_value::<RunEventRef>(json).unwrap(),value);
        assert!(RunLifecycle::parse("idle").is_none());
        assert!(RunEventRef::new(" ").is_none());
        let mut linked=RunEventRef::new("native-worker").unwrap();
        linked.job_id=Some("job-child".into());
        linked.parent_execution_id=Some("native-lead".into());
        linked.external_run_id=Some("external:receipt".into());
        let payload=serde_json::to_value(&linked).unwrap();
        assert_eq!(payload["jobId"],"job-child");
        assert_eq!(payload["parentExecutionId"],"native-lead");
        assert_eq!(payload["externalRunId"],"external:receipt");
        assert_eq!(serde_json::from_value::<RunEventRef>(payload).unwrap(),linked);
        assert_eq!(serde_json::to_value(RunLifecycle::parse("awaiting_approval")).unwrap(),
            json!("awaiting_approval"));
    }
}
