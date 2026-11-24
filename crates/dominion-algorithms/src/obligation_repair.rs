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

pub type ResourceKey = fn(&str,&Value)->Option<String>;
pub type Effectful = fn(&str,&Value)->bool;

const MAX_OPERATIONS: usize = 96;
const MAX_FRONTIERS: usize = 12;
const MAX_CHAIN: usize = 8;

/// Preserve path typing. Workspace and absolute-file operations do not
/// become equivalent just because their basenames happen to match.
fn check_resource(check: &Value, resource_key: ResourceKey) -> Option<String> {
    let path = check["path"].as_str().filter(|p| !p.is_empty())?;
    let kind = check["type"].as_str()?;
    if !kind.starts_with("file_") { return None; }
    let tool = if path.starts_with('/') || path.chars().nth(1) == Some(':') {
        "file_read"
    } else {
        "workspace_read"
    };
    resource_key(tool, &json!({"path":path}))
}

fn operation_resource(operation: &Value, resource_key: ResourceKey) -> Option<String> {
    operation["resourceKey"].as_str().map(str::to_string).or_else(|| {
        resource_key(
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
fn is_effect(operation: &Value, effectful: Effectful) -> bool {
    let tool=operation["tool"].as_str().unwrap_or("");
    effectful(tool,&operation["arguments"])
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

fn frontier_kind(operations: &[&Value], effectful: Effectful) -> &'static str {
    if operations.iter().any(|op| unknown_effect(op) && is_effect(op,effectful)) {
        return "unresolved_external_effect";
    }
    if operations.iter().any(|op| is_effect(op,effectful)
        && op["status"] == "failed") {
        return "failed_mutation";
    }
    if let Some(last_effect) = operations.iter().rev().find(|op|
        is_effect(op,effectful) && op["status"] == "completed") {
        let after = operations.iter().any(|op| {
            !is_effect(op,effectful) && op["status"] == "completed"
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
pub fn reconstruct(checks: &Value, gaps: &[String], operations: &[Value], resource_key: ResourceKey, effectful: Effectful) -> Value {
    let mut grouped: BTreeMap<String, Vec<Value>>=BTreeMap::new();
    for check in checks.as_array().into_iter().flatten() {
        if !check_unmet(check,gaps) { continue; }
        let Some(resource)=check_resource(check,resource_key) else { continue; };
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
            operation_resource(op,resource_key).as_deref()==Some(resource)
        ).collect::<Vec<_>>();
        if chain.len()>MAX_CHAIN {
            chain=chain.split_off(chain.len()-MAX_CHAIN);
        }
        let kind=frontier_kind(&chain,effectful);
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
mod tests{
 use super::*;
 fn key(_: &str,args:&Value)->Option<String>{args["path"].as_str().map(str::to_string)}
 fn effect(tool:&str,_:&Value)->bool{tool=="workspace_write"}
 #[test]
 fn unknown_effect_is_never_treated_as_safe_retry(){
  let check=json!({"type":"file_exists","path":"x"});
  let gaps=vec![format!("검증 조건 미충족: {check}")];
  let ops=vec![json!({"id":"r1","tool":"workspace_write","arguments":{"path":"x"},"status":"interrupted","outcomeUnknown":true})];
  let result=reconstruct(&json!([check]),&gaps,&ops,key,effect);
  assert_eq!(result["frontiers"][0]["kind"],"unresolved_external_effect");
  assert_eq!(result["verified"],false);
 }
}
