//! Task-conditioned diagnostic decisions. This is a pure decision layer, not
//! another execution loop: tools still pass through the native journal/gateway.
//! Model predictions are hypotheses; only a recorded tool result is evidence.
use serde_json::{json, Value};
use std::collections::HashSet;
use crate::engine::program::{predicate_verdict, PredicateVerdict};

/// Stable identity for one observation request. A repeated tool name with
/// different inputs is a different experiment.
pub fn signature(tool: &str, inputs: &Value) -> String {
    format!("{tool}:{}", inputs)
}

fn independent_read(candidate: &Value) -> bool {
    let Some(tool) = candidate["tool"].as_str() else { return false; };
    if !matches!(tool, "workspace_read" | "workspace_read_range" | "workspace_list") {
        return false;
    }
    let Some(inputs) = candidate["inputs"].as_object() else { return false; };
    if inputs.values().any(|value| value.get("from").is_some()) {
        return false;
    }
    // Autonomous diagnostics must not wander into arbitrary files or
    // credentials. Workspace tools already enforce their sandbox boundary.
    if !inputs.get("path").and_then(Value::as_str).is_some_and(|path| {
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains("..")
            && !path.split(['/', '\\']).any(|part| matches!(part,
                ".env" | ".ssh" | ".git" | "secrets" | "credentials"
                | "id_rsa" | "id_ed25519"))
    }) { return false; }
    if !crate::engine::effect::is_observation_only(tool, &candidate["inputs"]) {
        return false;
    }
    let predicate = &candidate["predicate"];
    predicate["from"] == "result"
        && matches!(predicate["type"].as_str(), Some("equals" | "compare" | "exists"))
        && predicate["path"].as_str().is_some_and(|path| path.starts_with('/') && path.len() <= 512)
}

/// Select a read that actually splits *currently viable* causal predictions.
/// Unlike candidate-score heuristics, no information gain is awarded if every
/// hypothesis predicts the same result. The cost model is fixed per call:
/// self-reported probabilities never become observed confidence.
pub fn choose(
    adaptation: &Value,
    refuted: &[String],
    attempted: &[String],
    failed_tool: &str,
    failed_inputs: &Value,
) -> Option<Value> {
    let active: HashSet<String> = adaptation["hypotheses"].as_array()?
        .iter().filter_map(|item| item["id"].as_str())
        .filter(|id| !refuted.iter().any(|r| r.as_str() == *id))
        .map(str::to_string).collect();
    if active.len() < 2 { return None; }
    let failed = signature(failed_tool, failed_inputs);
    adaptation["diagnostics"].as_array()?.iter()
        .filter_map(|candidate| {
            if !independent_read(candidate) { return None; }
            let tool = candidate["tool"].as_str()?;
            let key = signature(tool, &candidate["inputs"]);
            if key == failed || attempted.iter().any(|v| v == &key) { return None; }
            let mut seen = HashSet::new();
            let mut yes = 0usize;
            let mut no = 0usize;
            for prediction in candidate["predictions"].as_array()? {
                let id = prediction["hypothesisId"].as_str()?;
                if !active.contains(id) { continue; }
                if !seen.insert(id.to_string()) { return None; }
                match prediction["expected"].as_bool() {
                    Some(true) => yes += 1,
                    Some(false) => no += 1,
                    None => return None,
                }
            }
            if yes == 0 || no == 0 { return None; }
            let n = active.len() as f64;
            // Weighted Gini split, discounted for unclassified hypotheses.
            let gain = 2.0 * (yes * no) as f64 / (n * n);
            let cost = candidate["estimatedCost"].as_f64()
                .filter(|value| value.is_finite())
                .unwrap_or(1.0).clamp(0.0, 4.0);
            let score = gain / (1.0 + cost * 0.20);
            if score < 0.10 { return None; }
            let mut chosen = candidate.clone();
            chosen["informationGain"] = json!(gain);
            chosen["selectionScore"] = json!(score);
            Some((score, chosen))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, item)| item)
}

