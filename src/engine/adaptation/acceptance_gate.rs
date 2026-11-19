//! Evidence-gated repair acceptance.
//!
//! 2026 harness composition, kept inside the existing execution path:
//! Harness²: proposals/probes are compared through actual resulting evidence;
//! DeepSeek: the native permission/effect journal owns the dispatched tool;
//! PRISM: a candidate is retained only when frozen acceptance improves without
//! regressing any previously satisfied contract. This is per-task validation,
//! not a claim that PRISM's held-out benchmark optimizer was reproduced.
//!
//! No additional autonomous loop, no learned arbitrary score, no tool replay.
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn set(gaps: &[String]) -> BTreeSet<&str> {
    gaps.iter().map(String::as_str).collect()
}

/// Compare an executed repair to the *same frozen acceptance contract* before
/// and after a tool call. Tool receipts establish provenance/authority, NOT
/// acceptance. Every previously satisfied check is a no-regression constraint.
/// Missing, unresolved, interrupted, or uncertain receipts cannot be promoted.
pub fn assess(before: &[String], after: &[String], operation: Option<&Value>) -> Value {
    let before_set = set(before);
    let after_set = set(after);
    let resolved = before_set.difference(&after_set).copied().collect::<Vec<_>>();
    let regressed = after_set.difference(&before_set).copied().collect::<Vec<_>>();
    let receipt_id = operation.and_then(|op| op["id"].as_str());
    let status = operation.and_then(|op| op["status"].as_str()).unwrap_or("missing");
    let effect_state = operation.and_then(|op|op["effectState"].as_str()).unwrap_or("");
    let unresolved_effect = operation.is_some_and(|op| {
        op["outcomeUnknown"] == true || op["result"]["effectUnknown"] == true
            || matches!(op["effectState"].as_str(),
                Some("effect_unknown" | "needs_review" | "started" | "prepared"))
            || matches!(op["status"].as_str(), Some("running" | "interrupted"))
    });
    let decision = if operation.is_none() {
        "receipt_missing"
    } else if unresolved_effect {
        "unresolved_effect"
    } else if status != "completed" {
        "tool_failed"
    } else if !regressed.is_empty() {
        "regression"
    } else if before_set.is_empty() {
        "already_satisfied"
    } else if resolved.is_empty() {
        "no_proven_progress"
    } else {
        "improved"
    };
    let accepted = decision == "improved";
    json!({
        "policy":"contrastive_probe_native_receipt_no_regression_v1",
        "decision":decision,
        "accepted":accepted,
        "receiptId":receipt_id,
        "receiptStatus":status,
        "effectState":effect_state,
        "beforeGaps":before,
        "afterGaps":after,
        "resolved":resolved,
        "regressed":regressed,
        "basis":"frozen_acceptance_before_after_plus_native_receipt",
    })
}

