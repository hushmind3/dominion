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

pub fn is_terminal(phase: &str) -> bool {
    matches!(phase, SUCCEEDED | FAILED | CANCELLED | INTERRUPTED)
}

/// Returns the next phase, or None for an illegal transition. Repeating the
/// current phase is treated as idempotent.
pub fn transition(current: &str, event: &str) -> Option<&'static str> {
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

/// Applies one event to the durable execution record. An illegal transition is
/// audited and leaves the phase unchanged instead of corrupting recovery.
pub fn apply(state: &AppState, execution_id: &str, event: &str) -> Option<Value> {
    let execution = state.store.get("execution", execution_id)?;
    let stored_phase = execution.get("phase").cloned().unwrap_or(Value::Null);
    let current = stored_phase.as_str().unwrap_or(QUEUED).to_string();
    // A legacy/mid-flight record can have a terminal durable status but a
    // stale phase. Never let a phase update revive its execution payload.
    if matches!(
        execution["status"].as_str(),
        Some("completed" | "failed" | "cancelled" | "interrupted")
    ) {
        return Some(execution);
    }
    let Some(next) = transition(&current, event) else {
        state.store.audit(
            "run.phase_rejected",
            json!({"executionId": execution_id, "phase": current, "event": event}),
        );
        return Some(execution);
    };
    if next == current {
        return Some(execution);
    }
    // Patch under the SQLite record lock. A stale get/put would overwrite
    // cancellation, finalization evidence, or unrelated concurrent updates.
    let patch = json!({
        "phase": next,
        "phaseUpdatedAt": chrono::Utc::now().to_rfc3339(),
    });
    match state.store.patch_if("execution", execution_id, "phase", &stored_phase, &patch) {
        Some(saved) => {
            state.bus.record("execution", &saved);
            Some(saved)
        }
        None => {
            state.store.audit(
                "run.phase_conflict",
                json!({"executionId": execution_id, "phase": current, "event": event}),
            );
            state.store.get("execution", execution_id)
        }
    }
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
    fn phase_reducer_preserves_unrelated_record_fields_and_terminal_status() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir = std::env::temp_dir().join(format!("dominion-phase-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("execution", &json!({
            "id":"run","phase":"queued","status":"running",
            "evidence":{"receipt":"keep-this"},
        }));
        let started = apply(&state, "run", "start").unwrap();
        assert_eq!(started["phase"], RUNNING);
        assert_eq!(started["evidence"]["receipt"], "keep-this");
        state.store.patch_if(
            "execution", "run", "status", &json!("running"),
            &json!({"status":"cancelled","phase":"cancelled"}),
        );
        let terminal = apply(&state, "run", "verify").unwrap();
        assert_eq!(terminal["status"], "cancelled");
        assert_eq!(terminal["phase"], "cancelled");
        assert_eq!(terminal["evidence"]["receipt"], "keep-this");
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn final_status_maps_to_one_terminal_phase() {
        assert_eq!(phase_for_status("completed"), SUCCEEDED);
        assert_eq!(phase_for_status("cancelled"), CANCELLED);
        assert_eq!(phase_for_status("interrupted"), INTERRUPTED);
        assert_eq!(phase_for_status("failed"), FAILED);
    }
}
