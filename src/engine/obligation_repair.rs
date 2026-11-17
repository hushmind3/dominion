//! Dominion obligation-to-receipt lineage repair.
//!
//! Built for the 2026 evidence-attribution / trace-guided harness research
//! setting, not a copy of a published control loop. The primitive is a
//! *violated postcondition*, never a model-selected action or a reward score.
//! It links only attributable execution receipts on the very same resource.
//! A successful tool receipt can be counter-evidence when its postcondition
//! still fails. Unknown external effects remain blocked for review.
//!
//! No candidate scoring, model-only success claims or autonomous effect replay.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

const MAX_OPERATIONS: usize = 96;
const MAX_FRONTIERS: usize = 12;
const MAX_CHAIN: usize = 8;

/// Preserve path typing. Workspace and absolute-file operations do not
/// become equivalent just because their basenames happen to match.
fn check_resource(check: &Value) -> Option<String> {
    let path = check["path"].as_str().filter(|p| !p.is_empty())?;
    let kind = check["type"].as_str()?;
    if !kind.starts_with("file_") { return None; }
    let tool = if path.starts_with('/') || path.chars().nth(1) == Some(':') {
        "file_read"
    } else {
        "workspace_read"
    };
    crate::engine::effect::resource_key(tool, &json!({"path":path}))
}

fn operation_resource(operation: &Value) -> Option<String> {
    operation["resourceKey"].as_str().map(str::to_string).or_else(|| {
        crate::engine::effect::resource_key(
            operation["tool"].as_str()?, &operation["arguments"]
        )
    })
}

/// An obligation belongs here only if the deterministic verifier reported
/// that exact failed criterion. A path appearing in free-form user prose
/// cannot fabricate a failing postcondition.
fn check_unmet(check: &Value, gaps: &[String]) -> bool {
    let serialized = check.to_string();
    if gaps.iter().any(|gap|
        gap.strip_prefix("검증 조건 미충족: ")
            .is_some_and(|content|content == serialized)
    ) { return true; }
    if check["type"] == "file_replace_exact" {
        if let Some(path) = check["path"].as_str() {
            return gaps.iter().any(|gap|
                gap == &format!("The exact replacement in {path} has not been verified. Preserve every other byte of the original file.")
            );
        }
    }
    false
}

fn unknown_effect(operation: &Value) -> bool {
    operation["outcomeUnknown"] == true
        || matches!(operation["status"].as_str(), Some("running" | "interrupted"))
        || matches!(operation["effectState"].as_str(), Some("needs_review" | "effect_unknown"))
}
fn is_effect(operation: &Value) -> bool {
    let tool=operation["tool"].as_str().unwrap_or("");
    crate::engine::effect::is_effectful(tool,&operation["arguments"])
}
fn receipt_id(operation: &Value) -> Option<String> {
    operation["id"].as_str().map(str::to_string)
}
fn sorted_operations(operations: &[Value]) -> Vec<&Value> {
    let mut selected=operations.iter().rev().take(MAX_OPERATIONS)
        .collect::<Vec<_>>();
    selected.sort_by(|a,b|
        a["opSeq"].as_u64().unwrap_or(0).cmp(&b["opSeq"].as_u64().unwrap_or(0))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    );
    selected
}

fn frontier_kind(operations: &[&Value]) -> &'static str {
    if operations.iter().any(|op| unknown_effect(op) && is_effect(op)) {
        return "unresolved_external_effect";
    }
    if operations.iter().any(|op| is_effect(op)
        && op["status"] == "failed") {
        return "failed_mutation";
    }
    if let Some(last_effect) = operations.iter().rev().find(|op|
        is_effect(op) && op["status"] == "completed") {
        let after = operations.iter().any(|op| {
            !is_effect(op) && op["status"] == "completed"
                && op["opSeq"].as_u64().unwrap_or(0)
                    > last_effect["opSeq"].as_u64().unwrap_or(u64::MAX)
        });
        return if after { "postcondition_contradicts_mutation" }
            else { "mutation_needs_independent_verification" };
    }
    if !operations.is_empty() { "observation_without_effect" }
    else { "no_attributable_receipt" }
}