/// Guard against a mismatched operation accidentally being treated as the
/// proof for another intervention. The proof is scoped to this run and call.
pub fn matching_receipt<'a>(
    operation: Option<&'a Value>,
    run_id: &str,
    call_id: &str,
    tool: &str,
    arguments: &Value,
) -> Option<&'a Value> {
    let record=operation?;
    (record["runId"].as_str() == Some(run_id)
        && record["callId"].as_str() == Some(call_id)
        && record["tool"].as_str() == Some(tool)
        && record["arguments"] == *arguments
    ).then_some(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation(status: &str) -> Value {
        json!({"id":"op-1","runId":"run","callId":"call",
            "tool":"workspace_write","arguments":{"path":"out.txt","content":"ok"},
            "status":status,"effectState":"verified","toolReceipt":{"verified":true}})
    }

    #[test]
    fn contrastive_probes_accept_real_progress_without_holdout_regression() {
        let before=vec!["missing output".into(),"test failed".into()];
        let after=vec!["test failed".into()];
        let gate=assess(&before,&after,Some(&operation("completed")));
        assert_eq!(gate["decision"],"improved");
        assert_eq!(gate["resolved"],json!(["missing output"]));
        assert!(gate["regressed"].as_array().unwrap().is_empty());
        assert_eq!(gate["accepted"],true);
    }

    #[test]
    fn new_failure_invalidates_candidate_despite_other_resolved_checks() {
        let before=vec!["file missing".into()];
        let after=vec!["test failure".into()];
        let gate=assess(&before,&after,Some(&operation("completed")));
        assert_eq!(gate["decision"],"regression");
        assert_eq!(gate["resolved"],json!(["file missing"]));
        assert_eq!(gate["regressed"],json!(["test failure"]));
        assert_eq!(gate["accepted"],false);
    }

    #[test]
    fn successful_transport_without_postcondition_change_is_not_adopted() {
        let before=vec!["artifact hash wrong".into()];
        let gate=assess(&before,&before,Some(&operation("completed")));
        assert_eq!(gate["decision"],"no_proven_progress");
        assert_eq!(gate["accepted"],false);
    }

    #[test]
    fn unknown_side_effect_blocks_selection_even_when_file_seems_fixed() {
        let mut op=operation("completed");
        op["effectState"]=json!("effect_unknown");
        assert_eq!(assess(&["missing".into()],&[],Some(&op))["decision"],
            "unresolved_effect");
        op["effectState"]=json!("verified");
        op["result"]=json!({"effectUnknown":true});
        assert_eq!(assess(&["missing".into()],&[],Some(&op))["decision"],
            "unresolved_effect");
        assert_eq!(assess(&["missing".into()],&[],None)["decision"],
            "receipt_missing");
    }

    #[test]
    fn cannot_borrow_a_receipt_from_a_different_run_or_tool() {
        let op=operation("completed");
        let args=json!({"path":"out.txt","content":"ok"});
        assert!(matching_receipt(Some(&op),"run","call","workspace_write",&args).is_some());
        assert!(matching_receipt(Some(&op),"foreign","call","workspace_write",&args).is_none());
        assert!(matching_receipt(Some(&op),"run","other","workspace_write",&args).is_none());
        assert!(matching_receipt(Some(&op),"run","call","workspace_read",&args).is_none());
        assert!(matching_receipt(Some(&op),"run","call","workspace_write",
            &json!({"path":"different.txt"})).is_none());
    }

    #[test]
    fn held_baseline_stays_satisfied_without_fake_improvement() {
        let gate=assess(&[],&[],Some(&operation("completed")));
        assert_eq!(gate["decision"],"already_satisfied");
        assert_eq!(gate["accepted"],false);
    }

    #[test]
    fn physical_verifier_gates_a_successful_write_instead_of_the_model_claim() {
        use crate::{config::Config, db::Store, state::AppState, vault::Vault};
        let dir=std::env::temp_dir().join(format!(
            "dominion-proof-gate-{}",uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let checks=json!([
            {"type":"file_equals","path":"a.txt","content":"EXPECTED"},
            {"type":"file_equals","path":"untouched.txt","content":"PRESERVE"}
        ]);
        std::fs::write(dir.join("untouched.txt"),"PRESERVE").unwrap();
        let evidence=Vec::new();
        let before=crate::engine::verification::check_acceptance(
            &state,"update file",&checks,&evidence
        );
        assert!(before.iter().any(|gap|gap.contains("a.txt")));
        std::fs::write(dir.join("a.txt"),"EXPECTED").unwrap();
        let after=crate::engine::verification::check_acceptance(
            &state,"update file",&checks,&evidence
        );
        let op=operation("completed");
        assert_eq!(assess(&before,&after,Some(&op))["decision"],"improved");
        std::fs::write(dir.join("untouched.txt"),"BROKEN").unwrap();
        let regression=crate::engine::verification::check_acceptance(
            &state,"update file",&checks,&evidence
        );
        assert_eq!(assess(&before,&regression,Some(&op))["decision"],"regression");
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
}
