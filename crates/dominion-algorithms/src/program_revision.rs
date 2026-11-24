//! Propose a bounded program revision without changing completed effects.
//! The caller still enforces native tool admission, permissions and journaling.
use serde_json::{json,Value};
use dominion_core::program::{validate,validate_node};
pub fn apply_revision_proposal(
    program: &Value,
    checkpoint: &Value,
    proposal: &Value,
    available_tools: &[String],
    max_nodes: usize,
    repair_mode: &str,
) -> Result<Value, String> {
    let base = proposal["baseRevision"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("proposal.baseRevision is required")?;
    if Some(base) != program["revision"].as_str() {
        return Err(format!(
            "proposal baseRevision {base} does not match current revision {}",
            program["revision"]
        ));
    }
    if proposal["reason"].as_str().is_none_or(str::is_empty) {
        return Err("proposal.reason is required".into());
    }
    let completed = checkpoint["completedOperationIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>();
    let mut replacements = proposal["replacePendingNodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if repair_mode == "append" {
        // Append-only repair never rewrites pending nodes; it only adds work.
        replacements.clear();
    }
    let additions = proposal["addNodes"].as_array().cloned().unwrap_or_default();
    if replacements.is_empty() && additions.is_empty() {
        return Err("proposal changes no nodes".into());
    }
    let mut replaced_ids = std::collections::BTreeSet::new();
    for node in &replacements {
        let id = node["id"].as_str().filter(|value| !value.is_empty())
            .ok_or("replacement node needs an id")?;
        if completed.contains(id) {
            return Err(format!(
                "cannot replace completed node {id}; completed effects are preserved"
            ));
        }
        validate_node(node)?;
        if !replaced_ids.insert(id.to_string()) {
            return Err(format!("duplicate replacement node id {id}"));
        }
    }
    let mut nodes = Vec::new();
    for node in program["nodes"].as_array().into_iter().flatten() {
        let id = node["id"].as_str().unwrap_or("");
        if let Some(replacement) = replacements.iter().find(|item| item["id"].as_str() == Some(id)) {
            nodes.push(replacement.clone());
        } else {
            nodes.push(node.clone());
        }
    }
    for node in &additions {
        validate_node(node)?;
        nodes.push(node.clone());
    }

    let mut ids = std::collections::BTreeSet::new();
    let mut known = checkpoint["resolvedInputs"]
        .as_object()
        .map(|map| map.keys().cloned().collect::<std::collections::BTreeSet<_>>())
        .unwrap_or_default();
    for node in &nodes {
        let id = node["id"].as_str().filter(|value| !value.is_empty())
            .ok_or("every program node needs an id")?;
        if !ids.insert(id.to_string()) {
            return Err(format!("duplicate program node id {id}"));
        }
        if node["type"].as_str() == Some("call_tool") {
            let tool = node["tool"].as_str().unwrap_or("");
            if !available_tools.iter().any(|item| item == tool) {
                return Err(format!("tool {tool} is not available in this run"));
            }
        }
        // Every `{from}` reference must point to a prior node or a resolved observation.
        if let Some(inputs) = node["inputs"].as_object() {
            for (key, input) in inputs {
                if let Some(source) = input.get("from").and_then(Value::as_str) {
                    if !known.contains(source) && !ids.contains(source) {
                        return Err(format!(
                            "input {key} references unknown observation/node {source}"
                        ));
                    }
                }
            }
        }
        known.insert(id.to_string());
    }
    if nodes.len() > max_nodes {
        return Err(format!(
            "revised program has {} nodes, budget is {max_nodes}",
            nodes.len()
        ));
    }
    validate(&json!({
        "revision": format!("{}:rev", base),
        "nodes": nodes,
    }))?;
    Ok(json!({
        "revision": format!("{}:rev:{}", base, crate::memory::stable_id(&[&proposal.to_string()])),
        "nodes": nodes,
        "appliedProposal": proposal,
        "parentRevision": base,
    }))
}


#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn completed_node_cannot_be_replaced(){
  let original=json!({"revision":"p","nodes":[{"id":"done","type":"call_tool","tool":"workspace_write","inputs":{"path":"a"}}]});
  let checkpoint=json!({"completedOperationIds":["done"]});
  let proposal=json!({"baseRevision":"p","reason":"retry","replacePendingNodes":[{"id":"done","type":"call_tool","tool":"workspace_write","inputs":{"path":"b"}}]});
  assert!(apply_revision_proposal(&original,&checkpoint,&proposal,&["workspace_write".into()],8,"replace").is_err());
 }
}
