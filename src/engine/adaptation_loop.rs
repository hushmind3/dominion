//! Section 11-1 of design 2.4: observation -> adaptation inside the running
//! loop. A failing tool call is recorded as an observation with a hypothesis
//! and a changed-action intervention. The runtime also blocks an unchanged
//! repeat of that failed call so the model has to pick a different action.
use crate::state::AppState;
use serde_json::{json, Value};

/// Records a tool failure and the adaptation it implies. Bounded so a long run
/// cannot grow the execution record without limit.
pub fn record_tool_failure(
    state: &AppState,
    execution_id: &str,
    round: usize,
    call_id: &str,
    tool: &str,
    arguments: &Value,
    error: &str,
) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let digest = crate::engine::memory::stable_id(&[tool, &arguments.to_string()]);
    let adaptation = json!({
        "id": format!(
            "adaptation:{}",
            crate::engine::memory::stable_id(&[execution_id, call_id, &digest])
        ),
        "runId": execution_id,
        "stepId": round,
        "operationId": call_id,
        "observation": {
            "kind": "tool_failure",
            "tool": tool,
            "argsDigest": digest,
            "expected": "success",
            "actual": error.chars().take(600).collect::<String>(),
        },
        "hypothesis": {
            "claim": "the same call will repeat the same failure",
            "evidenceIds": [call_id],
        },
        "intervention": {
            "change": "different tool, arguments, or approach",
            "instruction": "do not repeat the identical failed call; choose a changed action or explain the blocker",
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let mut adaptations = execution["adaptations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    adaptations.push(adaptation.clone());
    if adaptations.len() > 20 {
        adaptations = adaptations.split_off(adaptations.len() - 20);
    }
    execution["adaptations"] = json!(adaptations);
    execution["lastAdaptation"] = adaptation.clone();
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(adaptation)
}

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
    let system = "You are the DOMINION adaptation planner. Given one failed action, propose DISTINCT causal hypotheses and DIFFERENT interventions. Output JSON only: {\"hypotheses\":[{\"id\":\"h1\",\"claim\":\"...\",\"evidenceIds\":[\"before\"],\"preconditions\":[],\"predictions\":[{\"observationTarget\":\"before\",\"observationPhase\":\"before_intervention\",\"predicate\":{\"type\":\"equals\",\"from\":\"before\",\"path\":\"/pathState/path/exists\",\"value\":false},\"expectedValue\":false}]}],\"interventions\":[{\"id\":\"i1\",\"hypothesisIds\":[\"h1\"],\"preconditions\":[],\"tool\":\"workspace_write\",\"inputs\":{...},\"expectedChecks\":[{\"predicate\":{\"type\":\"equals\",\"from\":\"after\",\"path\":\"/pathState/path/exists\",\"value\":true}}],\"expectedProgress\":0.7,\"uncertainty\":0.2,\"estimatedCost\":0.2}]}. Predictions about why the failure happened must use observationPhase=before_intervention and target the before observation; predictions about the result of an intervention use after_intervention. Missing fields are Unknown, never false. Use typed expectedChecks on host-observed after/pathState fields for writes; plain strings or echoing result.ok do not prove progress. Each intervention must change the tool, arguments, or approach.";
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

/// A candidate is diagnostic only if competing live causes predict
/// *different values for the same observable*. Naming more hypothesis IDs is
/// not information gain. Only exact-value predictions are contrasted:
/// ranges and existence claims may overlap and must not be treated as mutually
/// exclusive without real observations.
fn predicted_values(
    hypothesis: &Value,
    observed_slots: &std::collections::HashSet<(String, String)>,
) -> std::collections::BTreeMap<String, String> {
    let mut values = std::collections::BTreeMap::new();
    for prediction in hypothesis["predictions"].as_array().into_iter().flatten() {
        if prediction["observationPhase"].as_str().unwrap_or("after_intervention")
            != "after_intervention"
        {
            continue;
        }
        let predicate = &prediction["predicate"];
        let from = predicate["from"].as_str().unwrap_or("result");
        let path = predicate["path"].as_str().unwrap_or("");
        if !observed_slots.contains(&(from.to_owned(), path.to_owned())) {
            continue;
        }
        let kind = predicate["type"].as_str().unwrap_or("");
        let expected = match kind {
            "equals" => predicate.get("value"),
            "exists" => prediction.get("expectedValue").filter(Value::is_boolean),
            _ => None,
        };
        let Some(expected) = expected else { continue; };
        let key = format!("{}:{}:{}:{}",
            prediction["observationTarget"].as_str().unwrap_or("result"),
            predicate["from"].as_str().unwrap_or("result"),
            predicate["path"].as_str().unwrap_or(""),
            kind);
        values.insert(key, expected.to_string());
    }
    values
}

fn contrastive_gain(
    hypotheses: &[Value],
    hypothesis_ids: &[Value],
    refuted: &[String],
    expected_checks: &[Value],
) -> (f64, usize) {
    let observed_slots = expected_checks.iter().filter_map(|check| {
        let predicate = check.get("predicate").filter(Value::is_object)
            .unwrap_or(check);
        Some((
            predicate["from"].as_str()?.to_owned(),
            predicate["path"].as_str().unwrap_or("").to_owned(),
        ))
    }).collect::<std::collections::HashSet<_>>();
    let active = hypotheses.iter().filter(|h| {
        let Some(id) = h["id"].as_str() else { return false; };
        !refuted.iter().any(|old|old==id)
            && (hypothesis_ids.is_empty()
                || hypothesis_ids.iter().any(|v|v.as_str()==Some(id)))
    }).map(|h| predicted_values(h, &observed_slots)).collect::<Vec<_>>();
    let mut contradicted = 0usize;
    let mut all_pairs = 0usize;
    for (i, a) in active.iter().enumerate() {
        for b in active.iter().skip(i+1) {
            all_pairs += 1;
            if a.iter().any(|(slot,value)| {
                b.get(slot).is_some_and(|different|different!=value)
            }) {
                contradicted += 1;
            }
        }
    }
    (if all_pairs == 0 {0.0} else {contradicted as f64/all_pairs as f64}, all_pairs)
}

/// Picks an intervention using observable preconditions, expected progress,
/// hypothesis discrimination, historical effect, uncertainty and cost.
pub fn choose_intervention_with_context(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
    resolved: &Value,
    refuted_hypotheses: &[String],
) -> Option<Value> {
    choose_intervention_with_policy(
        adaptation,
        failed_tool,
        failed_arguments,
        environment_changed,
        resolved,
        refuted_hypotheses,
        &json!({}),
    )
}

/// Same selection, with weights supplied by the algorithm space. The harness
/// owns execution; the selected algorithm owns only this scoring choice.
#[allow(clippy::too_many_arguments)]
pub fn choose_intervention_with_policy(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
    resolved: &Value,
    refuted_hypotheses: &[String],
    algorithm: &Value,
) -> Option<Value> {
    use crate::engine::program::{predicate_verdict, PredicateVerdict};

    let weight = |name: &str, fallback: f64| {
        algorithm["parameters"][name].as_f64().unwrap_or(fallback)
    };
    let discrimination_weight = weight("discriminationWeight", 1.0);
    let progress_weight = weight("progressWeight", 1.0);
    let cost_weight = weight("costWeight", 1.0);
    let uncertainty_weight = weight("uncertaintyWeight", 0.5);
    let expression = (algorithm["implementation"].as_str() == Some("expression"))
        .then(|| algorithm["parameters"]["expression"].as_str())
        .flatten();
    let failed_key = intervention_key(failed_tool, failed_arguments);
    let hypotheses = adaptation["hypotheses"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let live_hypotheses = hypotheses
        .iter()
        .filter(|hypothesis| {
            !hypothesis["id"]
                .as_str()
                .is_some_and(|id| refuted_hypotheses.iter().any(|item| item == id))
        })
        .count();
    let cause_unclear = live_hypotheses >= 2;

    adaptation["interventions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let tool = item["tool"].as_str()?;
            let inputs = &item["inputs"];
            let key = intervention_key(tool, inputs);
            if key == failed_key && !environment_changed {
                return None;
            }
            let hypothesis_ids = item["hypothesisIds"].as_array().cloned().unwrap_or_default();
            let only_refuted = !hypothesis_ids.is_empty()
                && hypothesis_ids.iter().all(|id| {
                    id.as_str()
                        .is_some_and(|id| refuted_hypotheses.iter().any(|item| item == id))
                });
            if only_refuted {
                return None;
            }
            let mut precondition_status = "true";
            let mut preconditions = Vec::new();
            for precondition in item["preconditions"].as_array().into_iter().flatten() {
                let verdict = predicate_verdict(precondition, resolved).ok()?;
                preconditions.push(json!({
                    "predicate": precondition,
                    "verdict": verdict.as_str(),
                }));
                match verdict {
                    PredicateVerdict::False => {
                        precondition_status = "false";
                    }
                    PredicateVerdict::Unknown => {
                        if precondition_status != "false" {
                            precondition_status = "unknown";
                        }
                    }
                    PredicateVerdict::True => {}
                }
            }
            if precondition_status == "false" {
                return None;
            }
            let expected_checks = item["expectedChecks"].as_array().map(|checks|
                checks.iter().filter(|check| {
                    check["predicate"].is_object() || check["type"].is_string()
                }).count()
            ).unwrap_or(0) as f64;
            let evidence_coverage = (expected_checks / 3.0).clamp(0.0,1.0);
            let unresolved_requirements = item["unresolvedRequirements"]
                .as_array().map(Vec::len).unwrap_or(0).min(3) as f64;
            let expected_progress = item["expectedProgress"].as_f64()
                .unwrap_or(evidence_coverage * 0.5 + unresolved_requirements * 0.15)
                .clamp(0.0, 1.0);
            let (discrimination, contrasted_pairs) = contrastive_gain(
                &hypotheses, &hypothesis_ids, refuted_hypotheses,
                item["expectedChecks"].as_array().map(Vec::as_slice).unwrap_or(&[]),
            );
            // Model-asserted historical success is not a historical outcome.
            // Give it weight only when the declared evidence is resolvable in
            // the host's observation context, never from a bare float.
            let cited_history = item["historicalEvidenceIds"].as_array().is_some_and(|ids|
                !ids.is_empty() && ids.iter().all(|id|id.as_str()
                    .is_some_and(|key|resolved.get(key).is_some())));
            let historical_effect = if cited_history {
                item["historicalEffect"].as_f64().map(|v|v.clamp(-1.0,1.0))
            } else {None};
            let uncertainty = item["uncertainty"].as_f64().unwrap_or(0.5).clamp(0.0,1.0);
            let cost = item["estimatedCost"].as_f64().unwrap_or(0.5).clamp(0.0,10.0);
            let diagnostic = dominion_agent_sdk::runtime::builtin_is_read_only(tool);
            let mutation_risk = if cause_unclear && !diagnostic { 0.65 } else { 0.0 };
            // A changed environment permits a former failed call, but it
            // should not displace a genuinely changed action for a tiny cost
            // saving unless the evidence warrants another attempt.
            let retry_penalty = if key == failed_key { 0.75 } else { 0.0 };
            let mut variables = std::collections::BTreeMap::new();
            variables.insert("discrimination".into(), discrimination);
            variables.insert("progress".into(), expected_progress);
            variables.insert("cost".into(), cost);
            variables.insert("uncertainty".into(), uncertainty);
            variables.insert("historical".into(), historical_effect.unwrap_or(0.0));
            variables.insert("unresolved".into(), unresolved_requirements);
            variables.insert(
                "precondition".into(),
                if precondition_status == "true" {
                    1.0
                } else if precondition_status == "unknown" {
                    0.5
                } else {
                    0.0
                },
            );
            let base_score = if let Some(expression) = expression {
                crate::engine::algorithms::eval_expression(expression, &variables).ok()?
            } else if cause_unclear {
                discrimination * 4.0 * discrimination_weight
                    + expected_progress * 0.5 * progress_weight
                    + evidence_coverage * 0.5
                    + if diagnostic { 0.5 } else { 0.0 }
                    + historical_effect.unwrap_or(0.0)
                    - mutation_risk
                    - cost * cost_weight
                    - uncertainty * uncertainty_weight
                    - if precondition_status == "unknown" { 0.5 } else { 0.0 }
            } else {
                expected_progress * 2.0 * progress_weight
                    + discrimination * 0.5 * discrimination_weight
                    + evidence_coverage * 0.5
                    + historical_effect.unwrap_or(0.0)
                    - cost * cost_weight
                    - uncertainty * uncertainty_weight
                    - if precondition_status == "unknown" { 0.5 } else { 0.0 }
            };
            let score = base_score - retry_penalty;
            let evaluation = json!({
                "unresolvedRequirements": unresolved_requirements,
                "expectedProgress": expected_progress,
                "hypothesisDiscrimination": discrimination,
                "contrastedHypothesisPairs": contrasted_pairs,
                "executableCheckCoverage": evidence_coverage,
                "diagnosticReadOnly": diagnostic,
                "mutationRisk": mutation_risk,
                "retryPenalty": retry_penalty,
                "historicalEffect": historical_effect,
                "uncertainty": uncertainty,
                "estimatedCost": cost,
                "preconditionStatus": precondition_status,
                "preconditions": preconditions,
                "causeUnclear": cause_unclear,
                "algorithmRevision": algorithm["revision"],
                "algorithmKernel": algorithm["implementation"],
                "score": score,
            });
            let mut item = item.clone();
            item["score"] = json!(score);
            item["evaluation"] = evaluation;
            Some((score, item))
        })
        .max_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, item)| item)
}

/// Backwards-compatible wrapper for callers without an observation context.
#[allow(dead_code)]
pub fn choose_intervention(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
) -> Option<Value> {
    choose_intervention_with_context(
        adaptation,
        failed_tool,
        failed_arguments,
        environment_changed,
        &json!({}),
        &[],
    )
}

/// Compares hypotheses against the correct observation phase. A prediction
/// about the state before an intervention is never refuted by the state after
/// it. Missing fields remain Unknown.
pub fn evaluate_hypotheses_phased(
    adaptation: &Value,
    before: &Value,
    after: &Value,
) -> Vec<Value> {
    use crate::engine::program::predicate_verdict;

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
                    crate::engine::program::PredicateVerdict::True => supported += 1,
                    crate::engine::program::PredicateVerdict::False => refuted = true,
                    crate::engine::program::PredicateVerdict::Unknown => unknown += 1,
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

/// Score a proposed intervention against *observed* postconditions. A tool
/// returning successfully is a transport fact, not proof the repair worked.
/// Human-readable expectedChecks are retained but are not executable proof.
pub fn evaluate_intervention_outcome(
    intervention: &Value,
    before: &Value,
    after: &Value,
    result: &Value,
    tool_succeeded: bool,
) -> Value {
    if !tool_succeeded {
        return json!({"status":"failed","reason":"tool_dispatch_failed",
            "verifiedChecks":0,"unverifiedChecks":0});
    }
    let checks = intervention["expectedChecks"].as_array();
    let Some(checks) = checks else {
        return json!({"status":"unrated","reason":"no_expected_checks",
            "verifiedChecks":0,"unverifiedChecks":0});
    };
    if checks.is_empty() || checks.len() > 32 {
        return json!({"status":"unrated","reason":"missing_or_excessive_checks",
            "verifiedChecks":0,"unverifiedChecks":checks.len()});
    }
    let mut executable = Vec::new();
    let mut opaque = 0usize;
    let mut host_state_check = false;
    for check in checks {
        let predicate = check.get("predicate")
            .filter(|value| value.is_object())
            .or_else(|| check.get("type").filter(|value| value.is_string())
                .map(|_| check));
        if let Some(predicate) = predicate {
            host_state_check |= predicate["from"] == "after";
            executable.push(json!({"predicate":predicate}));
        } else {
            opaque += 1;
        }
    }
    if executable.is_empty() {
        return json!({"status":"unrated","reason":"checks_are_not_executable",
            "verifiedChecks":0,"unverifiedChecks":opaque});
    }
    // This is independent host-observed state for the *selected* intervention,
    // not a self-reported field inside the original tool result.
    let state_changed = before["pathState"].is_object()
        && after["pathState"].is_object()
        && before["pathState"] != after["pathState"];
    let scope = json!({"before":before,"after":after,"result":result});
    match crate::engine::program::evaluate_verification_checks(&executable, &scope) {
        Ok(evaluation) => {
            let status = if evaluation["failed"] == true {
                "failed"
            } else if evaluation["ok"] == true && opaque == 0
                && host_state_check && state_changed
            {
                "verified"
            } else {
                "unrated"
            };
            json!({
                "status":status,
                "reason":if status=="verified" {"host_observed_postcondition_passed"}
                    else if status=="failed" {"expected_postcondition_refuted"}
                    else {"missing_independent_state_change_or_complete_checks"},
                "verifiedChecks":executable.len(),
                "unverifiedChecks":opaque,
                "hostStateCheck":host_state_check,
                "hostStateChanged":state_changed,
                "beforePathState":before["pathState"],
                "afterPathState":after["pathState"],
                "evaluation":evaluation,
            })
        }
        Err(error) => json!({
            "status":"unrated","reason":"invalid_check",
            "verifiedChecks":0,"unverifiedChecks":checks.len(),
            "diagnostic":error.chars().take(240).collect::<String>(),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn record_adaptation(
    state: &AppState,
    execution_id: &str,
    round: usize,
    failed_tool: &str,
    failed_arguments: &Value,
    error: &str,
    adaptation: &Value,
    selected: &Value,
    environment_version: u64,
    result: Option<&Value>,
    hypothesis_results: &[Value],
    intervention_outcome: &Value,
) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let failure_signature = crate::engine::memory::stable_id(&[
        failed_tool,
        &failed_arguments.to_string(),
    ]);
    let record = json!({
        "id": format!(
            "adaptation:{}",
            crate::engine::memory::stable_id(&[execution_id, &failure_signature, &selected["id"].as_str().unwrap_or("")])
        ),
        "runId": execution_id,
        "stepId": round,
        "failureSignature": failure_signature,
        "environmentVersion": environment_version,
        "observation": {
            "kind": "tool_failure",
            "tool": failed_tool,
            "arguments": failed_arguments,
            "expected": "success",
            "actual": error.chars().take(600).collect::<String>(),
        },
        "beforeObservation": adaptation["beforeObservation"],
        "hypotheses": adaptation["hypotheses"],
        "interventions": adaptation["interventions"],
        "consultedExperienceRules": adaptation["consultedExperienceRules"],
        "selected": selected,
        "result": result,
        "hypothesisResults": hypothesis_results,
        "interventionOutcome": intervention_outcome,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let mut adaptations = execution["adaptations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    adaptations.push(record.clone());
    if adaptations.len() > 20 {
        adaptations = adaptations.split_off(adaptations.len() - 20);
    }
    execution["adaptations"] = json!(adaptations);
    execution["lastAdaptation"] = record.clone();
    execution["adaptationCheckpoint"] = json!({
        "environmentVersion": environment_version,
        "failureSignature": record["failureSignature"],
        "selectedIntervention": selected["id"],
        "selectedEvaluation": selected["evaluation"],
        "environmentFingerprint": adaptation["environmentFingerprint"],
        "adaptationRevision": execution["adaptationCheckpoint"]["adaptationRevision"]
            .as_u64().unwrap_or(0) + 1,
        "beforeObservation": adaptation["beforeObservation"],
        "hypothesisResults": hypothesis_results,
        "interventionOutcome": intervention_outcome,
    });
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(record)
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn adaptation() -> Value {
        json!({
            "hypotheses":[
                {"id":"h1","claim":"permission","predictions":[{"predicate":{"type":"equals","from":"result","path":"/exitCode","value":0}}]},
                {"id":"h2","claim":"wrong path","predictions":[{"predicate":{"type":"exists","from":"result","path":"/missing"}}]}
            ],
            "interventions":[
                {"id":"same","hypothesisIds":["h1"],"tool":"shell_exec","inputs":{"command":"exit 7"},"expectedChecks":["x"],"estimatedCost":0.1},
                {"id":"change","hypothesisIds":["h1","h2"],"tool":"workspace_write","inputs":{"path":"a.txt","content":"ok"},"expectedChecks":["a","b"],"estimatedCost":0.2}
            ]
        })
    }

    #[test]
    fn identical_failed_call_is_filtered_until_the_environment_changes() {
        let failed = json!({"command":"exit 7"});
        let chosen = choose_intervention(&adaptation(), "shell_exec", &failed, false).unwrap();
        assert_eq!(chosen["id"], "change");
        let retry = choose_intervention(&adaptation(), "shell_exec", &failed, true).unwrap();
        assert_eq!(retry["id"], "change"); // still higher score than identical retry
        let only_same = json!({"hypotheses":[],"interventions":[adaptation()["interventions"][0].clone()]});
        assert!(choose_intervention(&only_same, "shell_exec", &failed, false).is_none());
        assert!(choose_intervention(&only_same, "shell_exec", &failed, true).is_some());
    }

    #[test]
    fn noisy_model_output_is_parsed_and_invalid_schema_is_rejected() {
        let raw = "Here is the plan {not json} then:\n```json\n{\"hypotheses\":[{\"id\":\"h1\",\"claim\":\"missing file\",\"predictions\":[]}],\"interventions\":[{\"id\":\"i1\",\"hypothesisIds\":[\"h1\"],\"tool\":\"workspace_list\",\"inputs\":{},\"expectedChecks\":[\"entries\"],\"estimatedCost\":0.1}]}\n```\nDone.";
        let parsed = parse_adaptation(raw).expect("prose-wrapped JSON");
        assert_eq!(parsed["interventions"][0]["tool"], "workspace_list");
        assert!(validate_adaptation(&parsed).is_ok());
        let invalid = json!({
            "hypotheses":[{"id":"h1","claim":"x","predictions":[]}],
            "interventions":[{"id":"i1","hypothesisIds":["h1"],"tool":"workspace_list","inputs":"not-an-object"}]
        });
        assert!(validate_adaptation(&invalid).is_err());
    }

    #[test]
    fn predictions_refute_the_wrong_hypothesis() {
        let results = evaluate_hypotheses(&adaptation(), &json!({"exitCode": 7}));
        assert_eq!(results[0]["status"], "refuted");
        assert_eq!(results[1]["status"], "refuted");
        let success = evaluate_hypotheses(&adaptation(), &json!({"exitCode": 0, "missing": 1}));
        assert_eq!(success[0]["status"], "supported");
        assert_eq!(success[1]["status"], "supported");
    }
    #[test]
    fn intervention_success_requires_actual_executable_postconditions() {
        let old = json!({"pathState":{"path":{"exists":false}}});
        let after = json!({"pathState":{"path":{"exists":true,"len":5}}});
        let outcome = json!({"saved":true,"path":"result.txt"});
        let observed = json!({"expectedChecks":[{
            "predicate":{"type":"equals","from":"after",
                "path":"/pathState/path/exists","value":true}
        }]});
        assert_eq!(evaluate_intervention_outcome(
            &observed,&old,&after,&outcome,true)["status"],"verified");
        assert_eq!(evaluate_intervention_outcome(
            &observed,&old,&old,&outcome,true)["status"],"failed");
        assert_eq!(evaluate_intervention_outcome(
            &observed,&old,&json!({}),&outcome,true)["status"],"unrated");
        assert_eq!(evaluate_intervention_outcome(
            &observed,&old,&after,&outcome,false)["status"],"failed");
        let prose = json!({"expectedChecks":["file_exists","write succeeded"]});
        assert_eq!(evaluate_intervention_outcome(
            &prose,&old,&after,&outcome,true)["status"],"unrated");
        // A model repeating the tool's own success flag is never rated as
        // independently verified, even if the host filesystem did change.
        let echo = json!({"expectedChecks":[{
            "predicate":{"type":"equals","from":"result","path":"/saved","value":true}
        }]});
        assert_eq!(evaluate_intervention_outcome(
            &echo,&old,&after,&outcome,true)["status"],"unrated");
    }
    #[test]
    fn contrastive_diagnostic_selection_requires_disagreeing_predictions() {
        let adaptations = json!({
            "hypotheses":[
                {"id":"h1","predictions":[{"observationPhase":"after_intervention",
                    "predicate":{"type":"equals","from":"result","path":"/reason","value":"permissions"}}]},
                {"id":"h2","predictions":[{"observationPhase":"after_intervention",
                    "predicate":{"type":"equals","from":"result","path":"/reason","value":"missing_file"}}]}
            ],
            "interventions":[
                {"id":"blind_write","hypothesisIds":["h1","h2"],"tool":"workspace_write",
                    "inputs":{"path":"x","content":"guess"},"expectedProgress":0.8,
                    "estimatedCost":0.1},
                {"id":"inspect","hypothesisIds":["h1","h2"],"tool":"workspace_read",
                    "inputs":{"path":"x"},"expectedProgress":0.3,
                    "expectedChecks":[{"predicate":{"type":"exists",
                        "from":"result","path":"/reason"}}],
                    "estimatedCost":0.1}
            ]
        });
        let selected = choose_intervention(&adaptations,"shell_exec",&json!({"command":"false"}),false)
            .expect("read-only disambiguating action");
        assert_eq!(selected["id"],"inspect");
        assert_eq!(selected["evaluation"]["contrastedHypothesisPairs"],1);
        assert_eq!(selected["evaluation"]["hypothesisDiscrimination"],1.0);
        let inflated = json!({"hypotheses":[
            {"id":"a","predictions":[]},{"id":"b","predictions":[]}],
            "interventions":[{"id":"i","hypothesisIds":["a","b"],"tool":"workspace_read",
                "inputs":{"path":"x"},"expectedProgress":0.3,"estimatedCost":0.1}]});
        let chosen = choose_intervention(&inflated,"shell_exec",&json!({}),false).unwrap();
        assert_eq!(chosen["evaluation"]["hypothesisDiscrimination"],0.0);
        let unrelated = json!({"hypotheses":adaptations["hypotheses"],
            "interventions":[{"id":"wrong_probe","hypothesisIds":["h1","h2"],
                "tool":"workspace_read","inputs":{"path":"x"},
                "expectedChecks":[{"predicate":{"type":"exists",
                    "from":"result","path":"/unrelated"}}],
                "estimatedCost":0.1}]});
        let checked = choose_intervention(&unrelated,"shell_exec",&json!({}),false).unwrap();
        assert_eq!(checked["evaluation"]["hypothesisDiscrimination"],0.0);
    }

}
