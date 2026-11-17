//! Bounded model adaptation and failure observation as explicit work.
use super::*;

pub(super) fn failure_observation(
    state: &AppState,
    tool: &str,
    arguments: &Value,
    error: &str,
) -> Value {
    let mut path_state = serde_json::Map::new();
    for key in ["path", "cwd", "command"] {
        let Some(value) = arguments.get(key).and_then(Value::as_str) else {
            continue;
        };
        let path = if std::path::Path::new(value).is_absolute() {
            std::path::PathBuf::from(value)
        } else {
            state.config.workspace.join(value)
        };
        if let Ok(metadata) = std::fs::metadata(&path) {
            path_state.insert(
                key.to_string(),
                json!({
                    "exists": true,
                    "isDir": metadata.is_dir(),
                    "len": metadata.len(),
                }),
            );
        } else {
            path_state.insert(key.to_string(), json!({"exists": false}));
        }
    }
    json!({
        "kind": "tool_failure",
        "tool": tool,
        "arguments": arguments,
        "error": error.chars().take(600).collect::<String>(),
        "pathState": path_state,
    })
}

pub(super) fn environment_fingerprint(state: &AppState, tool: &str, arguments: &Value) -> String {
    let observation = failure_observation(state, tool, arguments, "");
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&state.config.workspace) {
        for entry in read_dir.flatten().take(64) {
            let name = entry.file_name().to_string_lossy().to_string();
            let len = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            entries.push((name, len));
        }
    }
    entries.sort();
    crate::engine::memory::stable_id(&[
        tool,
        &arguments.to_string(),
        &observation["pathState"].to_string(),
        &json!(entries).to_string(),
    ])
}

pub(super) fn record_adaptation_error(state: &AppState, execution_id: &str, error: &str) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["lastAdaptationError"] = json!(error.chars().take(600).collect::<String>());
        if let Some(saved) = state.store.put("execution", &execution) {
            state.bus.record("execution", &saved);
        }
    }
}

