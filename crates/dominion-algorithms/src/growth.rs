//! Agent-local evidence aggregation and guidance; no storage or model calls.
use serde_json::{json,Value};
fn stable_guidance(id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "revision": crate::memory::stable_id(&[id, text]),
        "text": text,
    })
}


pub fn update(growth:&mut Value,agent_id:&str,execution:&Value,operations:&[Value],now:&str){
    growth["runs"] = json!(growth["runs"].as_u64().unwrap_or(0) + 1);
    match execution["taskOutcome"].as_str() {
        Some("succeeded") => {
            growth["succeeded"] = json!(growth["succeeded"].as_u64().unwrap_or(0) + 1)
        }
        Some("failed") => {
            growth["failed"] = json!(growth["failed"].as_u64().unwrap_or(0) + 1)
        }
        Some("answered_unverified") => {
            growth["answeredUnverified"] =
                json!(growth["answeredUnverified"].as_u64().unwrap_or(0) + 1);
            growth["unrated"] = json!(growth["unrated"].as_u64().unwrap_or(0) + 1);
        }
        _ => {
            // Cancelled/unknown/interrupted runs are not failures. Counting
            // them as failures made every chat-only bot look like it never
            // succeeded and then injected that fake rate back into its prompt.
            growth["unrated"] = json!(growth["unrated"].as_u64().unwrap_or(0) + 1);
        }
    }
    if let Some(class) = execution["failureClass"]
        .as_str()
        .filter(|value| !value.is_empty() && execution["taskOutcome"] == "failed")
    {
        // Legacy records may carry a stale failureClass from an ordinary
        // unverified chat or a cancellation. Never turn it into guidance.
        let count = growth["failureClasses"][class].as_u64().unwrap_or(0) + 1;
        growth["failureClasses"][class] = json!(count);
        let hint = execution["recoveryHint"].as_str().unwrap_or("");
        if count >= 2 {
            let text = if hint.is_empty() {
                format!("`{class}` 실패가 {count}회 관찰되었습니다. 같은 접근을 반복하지 말고 원인을 먼저 확인하세요.")
            } else {
                format!("`{class}` 실패가 {count}회 관찰되었습니다. {hint}")
            };
            add_guidance(growth, &format!("failure:{agent_id}:{class}"), &text, count,
                "failed_execution");
        }
    }
    let run_id = execution["id"].as_str().unwrap_or("");
    for operation in operations {
        if operation["runId"].as_str() != Some(run_id) {
            continue;
        }
        let Some(tool) = operation["tool"].as_str() else {
            continue;
        };
        let uses = growth["toolUses"][tool].as_u64().unwrap_or(0) + 1;
        growth["toolUses"][tool] = json!(uses);
        if operation["status"].as_str() == Some("failed") {
            let failures = growth["toolFailures"][tool].as_u64().unwrap_or(0) + 1;
            growth["toolFailures"][tool] = json!(failures);
            if failures >= 2 {
                add_guidance(
                    growth,
                    &format!("tool:{agent_id}:{tool}"),
                    &format!(
                        "`{tool}` 실패가 {failures}회 관찰되었습니다. 같은 인자를 반복하지 말고 다른 도구나 더 단순한 인자를 사용하세요."
                    ),
                    failures,
                    "failed_tool_receipt",
                );
            }
        }
    }
    let rated_runs = growth["succeeded"].as_u64().unwrap_or(0)
        + growth["failed"].as_u64().unwrap_or(0);
    growth["ratedRuns"] = json!(rated_runs);
    growth["successRate"] = json!(if rated_runs > 0 {
        growth["succeeded"].as_u64().unwrap_or(0) as f64 / rated_runs as f64
    } else {
        0.0
    });
    growth["updatedAt"] = json!(now);
}
fn add_guidance(growth: &mut Value, id: &str, text: &str, evidence: u64, basis: &str) {
    let mut guidance = growth["guidance"].as_array().cloned().unwrap_or_default();
    if let Some(existing) = guidance
        .iter_mut()
        .find(|item| item["id"].as_str() == Some(id))
    {
        *existing = stable_guidance(id, text);
        existing["evidenceCount"] = json!(evidence);
        existing["evidenceBasis"] = json!(basis);
        existing["confidence"] = json!(
            (evidence as f64 / growth["runs"].as_u64().unwrap_or(1).max(1) as f64)
                .min(1.0)
        );
    } else {
        let mut record = stable_guidance(id, text);
        record["evidenceCount"] = json!(evidence);
        record["evidenceBasis"] = json!(basis);
        record["confidence"] = json!(
            (evidence as f64 / growth["runs"].as_u64().unwrap_or(1).max(1) as f64)
                .min(1.0)
        );
        guidance.push(record);
    }
    guidance.sort_by(|a, b| {
        b["evidenceCount"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["evidenceCount"].as_u64().unwrap_or(0))
    });
    if guidance.len() > 8 {
        // Keep the highest-evidence guidance. split_off would keep the tail.
        guidance.truncate(8);
    }
    growth["guidance"] = json!(guidance);
}


pub fn context_block(growth:&Value)->Option<(String,Vec<Value>)>{
    let runs = growth["runs"].as_u64().unwrap_or(0);
    if runs == 0 {
        return None;
    }
    let guidance = growth["guidance"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item["confidence"].as_f64().unwrap_or(0.0) >= 0.15)
        // Legacy failure advice could be synthesized from an unverified chat.
        // Require a modern failed-execution provenance marker before replay.
        // Historical DB rows are preserved for inspection/migration.
        .filter(|item| !item["id"].as_str()
            .is_some_and(|id| id.starts_with("failure:"))
            || item["evidenceBasis"] == "failed_execution")
        .take(3)
        .collect::<Vec<_>>();
    // Execution counters change every ordinary chat turn. They are useful
    // for the dashboard, but injecting them as prompts invalidates exact
    // provider prefixes and wastes tokens even when no new lesson exists.
    if guidance.is_empty() { return None; }
    let mut block=String::from("Agent growth guidance (verified lessons only):");
    for item in &guidance {
        if let Some(text)=item["text"].as_str().filter(|text|!text.is_empty()) {
            block.push_str("\n- ");
            block.push_str(text);
        }
    }
    let items = guidance
        .iter()
        .map(|item| {
            json!({
                "kind": "agent_growth",
                "id": item["id"],
                "revision": item["revision"],
            })
        })
        .collect();
    Some((block, items))
}


#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn unknown_outcomes_do_not_become_failure_guidance(){
        let mut record=json!({"guidance":[],"failureClasses":{},"toolUses":{},"toolFailures":{}});
        let execution=json!({"id":"r","taskOutcome":"answered_unverified","failureClass":"stale"});
        update(&mut record,"agent",&execution,&[],"fixed");
        update(&mut record,"agent",&execution,&[],"fixed");
        assert_eq!(record["unrated"],2);
        assert!(context_block(&record).is_none());
        assert!(record["failureClasses"]["stale"].is_null());
    }
}