fn next_action(kind: &str, resource: &str, receipt_ids: &[String]) -> String {
    let refs=receipt_ids.join(", ");
    match kind {
        "unresolved_external_effect" => format!(
            "UNCERTAIN EFFECT on {resource}. Inspect/reconcile prior operation IDs \
             [{refs}] before authorizing any overlapping mutation or retry."
        ),
        "failed_mutation" => format!(
            "FAILED EFFECT on {resource} ({refs}). Diagnose its actual error \
             and change the approach; repeating unchanged arguments is not evidence."
        ),
        "postcondition_contradicts_mutation" => format!(
            "CONTRADICTORY RECEIPTS on {resource} ({refs}): a completed mutation \
             and a later observation exist, yet its frozen verifier remains false. \
             Inspect content/version and recompose only this resource's actions."
        ),
        "mutation_needs_independent_verification" => format!(
            "UNPROVEN EFFECT on {resource} ({refs}). Tool completion is insufficient: \
             read back the artifact and compare it with the required postcondition \
             before further mutation."
        ),
        "observation_without_effect" => format!(
            "OBSERVATION-ONLY on {resource} ({refs}). Read receipts do not satisfy \
             a missing output. Plan the authorized effect and independently verify."
        ),
        _ => format!(
            "NO ATTRIBUTABLE RECEIPT for {resource}. Inspect the actual target, \
             establish its present state and address the frozen missing condition."
        ),
    }
}