/// Interpret an actual read result against *previously authored* predictions.
/// An unknown predicate or failed tool never refutes a hypothesis. Contradicted
/// predictions are evidence AGAINST a proposal, not proof of a physical cause.
pub fn observe(candidate: &Value, result: &Value, ok: bool, operation_id: &str) -> Value {
    let has_observable_path = candidate["predicate"]["path"].as_str()
        .is_some_and(|path| result.pointer(path).is_some());
    let verdict = if ok && has_observable_path {
        predicate_verdict(&candidate["predicate"], &json!({"result": result}))
            .unwrap_or(PredicateVerdict::Unknown)
    } else {
        PredicateVerdict::Unknown
    };
    let observed = match verdict {
        PredicateVerdict::True => Some(true),
        PredicateVerdict::False => Some(false),
        PredicateVerdict::Unknown => None,
    };
    let mut supports = Vec::new();
    let mut contradicts = Vec::new();
    for prediction in candidate["predictions"].as_array().into_iter().flatten() {
        if let (Some(observed), Some(expected), Some(id)) = (
            observed, prediction["expected"].as_bool(), prediction["hypothesisId"].as_str()
        ) {
            if expected == observed { supports.push(id.to_string()); }
            else { contradicts.push(id.to_string()); }
        }
    }
    let summary = result.to_string().chars().take(1400).collect::<String>();
    json!({
        "diagnosticId": candidate["id"], "tool": candidate["tool"],
        "predicate": candidate["predicate"],
        "predictions": candidate["predictions"],
        "signature": signature(candidate["tool"].as_str().unwrap_or(""), &candidate["inputs"]),
        "operationId": operation_id,
        "resultDigest": crate::engine::memory::stable_id(&[&result.to_string()]),
        "resultPreview": summary, "toolOk": ok,
        "verdict": verdict.as_str(),
        "supports": supports, "contradicts": contradicts,
        "basis": "tool_receipt_vs_model_prediction",
        "informationGainEstimate": candidate["informationGain"],
    })
}

pub fn contradicted(evidence: &[Value]) -> Vec<String> {
    let mut result = Vec::new();
    for entry in evidence {
        for item in entry["contradicts"].as_array().into_iter().flatten() {
            if let Some(id) = item.as_str() {
                if !result.iter().any(|old| old == id) {
                    result.push(id.to_string());
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Value {
        json!({
            "hypotheses":[{"id":"file_missing"},{"id":"permission"},{"id":"stale_cache"}],
            "diagnostics":[
                {"id":"not_informative","tool":"workspace_list","inputs":{"path":"."},
                 "predicate":{"type":"exists","from":"result","path":"/entries"},
                 "predictions":[
                   {"hypothesisId":"file_missing","expected":true},
                   {"hypothesisId":"permission","expected":true},
                   {"hypothesisId":"stale_cache","expected":true}]},
                {"id":"path_probe","tool":"workspace_read","inputs":{"path":"src/main.rs"},
                 "predicate":{"type":"exists","from":"result","path":"/content"},
                 "estimatedCost":0.2,
                 "predictions":[
                   {"hypothesisId":"file_missing","expected":false},
                   {"hypothesisId":"permission","expected":false},
                   {"hypothesisId":"stale_cache","expected":true}]},
                {"id":"unsafe","tool":"shell_exec","inputs":{"command":"rm -rf tmp"},
                 "predicate":{"type":"exists","from":"result","path":"/content"},
                 "predictions":[
                   {"hypothesisId":"file_missing","expected":true},
                   {"hypothesisId":"permission","expected":false}]}
            ]
        })
    }

    #[test]
    fn only_a_discriminating_independent_read_is_eligible() {
        let selected = choose(&candidates(), &[], &[], "workspace_write", &json!({})).unwrap();
        assert_eq!(selected["id"], "path_probe");
        assert!(selected["informationGain"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn repeated_experiments_and_mutating_tools_are_not_new_information() {
        let all = candidates();
        let used = vec![signature("workspace_read", &json!({"path":"src/main.rs"}))];
        assert!(choose(&all, &[], &used, "workspace_write", &json!({})).is_none());
        assert!(choose(&all, &[], &[], "workspace_read",
            &json!({"path":"src/main.rs"})).is_none());
        assert!(choose(&all, &["file_missing".into(), "permission".into()],
            &[], "workspace_write", &json!({})).is_none());
    }

    #[test]
    fn contradictory_real_observation_updates_hypothesis_set() {
        let item = choose(&candidates(), &[], &[], "workspace_write", &json!({})).unwrap();
        let evidence = observe(&item, &json!({"content":"real bytes"}), true, "op-101");
        assert_eq!(evidence["verdict"], "true");
        assert_eq!(evidence["supports"], json!(["stale_cache"]));
        assert_eq!(contradicted(&[evidence]), vec!["file_missing", "permission"]);
    }

    #[test]
    fn failed_or_unknown_read_cannot_prove_a_cause() {
        let item = choose(&candidates(), &[], &[], "workspace_write", &json!({})).unwrap();
        for (result, ok) in [(json!({"error":"denied"}),false), (json!({}),true)] {
            let evidence = observe(&item, &result, ok, "op-5");
            assert!(contradicted(&[evidence]).is_empty());
        }
    }

    #[test]
    fn prediction_fabrication_does_not_bypass_tool_allowlist() {
        let mut data = candidates();
        data["diagnostics"][1]["tool"] = json!("workspace_write");
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
        data["diagnostics"][1]["tool"] = json!("workspace_read");
        data["diagnostics"][1]["inputs"] = json!({"path":{"from":"unverified"}});
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
        data["diagnostics"][1]["inputs"] = json!({"path":"../.ssh/id_rsa"});
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
        data["diagnostics"][1]["inputs"] = json!({"path":"/etc/passwd"});
        assert!(choose(&data,&[],&[],"a",&json!({})).is_none());
    }
}
