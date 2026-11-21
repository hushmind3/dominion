//! H4 RunCoordinator phase reducer.
//!
//! The execution record's `status` is the durable outcome; `phase` is the
//! lifecycle position used by recovery. Transitions are explicit so a crashed
//! or duplicated run cannot jump from a terminal state back into executing.
use crate::state::AppState;
use serde_json::{json, Value};

pub const QUEUED: &str = "queued";
pub const RUNNING: &str = "running";
pub const WAITING_APPROVAL: &str = "waiting_approval";
pub const VERIFYING: &str = "verifying";
pub const FINALIZING: &str = "finalizing";
pub const SUCCEEDED: &str = "succeeded";
pub const FAILED: &str = "failed";
pub const CANCELLED: &str = "cancelled";
pub const INTERRUPTED: &str = "interrupted";

/// The portable kernel owns the lifecycle language. Product adapters
/// translate only the host event name, never implement their own state rules.
pub fn is_terminal(phase:&str)->bool {
    dominion_agent_sdk::RunPhase::from_host_status(phase)
        .is_some_and(|phase|phase.terminal())
}

/// A deterministic adapter onto the portable RunPhase reducer.
pub fn transition(current:&str,event:&str)->Option<&'static str>{
    use dominion_agent_sdk::runtime::run_contract::RunTransition;
    let phase=dominion_agent_sdk::RunPhase::from_host_status(current)?;
    let transition=RunTransition::from_host_event(event)?;
    phase.advance(transition).map(|next|next.as_str())
}

pub fn phase_for_status(status:&str)->&'static str{
    dominion_agent_sdk::RunPhase::from_host_status(status)
        .unwrap_or(dominion_agent_sdk::RunPhase::NeedsReview).as_str()
}

/// Applies one event to the durable execution record. An illegal transition is
/// audited and leaves the phase unchanged instead of corrupting recovery.
pub fn apply(state: &AppState, execution_id: &str, event: &str) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let current = execution["phase"].as_str().unwrap_or(QUEUED).to_string();
    // Status is the durable outcome, even when a migrated execution still
    // carries a stale non-terminal phase. A caller cannot revive it.
    let outcome = execution["status"].as_str().unwrap_or("");
    let committed_terminal = is_terminal(outcome);
    let next = if committed_terminal {None} else {transition(&current,event)};
    let Some(next) = next else {
        state.store.audit(
            "run.phase_rejected",
            json!({"executionId": execution_id, "phase": current, "event": event}),
        );
        return Some(execution);
    };
    if next == current {
        return Some(execution);
    }
    let expected_status=execution["status"].clone();
    let expected_phase=execution["phase"].clone();
    let patch=json!({"phase":next,
        "phaseUpdatedAt":chrono::Utc::now().to_rfc3339()});
    let Some(saved)=state.store.patch_if_all(
        "execution",execution_id,
        &[("status",&expected_status),("phase",&expected_phase)],&patch)
    else{
        // Another worker/cancellation changed the authoritative record.
        // Return the latest state without re-emitting a stale transition.
        state.store.audit("run.phase_race",
            json!({"executionId":execution_id,"oldPhase":current,"event":event}));
        return state.store.get("execution",execution_id);
    };
    state.bus.record("execution", &saved);
    Some(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_cannot_restart_after_a_terminal_phase() {
        let mut phase = QUEUED;
        for event in ["start", "approval_requested", "approval_resolved", "verify", "finalize", "succeed"] {
            phase = transition(phase, event).expect(event);
        }
        assert_eq!(phase, SUCCEEDED);
        for event in ["start", "finalize", "interrupt"] {
            assert!(transition(phase, event).is_none(), "{event}");
        }
    }

    #[test]
    fn crash_recovery_can_interrupt_running_and_waiting_runs() {
        assert_eq!(transition(RUNNING, "interrupt"), Some(INTERRUPTED));
        assert_eq!(
            transition(WAITING_APPROVAL, "interrupt"),
            Some(INTERRUPTED)
        );
        assert_eq!(transition(FINALIZING, "interrupt"), Some(INTERRUPTED));
        assert!(transition(INTERRUPTED, "start").is_none());
    }

    #[test]
    fn review_is_terminal_and_unknown_phase_fails_closed(){
        assert!(is_terminal("needs_review"));
        assert!(is_terminal("in_doubt"));
        assert!(!is_terminal("running"));
        assert_eq!(phase_for_status("future_vendor_status"),"needs_review");
        assert_eq!(transition("needs_review","start"),None);
        assert_eq!(transition("running","review_requested"),Some("needs_review"));
    }
    #[test]
    fn persisted_terminal_status_cannot_be_revived_by_stale_phase(){
        use crate::state::AppState;
        let dir=std::env::temp_dir().join(format!(
            "dominion-run-reducer-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        state.store.put("execution",&json!({
            "id":"finished","status":"completed","phase":"running"})).unwrap();
        let result=apply(&state,"finished","start").unwrap();
        assert_eq!(result["phase"],"running");
        assert_eq!(result["status"],"completed");
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn final_status_maps_to_one_terminal_phase() {
        assert_eq!(phase_for_status("completed"), SUCCEEDED);
        assert_eq!(phase_for_status("cancelled"), CANCELLED);
        assert_eq!(phase_for_status("interrupted"), INTERRUPTED);
        assert_eq!(phase_for_status("failed"), FAILED);
    }
}
