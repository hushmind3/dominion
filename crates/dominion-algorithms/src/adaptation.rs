//! Causal repair candidate parsing, validation, comparison and evidence updates.
//! Only the host can authorize execution, persist receipts and verify completion.
use serde_json::{json,Value};

pub fn repeated_failure_message(tool: &str) -> String {
    format!(
        "변경 없는 `{tool}` 호출이 이미 실패했습니다. 같은 호출을 다시 실행하지 않았습니다. \
         다른 도구, 다른 인자, 또는 원인을 확인하는 접근을 선택하세요."
    )
}

/// Builds the model prompt for hypothesis/intervention generation.
pub fn adaptation_prompt(
    tool: &str,
    arguments: &Value,
    error: &str,
    environment_version: u64,
    before_observation: &Value,
    environment_fingerprint: &str,
) -> (String, String) {
    let system = "You are the DOMINION adaptation planner. Given one failed action, propose DISTINCT causal hypotheses and DIFFERENT interventions. Output JSON only: {\"hypotheses\":[{\"id\":\"h1\",\"claim\":\"...\",\"evidenceIds\":[\"before\"],\"preconditions\":[],\"predictions\":[{\"observationTarget\":\"before\",\"observationPhase\":\"before_intervention\",\"predicate\":{\"type\":\"equals\",\"from\":\"before\",\"path\":\"/pathState/path/exists\",\"value\":false},\"expectedValue\":false}]}],\"interventions\":[{\"id\":\"i1\",\"hypothesisIds\":[\"h1\"],\"preconditions\":[],\"tool\":\"workspace_write\",\"inputs\":{...},\"expectedChecks\":[\"file_exists\"],\"expectedProgress\":0.7,\"uncertainty\":0.2,\"estimatedCost\":0.2}]}. Predictions about why the failure happened must use observationPhase=before_intervention and target the before observation; predictions about the result of an intervention use after_intervention. Missing fields are Unknown, never false. Each intervention must change the tool, arguments, or approach. OPTIONAL diagnostics allow evidence-guided repair: diagnostics:[{id:'d1',tool:'workspace_read',inputs:{path:'...'},predicate:{type:'equals',from:'result',path:'/content',value:'...'},predictions:[{hypothesisId:'h1',expected:true},{hypothesisId:'h2',expected:false}],estimatedCost:1}]. Diagnostic predicates describe the RESULT of one local read. Offer only probes whose true/false result distinguishes rival hypotheses. Never include writes, shells, remote calls, or secret-file reads in diagnostics. No probe is proof of task completion.";
    let prompt = format!(
        "Failed action: tool={tool}\nArguments: {}\nObserved error: {}\nEnvironment version: {environment_version}\nEnvironment fingerprint: {environment_fingerprint}\nBefore observation (authoritative): {}\nAvailable tools include workspace_read, workspace_write, workspace_list, shell_exec, terminal_exec.\nReturn JSON only.",
        arguments, error, before_observation
    );
    (system.to_string(), prompt)
}

