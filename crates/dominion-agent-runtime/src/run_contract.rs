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
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RunRef {
    pub version:u16,
    pub surface:RunSurface,
    pub run_id:String,
    pub parent_run_id:Option<String>,
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
    #[test] fn typed_ref_roundtrips_without_unscoped_parent(){
        let r=RunRef::new(RunSurface::Team,"job:123",Some("team:0".into())).unwrap();
        let restored:RunRef=serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(restored,r);
        assert!(RunRef::new(RunSurface::Team,"same",Some("same".into())).is_err());
    }
}