fn adaptation_repair_prompt(original: &str, raw: &str, error: &str) -> String {
    format!(
        "{original}\n\nYour previous response was rejected: {error}.\nPrevious response excerpt:\n{}\nReturn ONLY one JSON object with non-empty \"hypotheses\" and \"interventions\" arrays. Do not write prose or markdown. Each intervention needs id, hypothesisIds (array), tool, inputs (object), expectedChecks (array) and estimatedCost (number).",
        raw.chars().take(900).collect::<String>()
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn attempt_model_adaptation(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    round: usize,
    _call_id: &str,
    tool: &str,
    arguments: &Value,
    error: &str,
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::super::budget::BudgetLedger,
    op_seq: &mut u64,
    environment_version: u64,
) -> Result<Value, String> {
    let provider_id = request
        .provider_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("adaptation has no provider")?
        .to_string();
    let model = request
        .model
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("adaptation has no model")?
        .to_string();
    let before_observation = failure_observation(state, tool, arguments, error);
    let environment_fingerprint = environment_fingerprint(state, tool, arguments);
    let (system, mut prompt) = crate::engine::adaptation_loop::adaptation_prompt(
        tool,
        arguments,
        error,
        environment_version,
        &before_observation,
        &environment_fingerprint,
    );
    // Design 2.4 §6: reuse a verified precedent only when its observable
    // failure predicate matches. The old inputs are an example, not an action
    // to replay. The model must instantiate new inputs for this task.
    let scope = crate::engine::memory::MemoryScope::for_session(
        &state.store, session_id, request.agent_id.as_deref(),
    );
    let applicable_rules = crate::engine::experience::matching_rules_with_execution(
        state, &scope, tool, arguments, error, 3, Some(execution_id),
    );
    if !applicable_rules.is_empty() {
        let examples = applicable_rules.iter().map(|rule| json!({
            "id": rule["id"],
            "revision": rule["revision"],
            "evidenceGrade": rule["evidenceGrade"],
            "applicability": rule["applicability"],
            "intervention": rule["intervention"],
            "supportingCases": rule["supportingCases"].as_array().map(Vec::len).unwrap_or(0),
        })).collect::<Vec<_>>();
        prompt.push_str(
            "\nConditional precedents from VERIFIED past runs (observed, not experimentally promoted).              The failure condition matched, but do not copy old input values without adapting              them to the current task. Propose and validate new arguments: ",
        );
        prompt.push_str(&json!(examples).to_string());
    }
    if let Some(principles) = crate::engine::principle::context_block(
        state,
        request.agent_id.as_deref(),
        &model,
        tool,
        arguments,
        error,
        Some(execution_id),
    ) {
        prompt.push_str("\n\n");
        prompt.push_str(&principles);
    }
    let mut attempt_prompt = prompt.clone();
    let mut adaptation = None;
    let mut last_error = String::new();
    for attempt in 0..2 {
        if !budget.charge_round() { return Err("adaptation model budget exhausted".into()); }
        metrics.lock().unwrap().model_calls += 1;
        let mut native=load_native_checkpoint(state,execution_id,&request.input)?;
        let journal=ChatKernelJournal{state,execution_id,session_id,generation:state.cancellation_generation(session_id)};
        let raw = dominion_core::engine::model_call(&mut native,&journal,"adaptation",
            llm::complete_for_session(state,&provider_id,&model,&system,&attempt_prompt,session_id))
            .await??;
        let candidate = crate::engine::adaptation_loop::parse_adaptation(&raw);
        match candidate {
            Some(candidate) => {
                match crate::engine::adaptation_loop::validate_adaptation(&candidate) {
                    Ok(()) => {
                        adaptation = Some(candidate);
                        break;
                    }
                    Err(error) => last_error = error,
                }
            }
            None => {
                last_error =
                    "response had no JSON object with hypotheses and interventions".into();
            }
        }
        if attempt == 0 {
            attempt_prompt = adaptation_repair_prompt(&prompt, &raw, &last_error);
        }
    }
    let mut adaptation = adaptation.ok_or_else(|| {
        format!("adaptation output rejected after one repair attempt: {last_error}")
    })?;
    adaptation["consultedExperienceRules"] = json!(applicable_rules.iter().map(|rule| json!({
        "id": rule["id"], "revision": rule["revision"],
    })).collect::<Vec<_>>());
    adaptation["beforeObservation"] = before_observation.clone();
    adaptation["environmentFingerprint"] = json!(environment_fingerprint);
    let refuted = state
        .store
        .get("execution", execution_id)
        .map(|execution| {
            execution["adaptations"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|adaptation| {
                    adaptation["hypothesisResults"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                })
                .filter(|result| result["status"] == "refuted")
                .filter_map(|result| result["id"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let selection_context = json!({
        "before": before_observation,
        "result": before_observation,
        "after": Value::Null,
    });
    let adaptation_algorithm =
        crate::engine::algorithms::choose_for_agent(
            state,
            "adaptation.selection",
            request.agent_id.as_deref(),
            Some(execution_id),
        )
        .unwrap_or(Value::Null);
    // Bounded task-level propose/probe/compose: execute only explicitly
    // local read-only diagnostic candidates before selecting the intervention.
    // Every probe uses the normal dispatch journal, cancellation and approvals.
    // Reserve a tool slot for final action; model-authored predictions never
    // count as observed evidence until the tool result passes a predicate.
    let mut probe_outcomes = Vec::new();
    let mut performed_probes: Vec<(String, Value, bool)> = Vec::new();
    if adaptation["hypotheses"].as_array().is_some_and(|items| items.len() >= 2) {
        let probes = crate::engine::adaptation_loop::probe_candidates(
            &adaptation, tool, arguments, &selection_context, &refuted, &adaptation_algorithm
        );
        for probe in probes {
            let snapshot = budget.snapshot();
            let used = snapshot["tools"].as_u64().unwrap_or(u64::MAX);
            let max = snapshot["maxTools"].as_u64().unwrap_or(0);
            if used.saturating_add(1) >= max || !budget.can_execute_tool() {
                break;
            }
            if state.is_cancelled(session_id) {
                return Err("실행이 중단되었습니다.".into());
            }
            if !budget.charge_tool() { break; }
            let probe_tool = probe["tool"].as_str().unwrap_or("");
            let inputs = probe["inputs"].clone();
            *op_seq += 1;
            let call = ToolCall {
                id: format!("adapt-probe-{}", uuid::Uuid::new_v4()),
                name: probe_tool.to_string(),
                arguments: inputs.to_string(),
            };
            let (observation, ok, _text) = dispatch_tool_call(
                state, tx, session, request, session_id, execution_id, &call,
                inputs, metrics, *op_seq, round, 0, false,
                "adaptation_probe", None, None, None
            ).await?;
            let verdict = if ok {
                let observed = json!({
                    "before": before_observation,
                    "result": observation,
                });
                crate::engine::program::predicate_verdict(&probe["probePredicate"], &observed)
                    .map(|verdict| verdict.as_str())
                    .unwrap_or("unknown")
            } else {
                "tool_failed"
            };
            probe_outcomes.push(json!({
                "id": probe["id"],
                "hypothesisIds": probe["hypothesisIds"],
                "tool": probe_tool,
                "operationId": call.id,
                "toolOk": ok,
                "verdict": verdict,
                "predicate": probe["probePredicate"],
                "observationBasis": "durable_tool_result_not_task_completion",
            }));
            performed_probes.push((
                probe["id"].as_str().unwrap_or("").to_string(), observation, ok
            ));
        }
    }
    adaptation["probeOutcomes"] = json!(probe_outcomes);
    let selected = crate::engine::adaptation_loop::choose_after_probes(
        &adaptation, tool, arguments, &selection_context,
        &refuted, &adaptation_algorithm, &probe_outcomes
    ).ok_or("no distinct intervention survived probe-aware selection")?;
    let selected_tool = selected["tool"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("selected intervention has no tool")?
        .to_string();
    // A diagnostic already dispatched for this run cannot be dispatched
    // again merely because compose selected it. Reuse the recorded result.
    let result_from_probe = performed_probes.iter()
        .find(|(id, _, _)| selected["id"].as_str() == Some(id.as_str()))
        .map(|(_, result, ok)| (result.clone(), *ok));
    let (result, ok) = if let Some(cached) = result_from_probe {
        cached
    } else {
        let inputs = selected["inputs"].clone();
        if !inputs.is_object() {
            return Err("selected intervention inputs are not an object".into());
        }
        if !budget.charge_tool() {
            return Err("adaptation tool budget is exhausted".into());
        }
        *op_seq += 1;
        let call = ToolCall {
            id: format!("adapt-{}-{}", selected["id"].as_str().unwrap_or("i"), uuid::Uuid::new_v4()),
            name: selected_tool,
            arguments: inputs.to_string(),
        };
        let (result, ok, _content) = dispatch_tool_call(
            state, tx, session, request, session_id, execution_id,
            &call, inputs, metrics, *op_seq, round, 0, false,
            "adaptation", None, None, None
        ).await?;
        (result, ok)
    };
    crate::engine::algorithms::mark_signal(
        state, execution_id, "adaptation.selection", ok,
    );
    crate::engine::algorithms::mark_signal(
        state, execution_id, "learning.applicability", ok,
    );
    let hypothesis_results = crate::engine::adaptation_loop::evaluate_hypotheses_phased(
        &adaptation,
        &before_observation,
        &result,
    );
    if applicable_rules
        .iter()
        .any(|rule| rule["scope"].as_str() == Some("shared"))
    {
        crate::engine::algorithms::mark_signal(
            state,
            execution_id,
            "multiagent.sharing",
            tool_succeeded(&result),
        );
    }
    crate::engine::experience::record_rule_counterexamples(
        state, &applicable_rules, &selected, &result, execution_id,
    );
    let record = crate::engine::adaptation_loop::record_adaptation(
        state,
        execution_id,
        round,
        tool,
        arguments,
        error,
        &adaptation,
        &selected,
        environment_version,
        Some(&result),
        &hypothesis_results,
    );
    Ok(json!({
        "record": record,
        "selected": selected,
        "result": result,
        "hypothesisResults": hypothesis_results,
    }))
}