/// Reconstructs a resource-scoped proof frontier from VERIFIED FAILED
/// postconditions and current run's native receipts.
/// Returns both machine-readable provenance edges and a bounded local
/// replan directive. It NEVER issues tools or reclassifies goal completion.
pub fn reconstruct(checks: &Value, gaps: &[String], operations: &[Value]) -> Value {
    let mut grouped: BTreeMap<String, Vec<Value>>=BTreeMap::new();
    for check in checks.as_array().into_iter().flatten() {
        if !check_unmet(check,gaps) { continue; }
        let Some(resource)=check_resource(check) else { continue; };
        grouped.entry(resource).or_default().push(json!({
            "type":check["type"],
            "path":check["path"],
            "expectedSha256":check["expectedSha256"],
            "baseline":check["baseline"],
        }));
    }
    let ordered=sorted_operations(operations);
    let mut frontiers=Vec::new();
    let mut graph_nodes=Vec::new();
    let mut graph_edges=Vec::new();
    let mut graph_ids=HashSet::new();
    for (index,(resource,obligations)) in grouped.iter().take(MAX_FRONTIERS).enumerate() {
        let mut chain=ordered.iter().copied().filter(|op|
            operation_resource(op).as_deref()==Some(resource)
        ).collect::<Vec<_>>();
        if chain.len()>MAX_CHAIN {
            chain=chain.split_off(chain.len()-MAX_CHAIN);
        }
        let kind=frontier_kind(&chain);
        let ids=chain.iter().filter_map(|op|receipt_id(op)).collect::<Vec<_>>();
        let obligation_id=format!("obligation:{index}");
        graph_nodes.push(json!({
            "id":obligation_id,"kind":"failed_postcondition",
            "resource":resource,"clauses":obligations,
        }));
        let mut previous:Option<String>=None;
        for operation in &chain {
            let Some(id)=receipt_id(operation) else {continue};
            let node_id=format!("receipt:{id}");
            if graph_ids.insert(node_id.clone()) {
                graph_nodes.push(json!({
                    "id":node_id,"kind":"execution_receipt",
                    "tool":operation["tool"], "resource":resource,
                    "status":operation["status"],"opSeq":operation["opSeq"],
                    "effectState":operation["effectState"],
                    "observationVersion":operation["observedVersion"],
                    "receiptId":id
                }));
            }
            if let Some(source)=&previous {
                graph_edges.push(json!({
                    "from":source,"to":node_id,"type":"same_resource_temporal"
                }));
            }
            previous=Some(node_id);
        }
        if let Some(last)=previous {
            graph_edges.push(json!({
                "from":last,"to":obligation_id,
                "type":"receipt_does_not_discharge_postcondition"
            }));
        }
        frontiers.push(json!({
            "resource":resource,"kind":kind,
            "unmetObligations":obligations,"receiptIds":ids,
            "directive":next_action(kind,resource,&ids),
        }));
    }
    let specific=frontiers.iter().map(|item|
        item["directive"].as_str().unwrap_or("")
    ).take(3).collect::<Vec<_>>().join("\n");
    json!({
        "schema":"dominion.obligation_lineage.v1",
        "frontiers":frontiers,
        "graph":{"nodes":graph_nodes,"edges":graph_edges},
        "directive":specific,
        "source":"deterministic_verifier_plus_persisted_execution_receipts",
        "verified":false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap(check:&Value)->String {
        format!("검증 조건 미충족: {}",check)
    }

    #[test]
    fn a_successful_write_is_not_success_if_the_original_verifier_still_fails() {
        let check=json!({"type":"file_contains","path":"report.md","contains":"ready"});
        let ops=vec![
            json!({"id":"read","tool":"workspace_read",
                "arguments":{"path":"report.md"},"opSeq":1,"status":"completed"}),
            json!({"id":"write","tool":"workspace_write",
                "arguments":{"path":"report.md","content":"wrong"},"opSeq":2,"status":"completed"}),
            json!({"id":"reread","tool":"workspace_read",
                "arguments":{"path":"report.md"},"opSeq":3,"status":"completed"}),
            json!({"id":"unrelated","tool":"workspace_write",
                "arguments":{"path":"elsewhere.md"},"opSeq":4,"status":"completed"}),
        ];
        let g=reconstruct(&json!([check.clone()]),&[gap(&check)],&ops);
        let frontier=&g["frontiers"][0];
        assert_eq!(frontier["kind"],"postcondition_contradicts_mutation");
        assert_eq!(frontier["receiptIds"],json!(["read","write","reread"]));
        assert_eq!(g["graph"]["edges"].as_array().unwrap().len(),3);
        assert!(g["directive"].as_str().unwrap().contains("frozen verifier remains false"));
        assert_eq!(g["verified"],false);
    }

    #[test]
    fn unresolved_prior_effect_prevents_direct_retry_guidance() {
        let check=json!({"type":"file_exists","path":"output.json"});
        let ops=vec![json!({
            "id":"unsafe","tool":"workspace_write",
            "arguments":{"path":"output.json","content":"{}"},
            "status":"interrupted","outcomeUnknown":true,"opSeq":1
        })];
        let result=reconstruct(&json!([check.clone()]),&[gap(&check)],&ops);
        assert_eq!(result["frontiers"][0]["kind"],"unresolved_external_effect");
        assert!(result["directive"].as_str().unwrap().contains("Inspect/reconcile"));
    }

    #[test]
    fn independent_targets_cannot_supply_a_fake_proof() {
        let check=json!({"type":"file_equals","path":"expected.rs","content":"ok"});
        let ops=vec![json!({"id":"foreign","tool":"workspace_write",
            "arguments":{"path":"different.rs","content":"ok"},"opSeq":1,"status":"completed"})];
        let result=reconstruct(&json!([check.clone()]),&[gap(&check)],&ops);
        assert_eq!(result["frontiers"][0]["kind"],"no_attributable_receipt");
        assert!(result["frontiers"][0]["receiptIds"].as_array().unwrap().is_empty());
        assert_eq!(result["graph"]["edges"],json!([]));
    }

    #[test]
    fn satisfied_checks_do_not_appear_as_failed_obligations() {
        let a=json!({"type":"file_exists","path":"a.rs"});
        let b=json!({"type":"file_exists","path":"b.rs"});
        let result=reconstruct(&json!([a.clone(),b.clone()]),&[gap(&b)],&[]);
        assert_eq!(result["frontiers"].as_array().unwrap().len(),1);
        assert_eq!(result["frontiers"][0]["resource"],"workspace:b.rs");
    }

    #[test]
    fn two_failed_criteria_for_one_resource_share_the_same_receipt_lineage() {
        let a=json!({"type":"file_exists","path":"x.rs"});
        let b=json!({"type":"file_contains","path":"x.rs","contains":"ready"});
        let result=reconstruct(&json!([a.clone(),b.clone()]),&[gap(&a),gap(&b)],&[]);
        assert_eq!(result["frontiers"].as_array().unwrap().len(),1);
        assert_eq!(result["frontiers"][0]["unmetObligations"].as_array().unwrap().len(),2);
    }

    #[test]
    fn only_verifier_failures_can_open_a_repair_frontier() {
        let check=json!({"type":"file_exists","path":"x.rs"});
        let result=reconstruct(&json!([check]),&["free-form model said failure".to_string()],&[]);
        assert!(result["frontiers"].as_array().unwrap().is_empty());
    }
}
