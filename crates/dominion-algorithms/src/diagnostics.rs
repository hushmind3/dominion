//! Task-conditioned diagnostic decisions. This is a pure decision layer, not
//! another execution loop: tools still pass through the native journal/gateway.
//! Model predictions are hypotheses; only a recorded tool result is evidence.
use serde_json::{json, Value};
use dominion_core::predicate::{predicate_verdict, PredicateVerdict};

/// Stable identity for one observation request. A repeated tool name with
/// different inputs is a different experiment.
pub fn signature(tool: &str, inputs: &Value) -> String {
    format!("{tool}:{}", inputs)
}

fn independent_read(candidate: &Value, observation_only: fn(&str,&Value)->bool) -> bool {
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
    if !observation_only(tool, &candidate["inputs"]) {
        return false;
    }
    let predicate = &candidate["predicate"];
    predicate["from"] == "result"
        && matches!(predicate["type"].as_str(), Some("equals" | "compare" | "exists"))
        && predicate["path"].as_str().is_some_and(|path| path.starts_with('/') && path.len() <= 512)
}

/// A diagnostic prediction must classify EVERY surviving hypothesis.
/// Partial predictions never establish a safe decision partition.
struct Experiment<'a> {
    source: &'a Value,
    signature: String,
    yes: u32,
    no: u32,
    cost: f64,
}

/// Compute the worst unresolved causal set after an ADAPTIVE decision tree.
/// A later probe is chosen separately for each real true/false observation.
/// This is bounded planning, not a one-step Gini heuristic.
fn worst_remaining(mask: u32, experiments: &[Experiment<'_>], used: u16, depth: usize) -> u32 {
    let unsolved = mask.count_ones();
    if depth == 0 || unsolved <= 1 { return unsolved; }
    let mut best = unsolved;
    for (index, candidate) in experiments.iter().enumerate() {
        if (used & (1u16 << index)) != 0 { continue; }
        let yes = mask & candidate.yes;
        let no = mask & candidate.no;
        if yes == 0 || no == 0 { continue; }
        let next_used = used | (1u16 << index);
        let remaining = worst_remaining(yes, experiments, next_used, depth - 1)
            .max(worst_remaining(no, experiments, next_used, depth - 1));
        best = best.min(remaining);
        if best == 1 { break; }
    }
    best
}

/// Receding-horizon diagnostic policy: worst-case surviving causes after
/// further evidence determines this next action. The harness owns the policy;
/// the model provides candidate hypotheses and falsifiable predictions.
/// Complexity is bounded to 16 hypotheses, 12 candidates and 3 steps.
pub fn choose_with_budget(
    adaptation: &Value,
    refuted: &[String],
    attempted: &[String],
    failed_tool: &str,
    failed_inputs: &Value,
    remaining_steps: usize,
    observation_only: fn(&str,&Value)->bool,
) -> Option<Value> {
    let mut active = adaptation["hypotheses"].as_array()?
        .iter().filter_map(|item| item["id"].as_str())
        .filter(|id| !refuted.iter().any(|item| item.as_str() == *id))
        .map(str::to_string).collect::<Vec<_>>();
    active.sort();
    active.dedup();
    if !(2..=16).contains(&active.len()) || remaining_steps == 0 { return None; }
    let failed = signature(failed_tool, failed_inputs);
    let mut experiments = Vec::new();
    for candidate in adaptation["diagnostics"].as_array()?.iter().take(32) {
        if experiments.len() == 12 { break; }
        if !independent_read(candidate,observation_only) { continue; }
        let Some(tool) = candidate["tool"].as_str() else { continue; };
        let key = signature(tool, &candidate["inputs"]);
        if key == failed || attempted.iter().any(|old| old == &key)
            || experiments.iter().any(|item: &Experiment<'_>| item.signature == key) {
            continue;
        }
        let Some(predictions) = candidate["predictions"].as_array() else { continue; };
        let mut observed = 0u32;
        let mut yes = 0u32;
        let mut valid = true;
        for prediction in predictions {
            let Some(id) = prediction["hypothesisId"].as_str() else { valid = false; break; };
            let Some(index) = active.iter().position(|item| item == id) else {
                continue; // Already refuted, not part of this decision.
            };
            let bit = 1u32 << index;
            if (observed & bit) != 0 { valid = false; break; }
            let Some(expected) = prediction["expected"].as_bool() else {
                valid = false; break;
            };
            observed |= bit;
            if expected { yes |= bit; }
        }
        let all = (1u32 << active.len()) - 1;
        if !valid || observed != all || yes == 0 || yes == all { continue; }
        let cost = candidate["estimatedCost"].as_f64()
            .filter(|value| value.is_finite())
            .unwrap_or(1.0).clamp(0.0, 4.0);
        experiments.push(Experiment {
            source: candidate, signature: key, yes, no: all & !yes, cost,
        });
    }
    if experiments.is_empty() { return None; }
    let horizon = remaining_steps.min(3).min(experiments.len());
    let selected = (0..experiments.len()).min_by(|left, right| {
        let rank = |index: usize| {
            let candidate = &experiments[index];
            let used = 1u16 << index;
            let worst = worst_remaining(candidate.yes, &experiments, used, horizon - 1)
                .max(worst_remaining(candidate.no, &experiments, used, horizon - 1));
            let immediate = candidate.yes.count_ones().max(candidate.no.count_ones());
            (worst, immediate, candidate.cost)
        };
        let a = rank(*left);
        let b = rank(*right);
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.total_cmp(&b.2))
            .then_with(|| left.cmp(right))
    })?;
    let experiment = &experiments[selected];
    let worst = worst_remaining(
        experiment.yes, &experiments, 1u16 << selected, horizon - 1,
    ).max(worst_remaining(
        experiment.no, &experiments, 1u16 << selected, horizon - 1,
    ));
    let mut result = (*experiment.source).clone();
    // Strip stale/model-authored scalar scores. The bounded causal horizon,
    // not a Gini reward or a one-step success guess, owns the decision.
    if let Some(object) = result.as_object_mut() {
        object.remove("informationGain");
        object.remove("selectionScore");
    }
    result["selectionPolicy"] = json!("contingent_minimax_v1");
    result["forecastWorstRemaining"] = json!(worst);
    result["forecastHorizon"] = json!(horizon);
    Some(result)
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
        "resultDigest": crate::memory::stable_id(&[&result.to_string()]),
        "resultPreview": summary, "toolOk": ok,
        "verdict": verdict.as_str(),
        "supports": supports, "contradicts": contradicts,
        "basis": "tool_receipt_vs_model_prediction",
        "forecastWorstRemaining": candidate["forecastWorstRemaining"],
        "forecastHorizon": candidate["forecastHorizon"],
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
mod tests{
 use super::*;
 #[test]
 fn host_observation_gate_cannot_be_bypassed(){
    let adaptation=json!({"hypotheses":[{"id":"a"},{"id":"b"}],"diagnostics":[{
      "id":"probe","tool":"workspace_read","inputs":{"path":"test.txt"},
      "predicate":{"type":"exists","from":"result","path":"/content"},
      "predictions":[{"hypothesisId":"a","expected":true},{"hypothesisId":"b","expected":false}]
    }]});
    assert!(choose_with_budget(&adaptation,&[],&[],"workspace_write",&Value::Null,2,|_,_|false).is_none());
    assert!(choose_with_budget(&adaptation,&[],&[],"workspace_write",&Value::Null,2,|_,_|true).is_some());
 }
}
