//! H4 RunCoordinator phase reducer.
//!
//! The execution record's `status` is the durable outcome; `phase` is the
//! lifecycle position used by recovery. Transitions are explicit so a crashed
//! or duplicated run cannot jump from a terminal state back into executing.
use crate::state::AppState;
use serde_json::{json, Value};
pub use dominion_turn_core::run_state::{
    transition,phase_for_status,is_terminal,QUEUED,RUNNING,WAITING_APPROVAL,
    VERIFYING,FINALIZING,SUCCEEDED,FAILED,CANCELLED,INTERRUPTED,
};



/// Applies one event to the durable execution record. An illegal transition is
/// audited and leaves the phase unchanged instead of corrupting recovery.
pub fn apply(state: &AppState, execution_id: &str, event: &str) -> Option<Value> {
    // A stale read followed by put() can resurrect a terminal run and lose
    // another writer's counters. CAS the phase field under SQLite IMMEDIATE
    // and re-evaluate a raced transition against the actual committed state.
    for _attempt in 0..16 {
        let execution = state.store.get("execution", execution_id)?;
        let expected=execution.get("phase").cloned().unwrap_or(Value::Null);
        let current=expected.as_str().unwrap_or(QUEUED);
        let Some(next)=transition(current,event) else {
            state.store.audit("run.phase_rejected",json!({
                "executionId":execution_id,"phase":current,"event":event,
            }));
            return Some(execution);
        };
        if next==current {return Some(execution);}
        if let Some(saved)=state.store.patch_if("execution",execution_id,
            "phase",&expected,&json!({
                "phase":next,"phaseUpdatedAt":chrono::Utc::now().to_rfc3339(),
            })){
            state.bus.record("execution",&saved);
            return Some(saved);
        }
    }
    state.store.audit("run.phase_cas_contention",json!({
        "executionId":execution_id,"event":event,
    }));
    state.store.get("execution",execution_id)
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
    fn phase_cas_does_not_overwrite_later_fields_or_reopen_terminal_runs(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!(
            "dominion-phase-cas-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let state=AppState::new(config,store,Vault::open(&dir.join("key")));
        state.store.put("execution",&json!({
            "id":"run-1","status":"running","phase":"queued","tokens":41
        })).unwrap();
        let started=apply(&state,"run-1","start").unwrap();
        assert_eq!(started["phase"],RUNNING);
        state.store.patch_if("execution","run-1","tokens",&json!(41),
            &json!({"tokens":91})).unwrap();
        let cancelled=apply(&state,"run-1","cancel").unwrap();
        assert_eq!(cancelled["phase"],CANCELLED);
        let rejected=apply(&state,"run-1","start").unwrap();
        assert_eq!(rejected["phase"],CANCELLED);
        assert_eq!(rejected["tokens"],91);
        let typed=state.bus.replay(0,40).into_iter().filter(|event|
            event["type"]=="run_state"&&event["runId"]=="run-1"
        ).collect::<Vec<_>>();
        assert_eq!(typed.len(),2);
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
