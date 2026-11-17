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
        if !independent_read(candidate) { continue; }
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
    let n = active.len() as f64;
    result["informationGain"] = json!(
        2.0 * f64::from(experiment.yes.count_ones())
            * f64::from(experiment.no.count_ones()) / (n * n)
    );
    result["selectionPolicy"] = json!("contingent_minimax_v1");
    result["forecastWorstRemaining"] = json!(worst);
    result["forecastHorizon"] = json!(horizon);
    result["selectionScore"] = json!(1.0 - f64::from(worst) / n);
    Some(result)
}

/// Test-only compatibility: production always uses remaining live budget.
#[cfg(test)]
pub fn choose(
    adaptation: &Value,
    refuted: &[String],
    attempted: &[String],
    failed_tool: &str,
    failed_inputs: &Value,
) -> Option<Value> {
    choose_with_budget(adaptation, refuted, attempted, failed_tool, failed_inputs, 3)
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
    #[test]
    fn contingent_two_step_policy_beats_greedy_first_split() {
        // Nine distinct causes; greedy one-step picks probe_6 (5/4),
        // leaving at least four possible causes even after one more probe.
        // A contingent first choice probe_1 (3/6) can reduce both branches
        // to <=3 with a second independent observation.
        let masks = [508u32, 292, 501, 498, 34, 380, 472];
        let hypotheses = (0..9).map(|i| json!({"id": format!("h{i}")}))
            .collect::<Vec<_>>();
        let diagnostics = masks.iter().enumerate().map(|(i,mask)| json!({
            "id":format!("probe_{i}"),"tool":"workspace_read",
            "inputs":{"path":format!("fixture/probe_{i}.txt")},
            "predicate":{"type":"equals","from":"result","path":"/flag","value":true},
            "predictions": (0..9).map(|id| json!({
                "hypothesisId":format!("h{id}"),"expected": (mask & (1u32 << id)) != 0
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>();
        let scenario = json!({"hypotheses":hypotheses,"diagnostics":diagnostics});
        let first = choose_with_budget(&scenario,&[],&[],"workspace_write",&json!({}),2).unwrap();
        assert_eq!(first["id"],"probe_1");
        assert_eq!(first["forecastWorstRemaining"],3);
        assert_eq!(first["selectionPolicy"],"contingent_minimax_v1");
        let greedy = choose_with_budget(&scenario,&[],&[],"workspace_write",&json!({}),1).unwrap();
        assert_eq!(greedy["id"],"probe_6");
        assert_eq!(greedy["forecastWorstRemaining"],5);
        for flag in [false,true] {
            let receipt = observe(&first,&json!({"flag":flag}),true,"op-proof");
            let refuted = contradicted(&[receipt]);
            let used = vec![signature(first["tool"].as_str().unwrap(),&first["inputs"])];
            let follow = choose_with_budget(
                &scenario,&refuted,&used,"workspace_write",&json!({}),1
            ).unwrap();
            assert_ne!(follow["id"],first["id"]);
            assert!(follow["forecastWorstRemaining"].as_u64().unwrap() <= 3);
        }
    }

    #[test]
    fn incomplete_or_conflicting_diagnostic_predictions_are_rejected() {
        let mut data = candidates();
        data["diagnostics"][1]["predictions"].as_array_mut().unwrap().pop();
        assert!(choose(&data,&[],&[],"workspace_write",&json!({})).is_none());
        let mut data = candidates();
        data["diagnostics"][1]["predictions"].as_array_mut().unwrap()
            .push(json!({"hypothesisId":"stale_cache","expected":false}));
        assert!(choose(&data,&[],&[],"workspace_write",&json!({})).is_none());
    }

    #[test]
    fn diagnostic_horizon_obeys_the_remaining_execution_budget() {
        assert!(choose_with_budget(&candidates(),&[],&[],"x",&json!({}),0).is_none());
        let one = choose_with_budget(&candidates(),&[],&[],"x",&json!({}),1).unwrap();
        assert_eq!(one["forecastHorizon"],1);
        let many = choose_with_budget(&candidates(),&[],&[],"x",&json!({}),100).unwrap();
        assert_eq!(many["forecastHorizon"],1);
    }

    #[test]
    fn observed_cause_changes_the_selected_remediation() {
        let data = json!({
            "hypotheses":[{"id":"missing"},{"id":"permission"},{"id":"stale"}],
            "diagnostics":[{
                "id":"read","tool":"workspace_read","inputs":{"path":"source.rs"},
                "predicate":{"type":"equals","from":"result","path":"/reachable","value":true},
                "predictions":[
                    {"hypothesisId":"missing","expected":false},
                    {"hypothesisId":"permission","expected":false},
                    {"hypothesisId":"stale","expected":true}]
            }],
            "interventions":[
                {"id":"create","tool":"workspace_write","inputs":{"path":"source.rs","content":"a"},
                    "hypothesisIds":["missing"],"expectedProgress":2.0},
                {"id":"refresh","tool":"workspace_read","inputs":{"path":"actual.rs"},
                    "hypothesisIds":["stale"],"expectedProgress":1.0}
            ]
        });
        let before = crate::engine::adaptation_loop::choose_intervention_with_context(
            &data,"shell_exec",&json!({"command":"failed"}),false,&json!({}),&[]
        ).unwrap();
        assert_eq!(before["id"],"create");
        let diagnostic = choose(&data,&[],&[],"shell_exec",&json!({})).unwrap();
        let receipt = observe(&diagnostic,&json!({"reachable":true}),true,"op-11");
        let after = crate::engine::adaptation_loop::choose_intervention_with_context(
            &data,"shell_exec",&json!({"command":"failed"}),false,&json!({}),
            &contradicted(&[receipt])
        ).unwrap();
        assert_eq!(after["id"],"refresh");
    }

}
