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
    // Hypothesis ids are local to a failure. Reusing h1 in another task
    // must never inherit an unrelated refutation.
    let failure_signature = crate::engine::memory::stable_id(&[
        tool, &arguments.to_string()
    ]);
    let refuted = state
        .store
        .get("execution", execution_id)
        .map(|execution| {
            execution["adaptations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|record| {
                    record["failureSignature"].as_str() == Some(failure_signature.as_str())
                        && record["environmentFingerprint"].as_str()
                            == Some(environment_fingerprint.as_str())
                })
                .flat_map(|record| {
                    record["hypothesisResults"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                })
                .filter(|result| result["status"] == "refuted")
                .filter_map(|result| result["id"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut selection_context = json!({
        "before": before_observation,
        "result": before_observation,
        "after": Value::Null,
    });
    // Diagnostic policy: choose only experiments that divide competing
    // predictions. Every read is charged, cancellable and journaled through
    // the SAME gateway as other tool calls; diagnostic success is never a
    // successful task. Stop when observations no longer have discriminating
    // value, not after an arbitrary number of pre-selected probes.
    let mut diagnostic_evidence: Vec<Value> = Vec::new();
    let mut attempted: Vec<String> = state.store.get("execution", execution_id)
        .and_then(|execution| execution["adaptationDiagnosticHistory"].as_array().cloned())
        .unwrap_or_default().iter().filter(|entry| {
            entry["failureSignature"].as_str() == Some(failure_signature.as_str())
                && entry["environmentFingerprint"].as_str()
                    == Some(environment_fingerprint.as_str())
        }).filter_map(|entry| entry["signature"].as_str().map(str::to_string))
        .collect();
    let mut causal_refuted = refuted.clone();
    const MAX_DIAGNOSTICS_PER_FAILURE: usize = 4;
    while diagnostic_evidence.len() < MAX_DIAGNOSTICS_PER_FAILURE {
        let snapshot = budget.snapshot();
        let spent = snapshot["tools"].as_u64().unwrap_or(u64::MAX);
        let limit = snapshot["maxTools"].as_u64().unwrap_or(0);
        // Keep one charged call for the actual repair.
        if spent.saturating_add(1) >= limit || !budget.can_execute_tool() {
            break;
        }
        let remaining_steps = MAX_DIAGNOSTICS_PER_FAILURE
            .saturating_sub(diagnostic_evidence.len())
            .min(limit.saturating_sub(spent).saturating_sub(1) as usize);
        let Some(diagnostic) = crate::engine::adaptation::diagnostics::choose_with_budget(
            &adaptation, &causal_refuted, &attempted, tool, arguments,
            remaining_steps
        ) else { break; };
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let diagnosis_tool = diagnostic["tool"].as_str().unwrap_or("").to_string();
        let inputs = diagnostic["inputs"].clone();
        let signature = crate::engine::adaptation::diagnostics::signature(
            &diagnosis_tool, &inputs
        );
        attempted.push(signature);
        if !budget.charge_tool() { break; }
        *op_seq += 1;
        let call = ToolCall {
            id: format!("diagnose-{}", uuid::Uuid::new_v4()),
            name: diagnosis_tool,
            arguments: inputs.to_string(),
        };
        let (observation, ok, _) = dispatch_tool_call(
            state, tx, session, request, session_id, execution_id,
            &call, inputs, metrics, *op_seq, round, 0, false,
            "adaptation_diagnostic", None, None, None
        ).await?;
        let operation_id = format!("{execution_id}:{round}:0:{}", call.id);
        let mut evidence = crate::engine::adaptation::diagnostics::observe(
            &diagnostic, &observation, ok, &operation_id
        );
        evidence["failureSignature"] = json!(failure_signature);
        evidence["environmentFingerprint"] = json!(environment_fingerprint);
        if evidence["verdict"] != "unknown" {
            selection_context["result"] = observation.clone();
            selection_context["after"] = observation.clone();
        }
        // A crash after the observation must not erase which hypotheses were
        // actually tested. The complete result remains in execution_operation.
        let mut stored = state.store.get("execution", execution_id)
            .ok_or("진단 기록을 저장할 실행이 없습니다.")?;
        let mut history = stored["adaptationDiagnosticHistory"]
            .as_array().cloned().unwrap_or_default();
        history.push(evidence.clone());
        if history.len() > 24 {
            history = history.split_off(history.len() - 24);
        }
        stored["adaptationDiagnosticHistory"] = json!(history);
        let saved = state.store.put("execution", &stored)
            .ok_or("진단 기록을 영속화하지 못했습니다.")?;
        state.bus.record("execution", &saved);
        diagnostic_evidence.push(evidence);
        let newly_contradicted =
            crate::engine::adaptation::diagnostics::contradicted(&diagnostic_evidence);
        causal_refuted = refuted.iter().chain(newly_contradicted.iter())
            .cloned().collect();
    }
    adaptation["diagnosticEvidence"] = json!(diagnostic_evidence);
    let observations = diagnostic_evidence.iter().any(|item| {
        item["verdict"] == "true" || item["verdict"] == "false"
    });
    // Compose AFTER receiving results. A model cannot treat its own proposed
    // experiment as evidence. If the budget is gone, the existing interventions
    // remain available and are filtered by actual contradictions.
    if observations && budget.charge_round() {
        metrics.lock().unwrap().model_calls += 1;
        let compose_system = "You are the DOMINION evidence-based repair composer. \
            Observations are tool receipts, not success claims. Use the observed \
            diagnostics and surviving causal hypotheses to revise only the proposed \
            repair actions. Return JSON with hypotheses and interventions. Keep \
            hypothesis IDs stable and never repeat a diagnostic tool+inputs or \
            the original failed call. No simulated results, no unverified facts.";
        let compose_prompt = format!(
            "Original failed tool: {tool}\nFailure: {error}\nOriginal hypotheses: {}\n\
             Original interventions: {}\nActual diagnostics: {}\nContradicted IDs: {}\n\
             Propose new interventions conditioned on these observations. JSON only.",
            adaptation["hypotheses"], adaptation["interventions"],
            adaptation["diagnosticEvidence"], json!(causal_refuted),
        );
        let mut native = load_native_checkpoint(state, execution_id, &request.input)?;
        let journal = ChatKernelJournal {
            state, execution_id, session_id,
            generation: state.cancellation_generation(session_id),
        };
        let proposed = dominion_core::engine::model_call(
            &mut native, &journal, "adaptation_compose",
            llm::complete_for_session(
                state, &provider_id, &model, compose_system,
                &compose_prompt, session_id,
            ),
        ).await;
        let proposed = match proposed {
            Ok(Ok(text)) => Some(text),
            Ok(Err(error)) | Err(error) => {
                if state.is_cancelled(session_id) {
                    return Err("실행이 중단되었습니다.".into());
                }
                adaptation["compositionError"] = json!(error.to_string());
                None
            }
        };
        if let Some(composed) = proposed.as_deref()
            .and_then(crate::engine::adaptation_loop::parse_adaptation)
            .filter(|value| crate::engine::adaptation_loop::validate_adaptation(value).is_ok())
        {
            let old_ids = adaptation["hypotheses"].as_array().into_iter().flatten()
                .filter_map(|item| item["id"].as_str()).collect::<Vec<_>>();
            let new_ids = composed["hypotheses"].as_array().into_iter().flatten()
                .filter_map(|item| item["id"].as_str()).collect::<Vec<_>>();
            // If all old explanations were contradicted, genuinely replace the
            // hypothesis set rather than silently resurrecting the old one.
            if old_ids.len() == new_ids.len()
                && old_ids.iter().all(|id| new_ids.contains(id))
            {
                adaptation["interventions"] = composed["interventions"].clone();
                adaptation["composition"] = json!("evidence_conditioned");
            } else if old_ids.iter().all(|id| causal_refuted.iter().any(|x| x == id)) {
                adaptation["hypotheses"] = composed["hypotheses"].clone();
                adaptation["interventions"] = composed["interventions"].clone();
                causal_refuted.clear();
                adaptation["composition"] = json!("new_hypotheses_after_contradiction");
            }
        }
    }
    // Observations are not repairs: do not execute the same read again as
    // the selected remediation merely because the composer nominated it.
    if let Some(items) = adaptation["interventions"].as_array_mut() {
        items.retain(|item| {
            let key = crate::engine::adaptation::diagnostics::signature(
                item["tool"].as_str().unwrap_or(""), &item["inputs"]
            );
            !attempted.iter().any(|used| used == &key)
        });
    }
    // Freeze the existing owner acceptance contract BEFORE the candidate
    // repair runs. The very same checks are used AFTER dispatch; a model may
    // suggest strategies, but cannot silently rewrite what counts as success.
    let contract = state.store.get("execution", execution_id).and_then(|record| {
        let checks = record["acceptanceChecks"].clone();
        let intent = record["acceptanceText"].as_str()?.to_string();
        let source = record["acceptanceSourceMessageId"].as_str()
            .map(str::to_string);
        if checks.as_array().is_some_and(|items| !items.is_empty()) {
            Some((intent, checks, source))
        } else {
            None
        }
    });
    let acceptance_evidence = |source: &Option<String>| -> Result<Vec<Value>, String> {
        match source {
            Some(owner_source) => Ok(evidence_for_source(
                state, session_id, owner_source, execution_id
            )),
            None => Ok(state.store.execution_operations(execution_id)?
                .into_iter().filter(|entry| entry["status"] == "completed")
                .collect()),
        }
    };
    let before_gaps = if let Some((intent, checks, source)) = &contract {
        Some(verification::check_acceptance(
            state, intent, checks, &acceptance_evidence(source)?
        ))
    } else { None };

    let selected = crate::engine::adaptation_loop::choose_intervention_with_context(
        &adaptation, tool, arguments, false,
        &selection_context, &causal_refuted,
    )
    .ok_or("no distinct intervention survived selection")?;
    let selected_tool = selected["tool"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("selected intervention has no tool")?
        .to_string();
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
    let selected_inputs = inputs.clone();
    let selected_tool_name = call.name.clone();
    let selected_call_id = call.id.clone();
    let (result, ok, _content) = dispatch_tool_call(
        state,
        tx,
        session,
        request,
        session_id,
        execution_id,
        &call,
        inputs,
        metrics,
        *op_seq,
        round,
        0,
        false,
        "adaptation",
        None,
        None,
        None,
    )
    .await?;
    // Contrasting a model-proposed repair against the frozen owner contract:
    // Harness²-style real probe evidence, dispatched under DeepSeek-style
    // native tool admission, is subjected to PRISM-style no-regression gating.
    // This is an in-task gate, not a claim of an independent holdout benchmark.
    let acceptance_decision = if let (Some((intent, checks, source)), Some(before)) =
        (&contract, &before_gaps)
    {
        let receipt_id = format!("{execution_id}:{round}:0:{selected_call_id}");
        let receipt = state.store.get("execution_operation", &receipt_id);
        let verified_receipt = crate::engine::adaptation::acceptance_gate::matching_receipt(
            receipt.as_ref(), execution_id, &selected_call_id, &selected_tool_name,
            &selected_inputs,
        );
        let after = verification::check_acceptance(
            state, intent, checks, &acceptance_evidence(source)?
        );
        crate::engine::adaptation::acceptance_gate::assess(
            before, &after, verified_receipt,
        )
    } else {
        json!({"decision":"not_applicable","accepted":false,
            "basis":"no_frozen_owner_acceptance_contract"})
    };
    adaptation["acceptanceDecision"] = acceptance_decision.clone();
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
        "diagnosticEvidence": adaptation["diagnosticEvidence"],
        "acceptanceDecision": acceptance_decision,
    }))
}