/// Extracts the first JSON object that actually contains hypotheses and
/// interventions. Local thinking models often wrap JSON in prose or markdown,
/// so parsing the outermost braces is not reliable.
pub fn parse_adaptation(raw: &str) -> Option<Value> {
    let bytes = raw.as_bytes();
    for (start, _) in raw.match_indices('{') {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        for index in start..bytes.len() {
            let byte = bytes[index];
            if in_string {
                if escape {
                    escape = false;
                } else if byte == b'\\' {
                    escape = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(value) = serde_json::from_str::<Value>(&raw[start..=index]) {
                            if value["hypotheses"].is_array()
                                && value["interventions"].is_array()
                            {
                                return Some(value);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Structural validation used before execution. Field errors are returned to
/// the caller so the model can repair its output instead of being trusted.
pub fn validate_adaptation(adaptation: &Value) -> Result<(), String> {
    let hypotheses = adaptation["hypotheses"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("hypotheses must be a non-empty array")?;
    for hypothesis in hypotheses {
        if hypothesis["id"].as_str().is_none_or(str::is_empty) {
            return Err("each hypothesis needs an id".into());
        }
        if hypothesis["claim"].as_str().is_none_or(str::is_empty) {
            return Err(format!(
                "hypothesis {} needs a claim",
                hypothesis["id"]
            ));
        }
        if !hypothesis["predictions"].is_array() {
            return Err(format!(
                "hypothesis {} predictions must be an array",
                hypothesis["id"]
            ));
        }
        if !hypothesis["preconditions"].is_null()
            && !hypothesis["preconditions"].is_array()
        {
            return Err(format!(
                "hypothesis {} preconditions must be an array",
                hypothesis["id"]
            ));
        }
        for prediction in hypothesis["predictions"].as_array().into_iter().flatten() {
            if let Some(phase) = prediction["observationPhase"].as_str() {
                if !matches!(phase, "before_intervention" | "after_intervention") {
                    return Err(format!(
                        "hypothesis {} has invalid observationPhase {phase}",
                        hypothesis["id"]
                    ));
                }
            }
            if !prediction["predicate"].is_object() {
                return Err(format!(
                    "hypothesis {} prediction needs a predicate object",
                    hypothesis["id"]
                ));
            }
        }
    }
    let interventions = adaptation["interventions"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("interventions must be a non-empty array")?;
    for intervention in interventions {
        if intervention["id"].as_str().is_none_or(str::is_empty) {
            return Err("each intervention needs an id".into());
        }
        if intervention["tool"].as_str().is_none_or(str::is_empty) {
            return Err(format!(
                "intervention {} needs a tool",
                intervention["id"]
            ));
        }
        if !intervention["inputs"].is_object() {
            return Err(format!(
                "intervention {} inputs must be an object",
                intervention["id"]
            ));
        }
        if !intervention["hypothesisIds"].is_array() {
            return Err(format!(
                "intervention {} hypothesisIds must be an array",
                intervention["id"]
            ));
        }
        if !intervention["preconditions"].is_null()
            && !intervention["preconditions"].is_array()
        {
            return Err(format!(
                "intervention {} preconditions must be an array",
                intervention["id"]
            ));
        }
    }
    Ok(())
}

fn intervention_key(tool: &str, inputs: &Value) -> String {
    format!("{tool}:{}", inputs)
}

/// Choose a non-dominated remediation proposal; the actual acceptance gate
/// still decides whether the action made task progress. Model-authored
/// progress/historical scores never count as observed evidence.
pub fn choose_intervention_with_context(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
    resolved: &Value,
    refuted_hypotheses: &[String],
) -> Option<Value> {
    use dominion_core::predicate::{predicate_verdict, PredicateVerdict};
    struct Candidate {
        value: Value, surviving: usize, unknown: usize, cost: f64,
    }
    let failed_key = intervention_key(failed_tool, failed_arguments);
    let mut candidates = Vec::new();
    for proposal in adaptation["interventions"].as_array().into_iter().flatten() {
        let Some(tool) = proposal["tool"].as_str() else { continue };
        let inputs = &proposal["inputs"];
        if !inputs.is_object()
            || (!environment_changed && intervention_key(tool, inputs) == failed_key) {
            continue;
        }
        let ids = proposal["hypothesisIds"].as_array();
        let surviving = ids.into_iter().flatten()
            .filter_map(Value::as_str)
            .filter(|id| !refuted_hypotheses.iter().any(|x| x.as_str() == *id))
            .count();
        if ids.is_some_and(|ids| !ids.is_empty()) && surviving == 0 {
            continue;
        }
        let mut unknown = 0usize;
        let mut preconditions = Vec::new();
        let mut contradicted = false;
        for precondition in proposal["preconditions"].as_array().into_iter().flatten() {
            let Ok(verdict) = predicate_verdict(precondition, resolved) else {
                contradicted = true; break;
            };
            preconditions.push(json!({
                "predicate":precondition,"verdict":verdict.as_str(),
            }));
            match verdict {
                PredicateVerdict::True => {}
                PredicateVerdict::False => {contradicted = true; break;}
                PredicateVerdict::Unknown => unknown += 1,
            }
        }
        if contradicted { continue; }
        let cost = proposal["estimatedCost"].as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(1.0);
        let mut value=proposal.clone();
        value["evaluation"]=json!({
            "selectionPolicy":"constrained_pareto_2026",
            "survivingCausalExplanations":surviving,
            "unknownPreconditions":unknown,
            "estimatedCost":cost,
            "preconditions":preconditions,
            "finalAcceptance":"frozen_postcondition_no_regression",
        });
        candidates.push(Candidate{value,surviving,unknown,cost});
    }
    let dominates = |a: &Candidate, b: &Candidate| {
        // b is strictly no worse than a on every independent constraint.
        b.surviving >= a.surviving && b.unknown <= a.unknown && b.cost <= a.cost
            && (b.surviving > a.surviving
                || b.unknown < a.unknown || b.cost < a.cost)
    };
    let frontier = (0..candidates.len()).filter(|&i| {
        !candidates.iter().enumerate().any(|(j,other)|
            i != j && dominates(&candidates[i],other)
        )
    }).collect::<Vec<_>>();
    // Stable resolution of incomparable survivors, no synthetic reward scalar.
    let chosen = frontier.iter().copied().max_by(|&i,&j| {
        let a=&candidates[i]; let b=&candidates[j];
        a.surviving.cmp(&b.surviving)
            .then_with(|| b.unknown.cmp(&a.unknown))
            .then_with(|| b.cost.total_cmp(&a.cost))
            .then_with(|| j.cmp(&i))
    })?;
    let mut selected=candidates[chosen].value.clone();
    selected["evaluation"]["candidateCount"]=json!(candidates.len());
    selected["evaluation"]["paretoFrontierCount"]=json!(frontier.len());
    Some(selected)
}

/// Compares hypotheses against the correct observation phase. A prediction
/// about the state before an intervention is never refuted by the state after
/// it. Missing fields remain Unknown.
pub fn evaluate_hypotheses_phased(
    adaptation: &Value,
    before: &Value,
    after: &Value,
) -> Vec<Value> {
    use dominion_core::predicate::predicate_verdict;

    adaptation["hypotheses"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|hypothesis| {
            let predictions = hypothesis["predictions"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut supported = 0;
            let mut unknown = 0;
            let mut refuted = false;
            let mut verdicts = Vec::new();
            for prediction in &predictions {
                let phase = prediction["observationPhase"]
                    .as_str()
                    .unwrap_or("after_intervention");
                let target = prediction["observationTarget"]
                    .as_str()
                    .unwrap_or("result");
                let observation = if phase == "before_intervention" {
                    before
                } else {
                    after
                };
                let mut scope = json!({"before": before, "after": after, "result": observation});
                scope[target] = observation.clone();
                let verdict = match predicate_verdict(&prediction["predicate"], &scope) {
                    Ok(verdict) => verdict,
                    Err(error) => {
                        return json!({
                            "id": hypothesis["id"],
                            "status": "unknown",
                            "error": error,
                            "predictions": [],
                        });
                    }
                };
                match verdict {
                    dominion_core::predicate::PredicateVerdict::True => supported += 1,
                    dominion_core::predicate::PredicateVerdict::False => refuted = true,
                    dominion_core::predicate::PredicateVerdict::Unknown => unknown += 1,
                }
                verdicts.push(json!({
                    "observationPhase": phase,
                    "observationTarget": target,
                    "verdict": verdict.as_str(),
                }));
            }
            let status = if refuted {
                "refuted"
            } else if predictions.is_empty() || unknown > 0 {
                "unknown"
            } else if supported > 0 {
                "supported"
            } else {
                "unknown"
            };
            json!({
                "id": hypothesis["id"],
                "status": status,
                "supported": supported,
                "unknown": unknown,
                "predictions": verdicts,
            })
        })
        .collect()
}

/// Legacy wrapper: uses the same value for before/after so old callers keep
/// working, but missing fields are still Unknown instead of refuted.
#[allow(dead_code)]
pub fn evaluate_hypotheses(adaptation: &Value, result: &Value) -> Vec<Value> {
    evaluate_hypotheses_phased(adaptation, result, result)
}


#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn independent_kernel_rejects_identical_retry_and_refuted_hypotheses(){
  let proposals=json!({"interventions":[
    {"id":"same","tool":"workspace_write","inputs":{"path":"a"},"hypothesisIds":["h1"]},
    {"id":"refuted","tool":"workspace_read","inputs":{"path":"b"},"hypothesisIds":["h2"]},
    {"id":"other","tool":"workspace_read","inputs":{"path":"a"},"hypothesisIds":["h1"]}
  ]});
  let selected=choose_intervention_with_context(&proposals,"workspace_write",&json!({"path":"a"}),false,&Value::Null,&["h2".into()]).unwrap();
  assert_eq!(selected["id"],"other");
 }
}
