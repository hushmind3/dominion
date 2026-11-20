//! Read-only compatibility for historical owner-selected runtime policy slots.
//! Native chat still consumes planning/recovery/execution and verified file
//! preflight records. The retired automatic candidate search, model experiments,
//! self-promotion and meta-optimizer are intentionally absent.
use crate::state::AppState;
use crate::engine::preflight_fixture as preflight;
use serde_json::Value;

pub fn is_preflight_kernel(value: &Value) -> bool {
    matches!(value.as_str(), Some("same-file-revision-v1" | preflight::GUARD_KERNEL | preflight::EXACT_KERNEL))
}

/// Recovery retry budgets from the `recovery` slot. Values are clamped so a
/// policy can tune, but not remove, execution safety limits.
pub fn recovery_limits(parameters: &Value) -> (usize, usize) {
    let acceptance = parameters["maxAcceptanceRetries"]
        .as_u64()
        .unwrap_or(2)
        .min(4) as usize;
    let repairs = parameters["maxProcedureRepairs"]
        .as_u64()
        .unwrap_or(2)
        .min(4) as usize;
    (acceptance, repairs)
}

/// Decides whether a planning policy asks for a plan. `None` means the policy
/// does not override the built-in heuristic.
pub fn outline_decision(parameters: &Value, input_chars: usize) -> Option<bool> {
    let outline = parameters["outline"].as_bool()?;
    if !outline {
        return Some(false);
    }
    match parameters["minChars"].as_u64() {
        Some(minimum) => Some(input_chars as u64 >= minimum),
        None => Some(true),
    }
}

pub fn list(state: &AppState) -> Vec<Value> {
    let mut items = state.store.all("adaptation_policy");
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    items
}

pub fn active(state: &AppState, slot: &str) -> Option<Value> {
    state
        .store
        .all("adaptation_policy")
        .into_iter()
        .find(|policy| {
            policy["slot"].as_str() == Some(slot) && policy["status"].as_str() == Some("active")
        })
}

/// A learned guard is selected by the runtime's task contract. Model identities
/// recorded during evaluation are evidence, never live routing keys.
pub fn portable_file_guard(state: &AppState) -> Option<Value> {
    state.store.all("adaptation_policy").into_iter().find(|policy| {
        policy["slot"] == "mutation.preflight"
            && policy["status"] == "active"
            && policy["parameters"]["kernel"] == preflight::GUARD_KERNEL
            && policy["parameters"]["workload"] == preflight::GUARD_WORKLOAD
            && policy["preflightEvidence"].as_object().is_some_and(|evidence| evidence.len() >= 2)
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::{config::Config, db::Store, vault::Vault};

    fn state() -> (AppState, std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!("dominion-policy-readonly-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config {port:0, data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        (AppState::new(config,store,vault),dir)
    }

    #[test]
    fn preserved_policy_read_does_not_promote_old_candidate() {
        let (state,dir)=state();
        state.store.put("adaptation_policy",&json!({
            "id":"old-candidate","slot":"planning","status":"candidate",
            "parameters":{"outline":false},"createdAt":"2026-11-01T00:00:00Z"
        })).unwrap();
        assert!(active(&state,"planning").is_none());
        assert_eq!(list(&state).len(),1,"historical policy remains readable");
        state.store.put("adaptation_policy",&json!({
            "id":"owner-selected","slot":"planning","status":"active",
            "parameters":{"outline":true,"minChars":100}
        })).unwrap();
        assert_eq!(active(&state,"planning").unwrap()["id"],"owner-selected");
        assert_eq!(state.store.get("adaptation_policy","old-candidate").unwrap()["status"],"candidate");
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn runtime_planning_and_recovery_bounds_still_apply() {
        assert_eq!(outline_decision(&json!({"outline":true,"minChars":100}),99),Some(false));
        assert_eq!(outline_decision(&json!({"outline":true,"minChars":100}),100),Some(true));
        assert_eq!(outline_decision(&json!({"outline":false}),999),Some(false));
        assert_eq!(outline_decision(&json!({}),999),None);
        assert_eq!(recovery_limits(&json!({"maxAcceptanceRetries":999,"maxProcedureRepairs":999})),(4,4));
        assert!(is_preflight_kernel(&json!(preflight::GUARD_KERNEL)));
        assert!(!is_preflight_kernel(&json!("unknown")));
    }

    #[test]
    fn old_guard_without_independent_evidence_cannot_be_selected() {
        let (state,dir)=state();
        state.store.put("adaptation_policy",&json!({
            "id":"old-guard","slot":"mutation.preflight","status":"active",
            "parameters":{"kernel":preflight::GUARD_KERNEL,"workload":preflight::GUARD_WORKLOAD},
            "preflightEvidence":{"one":"historical"}
        })).unwrap();
        assert!(portable_file_guard(&state).is_none());
        let mut guard=state.store.get("adaptation_policy","old-guard").unwrap();
        guard["preflightEvidence"]=json!({"one":"verified","two":"verified"});
        state.store.put("adaptation_policy",&guard).unwrap();
        assert!(portable_file_guard(&state).is_some());
        let _=std::fs::remove_dir_all(dir);
    }
}
