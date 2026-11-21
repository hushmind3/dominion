//! Single portable run/status/event contract for chat, rooms, task teams and SDK.
//! Host-specific storage and approval remain in the Rust host, not this crate.
use serde::{Deserialize,Serialize};

pub const RUN_CONTRACT_VERSION:u16=1;
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum RunSurface { Chat, Room, Team, Sdk, ExternalAgent }
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum RunPhase {
    Queued,Running,WaitingApproval,Verifying,Finalizing,Succeeded,
    Failed,Cancelled,Interrupted,NeedsReview,
}
impl RunPhase {
    pub const fn as_str(self)->&'static str {
        match self {
            Self::Queued=>"queued",Self::Running=>"running",
            Self::WaitingApproval=>"waiting_approval",
            Self::Verifying=>"verifying",Self::Finalizing=>"finalizing",
            Self::Succeeded=>"succeeded",Self::Failed=>"failed",
            Self::Cancelled=>"cancelled",Self::Interrupted=>"interrupted",
            Self::NeedsReview=>"needs_review",
        }
    }
    pub fn from_host_status(value:&str)->Option<Self>{
        Some(match value {
            "queued"=>Self::Queued,"running"=>Self::Running,
            "waiting_approval"|"awaiting_approval"=>Self::WaitingApproval,
            "verifying"=>Self::Verifying,"finalizing"=>Self::Finalizing,
            "completed"|"succeeded"=>Self::Succeeded,
            "failed"|"denied"|"exhausted"=>Self::Failed,
            "cancelled"=>Self::Cancelled,"interrupted"=>Self::Interrupted,
            "needs_review"|"in_doubt"=>Self::NeedsReview,
            _=>return None,
        })
    }
    pub const fn terminal(self)->bool {
        matches!(self,Self::Succeeded|Self::Failed|Self::Cancelled|
            Self::Interrupted|Self::NeedsReview)
    }
}
/// The only phase transition vocabulary shared by Chat, Rooms, native SDK
/// jobs and Task Team projection. An orchestrator is NOT allowed to rewrite
/// a terminal outcome by providing a different runtime-specific string.
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum RunTransition {
    Start, RequestApproval, ResolveApproval, Verify, Finalize, Succeed,
    Fail, Cancel, Interrupt, RequireReview,
}
impl RunTransition {
    pub fn from_host_event(event:&str)->Option<Self> {
        Some(match event {
            "start"=>Self::Start,
            "approval_requested"=>Self::RequestApproval,
            "approval_resolved"=>Self::ResolveApproval,
            "verify"=>Self::Verify,
            "finalize"=>Self::Finalize,
            "succeed"=>Self::Succeed,
            "fail"=>Self::Fail,
            "cancel"=>Self::Cancel,
            "interrupt"=>Self::Interrupt,
            "review_requested"=>Self::RequireReview,
            _=>return None,
        })
    }
}
impl RunPhase {
    /// Pure, portable lifecycle reducer. The host persists the transition
    /// and evidence atomically; this function never dispatches work.
    pub fn advance(self,event:RunTransition)->Option<Self>{
        use RunPhase::*;
        use RunTransition::*;
        let next=match (self,event) {
            (Queued|Running,Start)=>Running,
            (Running|WaitingApproval,RequestApproval)=>WaitingApproval,
            (WaitingApproval|Running,ResolveApproval)=>Running,
            (Running|Verifying,Verify)=>Verifying,
            (Running|Verifying|WaitingApproval|Finalizing,Finalize)=>Finalizing,
            (Finalizing|Running|Verifying|Succeeded,Succeed)=>Succeeded,
            (phase,Fail) if !phase.terminal()=>Failed,
            (Cancelled,Cancel)=>Cancelled,
            (phase,Cancel) if !phase.terminal()=>Cancelled,
            (Interrupted,Interrupt)=>Interrupted,
            (phase,Interrupt) if !phase.terminal()=>Interrupted,
            (NeedsReview,RequireReview)=>NeedsReview,
            (phase,RequireReview) if !phase.terminal()=>NeedsReview,
            _=>return None,
        };
        Some(next)
    }
}
/// One committed Room event. The ordinal is a per-room SQLite sequence.
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RoomEvent {
    pub version:u16,
    pub room_id:String,
    pub session_id:String,
    pub seq:u64,
    pub event_id:String,
    pub author:String,
    pub reply_to:Option<String>,
    pub origin_run:Option<String>,
    pub kind:String,
}
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RoomAgentBinding {
    pub room_id:String,
    pub agent_id:String,
    pub session_id:String,
    pub read_cursor:u64,
    pub state:String,
}
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RunRef {
    pub version:u16,
    pub surface:RunSurface,
    pub run_id:String,
    pub parent_run_id:Option<String>,
}
/// Portable, source-ordered record event for all DOMINION run surfaces.
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RunEvent {
    pub version:u16,
    pub event_seq:u64,
    pub run_ref:RunRef,
    pub phase:RunPhase,
    pub event_type:String,
    pub source_journal:String,
}
impl RunEvent {
    pub fn new(event_seq:u64,run_ref:RunRef,phase:RunPhase,
        event_type:impl Into<String>,source_journal:impl Into<String>)
        ->Result<Self,String>{
        let event_type=event_type.into();
        let source_journal=source_journal.into();
        if event_seq==0||run_ref.version!=RUN_CONTRACT_VERSION||
            event_type.is_empty()||event_type.len()>80||
            !matches!(source_journal.as_str(),"job"|"execution"|"remote_task"){
            return Err("Invalid durable RunEvent identity".into());
        }
        Ok(Self{version:RUN_CONTRACT_VERSION,event_seq,run_ref,phase,
            event_type,source_journal})
    }
}
impl RunRef {
    pub fn new(surface:RunSurface,run_id:impl Into<String>,
        parent_run_id:Option<String>)->Result<Self,String>{
        let id=run_id.into();
        if id.trim().is_empty()||id.len()>256||
            parent_run_id.as_ref().is_some_and(|p|p.is_empty()||p==&id||p.len()>256){
            return Err("Invalid run reference or parent linkage".into());
        }
        Ok(Self{version:RUN_CONTRACT_VERSION,surface,run_id:id,parent_run_id})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn status_mapping_is_explicit_across_old_product_surfaces(){
        for (host,phase) in [
            ("completed",RunPhase::Succeeded),("awaiting_approval",RunPhase::WaitingApproval),
            ("in_doubt",RunPhase::NeedsReview),("cancelled",RunPhase::Cancelled),
            ("needs_review",RunPhase::NeedsReview),("running",RunPhase::Running)
        ] {assert_eq!(RunPhase::from_host_status(host),Some(phase))}
        assert_eq!(RunPhase::from_host_status("mysterious"),None);
    }
    #[test] fn all_execution_surfaces_share_one_terminal_barrier(){
        for terminal in [RunPhase::Succeeded,RunPhase::Failed,
            RunPhase::Cancelled,RunPhase::Interrupted,RunPhase::NeedsReview]{
            for event in [RunTransition::Start,RunTransition::RequestApproval,
                RunTransition::Verify,RunTransition::Finalize,RunTransition::Fail]{
                assert_eq!(terminal.advance(event),None);
            }
        }
        assert_eq!(RunPhase::Running.advance(RunTransition::RequireReview),
            Some(RunPhase::NeedsReview));
        assert_eq!(RunPhase::NeedsReview.advance(RunTransition::RequireReview),
            Some(RunPhase::NeedsReview));
        assert_eq!(RunPhase::Cancelled.advance(RunTransition::Cancel),
            Some(RunPhase::Cancelled));
    }
    #[test] fn canonical_transition_chain_and_unknown_event_are_explicit(){
        let mut phase=RunPhase::Queued;
        for event in ["start","approval_requested","approval_resolved","verify",
            "finalize","succeed"] {
            phase=phase.advance(RunTransition::from_host_event(event).unwrap()).unwrap();
        }
        assert_eq!(phase,RunPhase::Succeeded);
        assert!(RunTransition::from_host_event("run_unsafe_tool").is_none());
        assert_eq!(RunPhase::Running.advance(RunTransition::Fail),Some(RunPhase::Failed));
        assert_eq!(RunPhase::Queued.advance(RunTransition::Interrupt),
            Some(RunPhase::Interrupted));
    }
    #[test] fn per_room_events_roundtrip_without_rebinding_agent_cursor(){
        let event=RoomEvent{version:1,room_id:"r".into(),
            session_id:"room:r".into(),seq:4,event_id:"e4".into(),
            author:"a".into(),reply_to:Some("e1".into()),
            origin_run:Some("run".into()),kind:"agent_message".into()};
        let raw=serde_json::to_value(&event).unwrap();
        assert_eq!(raw["originRun"],"run");
        assert_eq!(serde_json::from_value::<RoomEvent>(raw).unwrap(),event);
        let binding=RoomAgentBinding{room_id:"r".into(),agent_id:"b".into(),
            session_id:"room:r".into(),read_cursor:2,state:"bound".into()};
        assert_eq!(serde_json::to_value(binding).unwrap()["readCursor"],2);
    }
    #[test] fn run_event_binds_global_sequence_phase_and_parent(){
        let reference=RunRef::new(RunSurface::Room,"room-1",Some("team".into())).unwrap();
        assert!(RunEvent::new(0,reference.clone(),RunPhase::Running,"record","job").is_err());
        assert!(RunEvent::new(1,reference.clone(),RunPhase::Running,"record","unknown").is_err());
        let event=RunEvent::new(84,reference,RunPhase::WaitingApproval,
            "record","execution").unwrap();
        let encoded=serde_json::to_value(&event).unwrap();
        assert_eq!(encoded["eventSeq"],84);
        assert_eq!(encoded["runRef"]["parentRunId"],"team");
        assert_eq!(serde_json::from_value::<RunEvent>(encoded).unwrap(),event);
    }
    #[test] fn typed_ref_roundtrips_without_unscoped_parent(){
        let r=RunRef::new(RunSurface::Team,"job:123",Some("team:0".into())).unwrap();
        let restored:RunRef=serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(restored,r);
        assert!(RunRef::new(RunSurface::Team,"same",Some("same".into())).is_err());
    }
}
