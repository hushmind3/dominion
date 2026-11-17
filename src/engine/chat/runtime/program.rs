//! Durable program execution, checkpoints, replay guards and adaptive repair.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_harness_nodes(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    strategy_candidates: &[Value],
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::budget::BudgetLedger,
    op_seq: &mut u64,
    generation: u64,
) -> Result<Vec<Value>, String> {
    let mut synthetic = Vec::new();
    for strategy in strategy_candidates {
        if strategy["assistanceMode"].as_str() != Some("harness_assisted") {
            continue;
        }
        if !crate::engine::procedure::applies(strategy, &request.input, None, None) {
            continue;
        }
        let mut results = std::collections::HashMap::new();
        for _ in 0..16 {
            if state.is_cancelled(session_id)
                || state.cancellation_generation(session_id) != generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            let operations=state.store.execution_operations(execution_id)?;
            for operation in &operations {
                if operation["origin"].as_str() == Some("procedure")
                    && operation["procedureStrategyId"].as_str() == strategy["id"].as_str()
                    && operation["procedureRevision"].as_str() == strategy["revision"].as_str()
                    && crate::engine::procedure::operation_succeeded(operation)
                {
                    if let Some(node) = operation["procedureNodeId"].as_str() {
                        results.insert(node.to_string(), operation["result"].clone());
                    }
                }
            }
            let Some((node, tool, arguments)) = crate::engine::procedure::ready_intent(
                strategy,
                &operations,
                &request.input,
                &results,
            ) else {
                break;
            };
            if !budget.charge_tool() {
                return Err(format!(
                    "harness 절차 도구 예산({MAX_TOOL_CALLS}회)을 초과했습니다."
                ));
            }
            *op_seq += 1;
            let call = ToolCall {
                id: format!("procedure-{node}-{}", uuid::Uuid::new_v4()),
                name: tool,
                arguments: arguments.to_string(),
            };
            let (result, ok, content) = dispatch_tool_call(
                state,
                tx,
                session,
                request,
                session_id,
                execution_id,
                &call,
                arguments,
                metrics,
                *op_seq,
                0,
                0,
                true,
                "procedure",
                Some(&node),
                strategy["id"].as_str(),
                strategy["revision"].as_str(),
            )
            .await?;
            results.insert(node.clone(), result);
            synthetic.push(json!({
                "role": "system",
                "content": format!(
                    "Harness procedure node {node} executed (origin=procedure). result: {}",
                    content.chars().take(1200).collect::<String>()
                )
            }));
            if !ok {
                break;
            }
        }
    }
    Ok(synthetic)
}

/// Observable state related to a failed call. This is what "the environment
/// changed" means for retry admission; a successful call count is not proof.
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

pub(super) fn adaptation_repair_prompt(original: &str, raw: &str, error: &str) -> String {
    format!(
        "{original}\n\nYour previous response was rejected: {error}.\nPrevious response excerpt:\n{}\nReturn ONLY one JSON object with non-empty \"hypotheses\" and \"interventions\" arrays. Do not write prose or markdown. Each intervention needs id, hypothesisIds (array), tool, inputs (object), expectedChecks (array), estimatedCost (number). Every mutating/shell intervention MUST contain evidenceIds (array) citing current-failure, before, or a real receipt upstream of the failed operation; do not cite temporal neighbors.",
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
    failed_call_id: &str,
    tool: &str,
    arguments: &Value,
    error: &str,
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::budget::BudgetLedger,
    op_seq: &mut u64,
    environment_version: u64,
    generation: u64,
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
    // Backward evidence slice from native receipts, not a fabricated model
    // analysis. Historical file contents and access tokens never enter here.
    let failure_trace = crate::engine::adaptation::trace_ir::compile(
        &state.store.recent_execution_operations(execution_id, 64)?,
        failed_call_id, tool, arguments,
    );
    prompt.push_str("\n\nNative execution provenance (evidence, not a causal conclusion):\n");
    prompt.push_str(&failure_trace.to_string());
    prompt.push_str("\nUse only the trace evidence IDs above or before/current-failure. ");
    prompt.push_str("Distinguish observation from hypothesis; never invent tool receipts.");
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
        let raw = llm::complete_for_session(
            state,
            &provider_id,
            &model,
            &system,
            &attempt_prompt,
            session_id,
        )
        .await?;
        metrics.lock().unwrap().model_calls += 1;
        let candidate = crate::engine::adaptation_loop::parse_adaptation(&raw);
        match candidate {
            Some(candidate) => {
                let validity = crate::engine::adaptation_loop::validate_adaptation(&candidate)
                    .and_then(|()|crate::engine::adaptation::trace_ir::validate_evidence_ids(
                        &candidate,&failure_trace,
                    ));
                match validity {
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
    adaptation["failureTrace"] = failure_trace.clone();
    // Hypothesis IDs belong to THIS generated proposal. Prior revisions may
    // reuse "h1" for a completely different explanation; never transfer
    // their refutations based on ID alone.
    let mut refuted = Vec::<String>::new();
    let mut attempted_ids = std::collections::HashSet::<String>::new();
    let mut attempted_calls = std::collections::HashSet::<String>::new();
    let mut probe_receipts = Vec::<Value>::new();
    let mut resolved_probes = json!({});
    let mut probe_budget_exhausted = false;
    let mut determinate_observations=0usize;
    // Probes are fully admitted through the same gateway and ledger as normal
    // tools. No shadow executor, no extra model call and no external writes.
    for _ in 0..crate::engine::adaptation::probes::max_executed() {
        if state.is_cancelled(session_id)
            || state.cancellation_generation(session_id) != generation {
            return Err("실행이 중단되었습니다.".into());
        }
        let candidate = crate::engine::adaptation::probes::rank(
            &adaptation, &refuted, &attempted_ids,
        ).into_iter().find(|probe| {
            crate::engine::adaptation::probes::admissible_after_failure(
                probe, tool, arguments,
            ) && !attempted_calls.contains(&format!(
                "{}:{}", probe["tool"].as_str().unwrap_or(""), probe["inputs"]
            ))
        });
        let Some(probe) = candidate else {break};
        let probe_id = probe["id"].as_str().unwrap_or("").to_string();
        let probe_tool = probe["tool"].as_str().unwrap_or("").to_string();
        attempted_ids.insert(probe_id.clone());
        attempted_calls.insert(format!("{probe_tool}:{}",probe["inputs"]));
        if !budget.charge_tool() {
            probe_budget_exhausted=true;
            break;
        }
        *op_seq += 1;
        let inputs = probe["inputs"].clone();
        let call = ToolCall {
            id: format!("adapt-probe-{probe_id}-{}", uuid::Uuid::new_v4()),
            name: probe_tool.clone(),
            arguments: inputs.to_string(),
        };
        let (probe_result, ok, _) = dispatch_tool_call(
            state, tx, session, request, session_id, execution_id,
            &call, inputs, metrics, *op_seq, round, 0, false,
            "adaptation_probe", None, None, None,
        ).await?;
        let observed = if ok && tool_succeeded(&probe_result) {
            let observation = crate::engine::adaptation::probes::observe(
                &probe, &probe_result,
            );
            for id in observation["refutedIds"].as_array().into_iter().flatten() {
                if let Some(id) = id.as_str() {
                    if !refuted.iter().any(|prior|prior.as_str()==id) {
                        refuted.push(id.to_string());
                    }
                }
            }
            if crate::engine::adaptation::probes::determinate(&observation) {
                determinate_observations+=1;
            }
            resolved_probes[probe_id.as_str()] = probe_result.clone();
            observation
        } else {
            // A failed probe is NOT an observation against its hypotheses.
            json!({"probeId":probe_id,"status":"tool_failed",
                "predictions":[],"refutedIds":[]})
        };
        // Full tool results and arguments already live in execution_operation;
        // keep this decision receipt bounded and link to its real call ID.
        probe_receipts.push(json!({
            "probeId":probe_id,"tool":probe_tool,"callId":call.id,
            "expectedInformationBits":probe["expectedInformationBits"],
            "observed":observed["predictions"],
            "refutedIds":observed["refutedIds"],
            "status":if ok && tool_succeeded(&probe_result) {"observed"} else {"tool_failed"},
        }));
        let live=adaptation["hypotheses"].as_array().into_iter().flatten()
            .filter(|hypothesis| hypothesis["id"].as_str()
                .is_some_and(|id|!refuted.iter().any(|prior|prior.as_str()==id)))
            .count();
        if live==0 {
            // All predictions disagreed with the environment. The model's
            // hypothesis set is invalid; do not run an unsupported write.
            adaptation["probeResults"]=json!(probe_receipts);
            return Err("all causal hypotheses were refuted by observed probes; revise hypotheses before changing state".into());
        }
        if live==1 {break;}
    }
    // If the model asked for tests and all actual results were missing,
    // denied or uninterpretable, it has not gained a reason to mutate.
    if !probe_receipts.is_empty() && determinate_observations==0 {
        return Err("diagnostic probes yielded no determinate evidence; do not execute an unsupported intervention".into());
    }
    adaptation["probeResults"]=json!(probe_receipts);
    adaptation["probeBudgetExhausted"]=json!(probe_budget_exhausted);
    let selection_context = json!({
        "before": before_observation,
        "result": before_observation,
        "after": Value::Null,
        // Preconditions may now reference probes using a JSON pointer.
        "probes": resolved_probes,
    });
    let adaptation_algorithm =
        crate::engine::algorithms::choose_for_agent(
            state,
            "adaptation.selection",
            request.agent_id.as_deref(),
            Some(execution_id),
        )
        .unwrap_or(Value::Null);
    let selected = crate::engine::adaptation_loop::choose_intervention_with_policy(
        &adaptation,
        tool,
        arguments,
        false,
        &selection_context,
        &refuted,
        &adaptation_algorithm,
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
    // A native provenance graph plus current-proposal probe refutations
    // constrain mutation admission, without replacing gateway permission.
    let action_certificate = crate::engine::adaptation::trace_ir::action_certificate(
        &failure_trace, &selected,
        adaptation["hypotheses"].as_array().map(Vec::as_slice).unwrap_or(&[]),
        &refuted, crate::engine::effect::is_effectful(&selected_tool, &inputs),
    )?;
    adaptation["actionCertificate"] = action_certificate.clone();
    if state.is_cancelled(session_id)
        || state.cancellation_generation(session_id) != generation {
        return Err("실행이 중단되었습니다.".into());
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
        "probeResults": adaptation["probeResults"],
        "actionCertificate": action_certificate,
    }))
}

/// Restores a durable program/adaptation checkpoint approved by startup wake.
pub(super) fn restore_program_state(
    state: &AppState,
    task_id: &str,
    execution_id: &str,
    program: &Value,
) -> Result<(), String> {
    let source = state
        .store
        .task_executions(task_id)?
        .into_iter()
        .filter(|execution| {
            execution["id"].as_str() != Some(execution_id)
                && execution["taskId"].as_str() == Some(task_id)
                && execution["status"].as_str() == Some("interrupted")
                && execution["programResumeApproved"].as_bool() == Some(true)
                && (execution["programCheckpoint"]["programRevision"].as_str()
                    == program["revision"].as_str()
                    || execution["revisedProgram"]["revision"].as_str()
                        == execution["programCheckpoint"]["programRevision"].as_str())
        })
        .max_by_key(|execution| {
            execution["startedAt"].as_str().unwrap_or("").to_string()
        });
    let Some(source) = source else {
        return Ok(());
    };
    let checkpoint = &source["programCheckpoint"];
    if checkpoint["pendingOperationIds"]
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        return Err("중단된 program 연산의 결과를 확인해야 합니다.".into());
    }
    let source_id = source["id"].as_str().unwrap_or("");
    if let Some(mut target) = state.store.get("execution", execution_id) {
        target["programCheckpoint"] = checkpoint.clone();
        if let Some(adaptations) = source.get("adaptations") {
            target["adaptations"] = adaptations.clone();
        }
        if let Some(adaptation_checkpoint) = source.get("adaptationCheckpoint") {
            target["adaptationCheckpoint"] = adaptation_checkpoint.clone();
        }
        if let Some(last_adaptation) = source.get("lastAdaptation") {
            target["lastAdaptation"] = last_adaptation.clone();
        }
        if let Some(revised) = source.get("revisedProgram") {
            target["revisedProgram"] = revised.clone();
        }
        target["resumedFromExecutionId"] = json!(source_id);
        state.store.put("execution", &target);
    }
    let mut old = source.clone();
    old["programResumeApproved"] = json!(false);
    old["resumedByExecutionId"] = json!(execution_id);
    state.store.put("execution", &old);
    state.store.audit(
        "program.resumed",
        json!({"from": source_id, "to": execution_id, "revision": program["revision"]}),
    );
    Ok(())
}

pub(super) fn tool_is_read_only(name:&str)->bool {
    super::tool_schedule::safe_to_overlap(name)
}

pub(super) fn mark_tool_argument_signal(
    state: &AppState, request: &TurnRequest, execution_id: &str, ok: bool,
) {
    let _ = crate::engine::algorithms::choose_for_agent(
        state,
        "tool.arguments",
        request.agent_id.as_deref(),
        Some(execution_id),
    );
    crate::engine::algorithms::mark_signal(state, execution_id, "tool.arguments", ok);
}

pub(super) fn normalize_tool_arguments(mut arguments: Value, strategy: &str) -> Value {
    if strategy != "normalize-paths" {
        return arguments;
    }
    let Some(map) = arguments.as_object_mut() else {
        return arguments;
    };
    for key in ["path", "cwd", "file", "filename"] {
        let Some(raw) = map.get(key).and_then(Value::as_str) else {
            continue;
        };
        let mut normalized = raw.trim().replace('\\', "/");
        while normalized.contains("//") {
            normalized = normalized.replace("//", "/");
        }
        while normalized.starts_with("./") {
            normalized = normalized[2..].to_string();
        }
        if !normalized.is_empty() {
            map.insert(key.to_string(), Value::String(normalized));
        }
    }
    arguments
}

pub(super) fn save_program_checkpoint(state: &AppState, execution_id: &str, checkpoint: &Value) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["programCheckpoint"] = checkpoint.clone();
        if let Some(saved) = state.store.put("execution", &execution) {
            state.bus.record("execution", &saved);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_program_tool(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    node: &Value,
    resolved: &mut Value,
    checkpoint: &mut Value,
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::budget::BudgetLedger,
    op_seq: &mut u64,
    round: usize,
    persist: bool,
) -> Result<Value, String> {
    let node_id = node["id"].as_str().unwrap_or("");
    if node_id.is_empty() {
        return Err("program call_tool node needs an id".into());
    }
    let tool = node["tool"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("program call_tool needs a tool")?;
    let arguments = crate::engine::program::resolve_inputs(&node["inputs"], resolved)?;
    if !budget.charge_tool() {
        return Err(format!(
            "program tool budget exhausted at node {node_id}"
        ));
    }
    *op_seq += 1;
    let call = ToolCall {
        id: format!("program-{node_id}-{}", uuid::Uuid::new_v4()),
        name: tool.to_string(),
        arguments: arguments.to_string(),
    };
    checkpoint["pendingOperationIds"] = json!([node_id]);
    checkpoint["resolvedInputs"] = resolved.clone();
    if persist {
        save_program_checkpoint(state, execution_id, checkpoint);
    }
    let (result, ok, _content) = dispatch_tool_call(
        state,
        tx,
        session,
        request,
        session_id,
        execution_id,
        &call,
        arguments,
        metrics,
        *op_seq,
        round,
        0,
        false,
        "program",
        None,
        None,
        None,
    )
    .await?;
    checkpoint["pendingOperationIds"] = json!([]);
    // A failed operation requested for revision MUST remain pending.
    // The old code marked it completed even on failure, causing
    // apply_revision_proposal to reject any replacement of the failed node.
    // Other legacy nodes retain their previous terminal completion behavior.
    if program_operation_completed(ok, node["onFailure"].as_str()) {
        mark_program_completed(checkpoint, node_id);
    } else {
        let mut failed=checkpoint["failedOperationIds"]
            .as_array().cloned().unwrap_or_default();
        if !failed.iter().any(|item|item.as_str()==Some(node_id)){
            failed.push(json!(node_id));
        }
        checkpoint["failedOperationIds"]=json!(failed);
        checkpoint["lastFailedCallId"]=json!(call.id);
    }
    if !checkpoint["observationRefs"].is_object() {
        checkpoint["observationRefs"] = json!({});
    }
    checkpoint["observationRefs"][node_id] = json!(
        crate::engine::memory::stable_id(&[&result.to_string()])
    );
    let key = node["saveAs"].as_str().filter(|value| !value.is_empty()).unwrap_or(node_id);
    resolved[key] = result.clone();
    checkpoint["resolvedInputs"] = resolved.clone();
    if persist {
        save_program_checkpoint(state, execution_id, checkpoint);
    }
    Ok(result)
}

/// A failed, revision-requesting node is never a completed external effect.
pub(super) fn program_operation_completed(ok:bool,on_failure:Option<&str>)->bool{
    ok || on_failure!=Some("revise")
}

#[cfg(test)]
mod repair_completion_tests {
    use super::*;
    #[test]
    fn revision_failure_is_pending_but_success_is_completed(){
        assert!(!program_operation_completed(false,Some("revise")));
        assert!(program_operation_completed(true,Some("revise")));
        assert!(program_operation_completed(false,None));
        assert!(program_operation_completed(false,Some("continue")));
    }
}

pub(super) fn mark_program_completed(checkpoint: &mut Value, node_id: &str) {
    if node_id.is_empty() {
        return;
    }
    let mut completed = checkpoint["completedOperationIds"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if !completed.iter().any(|value| value.as_str() == Some(node_id)) {
        completed.push(json!(node_id));
    }
    checkpoint["completedOperationIds"] = json!(completed);
}

pub(super) struct ProgramCtx<'a> {
    pub(super) state: &'a AppState,
    pub(super) tx: &'a UnboundedSender<Value>,
    pub(super) session: &'a Value,
    pub(super) request: &'a TurnRequest,
    pub(super) session_id: &'a str,
    pub(super) execution_id: &'a str,
    pub(super) metrics: &'a std::sync::Mutex<Metrics>,
    pub(super) generation: u64,
}

pub(super) struct ProgramFrame {
    pub(super) resolved: Value,
    pub(super) checkpoint: Value,
    pub(super) budget: super::budget::BudgetLedger,
    pub(super) op_seq: u64,
    pub(super) messages: Vec<Value>,
    pub(super) paused: bool,
    pub(super) failure: Option<Value>,
}

/// Rewrites ids inside a loop body so the same node in two iterations is a
/// different durable operation. `saveAs` keeps the original binding name.
pub(super) fn scope_program_node_ids(node: &Value, prefix: &str) -> Value {
    let mut copy = node.clone();
    let node_type = node["type"].as_str().unwrap_or("");
    if node_type != "observe" {
        if let Some(id) = node["id"].as_str().filter(|value| !value.is_empty()) {
            copy["id"] = json!(format!("{prefix}:{id}"));
            if matches!(node_type, "call_tool" | "delegate") && copy["saveAs"].is_null() {
                copy["saveAs"] = json!(id);
            }
        }
    }
    for key in ["nodes", "then", "else", "body"] {
        if let Some(children) = node[key].as_array() {
            copy[key] = json!(children
                .iter()
                .map(|child| scope_program_node_ids(child, prefix))
                .collect::<Vec<_>>());
        }
    }
    copy
}

/// Executes nodes recursively so Sequence/Branch/Parallel/Loop can contain any
/// supported node. All side effects go through the same tool gateway.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_nodes<'a>(
    ctx: &'a ProgramCtx<'a>,
    nodes: Vec<Value>,
    mut resolved: Value,
    mut checkpoint: Value,
    mut budget: super::budget::BudgetLedger,
    mut op_seq: u64,
    depth: usize,
    persist: bool,
    skip_completed: bool,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<ProgramFrame, String>> + Send + 'a>,
> {
    Box::pin(async move {
        if depth > 16 {
            return Err("program nesting is too deep".into());
        }
        let verification_strict = crate::engine::algorithms::choose_for_agent(
            ctx.state,
            "verification.predicate",
            ctx.request.agent_id.as_deref(),
            Some(ctx.execution_id),
        )
        .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
        .as_deref()
            == Some("strict");
        let mut messages = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            if ctx.state.is_cancelled(ctx.session_id)
                || ctx.state.cancellation_generation(ctx.session_id) != ctx.generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            let node_id = node["id"].as_str().unwrap_or("");
            if skip_completed
                && !node_id.is_empty()
                && crate::engine::program::checkpoint_skip_completed(&checkpoint, node_id)
            {
                continue;
            }
            if persist {
                checkpoint["nextIndex"] = json!(index);
                checkpoint["frameStack"] = json!([{
                    "nodeId": node_id,
                    "index": index,
                }]);
                checkpoint["resolvedInputs"] = resolved.clone();
                save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
            }
            match node["type"].as_str().unwrap_or("") {
                "observe" => {
                    let last = resolved.get("last").cloned().unwrap_or(Value::Null);
                    if !node_id.is_empty() {
                        resolved[node_id] = last;
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "call_tool" => {
                    let result = execute_program_tool(
                        ctx.state,
                        ctx.tx,
                        ctx.session,
                        ctx.request,
                        ctx.session_id,
                        ctx.execution_id,
                        node,
                        &mut resolved,
                        &mut checkpoint,
                        ctx.metrics,
                        &mut budget,
                        &mut op_seq,
                        index,
                        persist,
                    )
                    .await?;
                    let failed = !tool_succeeded(&result);
                    resolved["last"] = result.clone();
                    if failed && node["onFailure"].as_str() == Some("revise") {
                        let failure = json!({
                            "nodeId": node_id,
                            "callId": checkpoint["lastFailedCallId"],
                            "tool": node["tool"],
                            "result": result,
                            "observation": failure_observation(
                                ctx.state,
                                node["tool"].as_str().unwrap_or(""),
                                &node["inputs"],
                                result["error"].as_str().unwrap_or("tool failed"),
                            ),
                        });
                        checkpoint["programFailure"] = failure.clone();
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(
                                ctx.state, ctx.execution_id, &checkpoint,
                            );
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: false, failure: Some(failure),
                        });
                    }
                }
                "branch" => {
                    use crate::engine::program::PredicateVerdict;
                    let chosen = match crate::engine::program::predicate_verdict(
                        &node["predicate"],
                        &resolved,
                    )? {
                        PredicateVerdict::True => {
                            node["then"].as_array().cloned().unwrap_or_default()
                        }
                        PredicateVerdict::False => {
                            node["else"].as_array().cloned().unwrap_or_default()
                        }
                        PredicateVerdict::Unknown if verification_strict => {
                            node["else"].as_array().cloned().unwrap_or_default()
                        }
                        PredicateVerdict::Unknown => {
                            // Do not send an unknown condition to `else`.
                            // Pause and let the model add the observation.
                            let mut checkpoint = checkpoint;
                            checkpoint["status"] = json!("paused");
                            checkpoint["pausedReason"] = json!("predicate_unknown");
                            checkpoint["pausedNodeId"] = json!(node_id);
                            checkpoint["resolvedInputs"] = resolved.clone();
                            if persist {
                                save_program_checkpoint(
                                    ctx.state, ctx.execution_id, &checkpoint,
                                );
                            }
                            messages.push(json!({
                                "role":"system",
                                "content": format!(
                                    "program branch `{node_id}` is Unknown for {}. Add the missing observation or choose another node; the else branch was not taken.",
                                    node["predicate"]
                                )
                            }));
                            return Ok(ProgramFrame {
                                resolved, checkpoint, budget, op_seq, messages,
                                paused: true,
                                failure: None,
                            });
                        }
                    };
                    let frame = execute_nodes(
                        ctx, chosen, resolved, checkpoint, budget, op_seq, depth + 1,
                        persist, true,
                    )
                    .await?;
                    resolved = frame.resolved;
                    checkpoint = frame.checkpoint;
                    budget = frame.budget;
                    op_seq = frame.op_seq;
                    messages.extend(frame.messages);
                    if frame.paused {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved,
                            checkpoint,
                            budget,
                            op_seq,
                            messages,
                            paused: true,
                                failure: None,
                        });
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "sequence" => {
                    let children = node["nodes"].as_array().cloned().unwrap_or_default();
                    let frame = execute_nodes(
                        ctx, children, resolved, checkpoint, budget, op_seq, depth + 1,
                        persist, true,
                    )
                    .await?;
                    resolved = frame.resolved;
                    checkpoint = frame.checkpoint;
                    budget = frame.budget;
                    op_seq = frame.op_seq;
                    messages.extend(frame.messages);
                    if frame.paused {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved,
                            checkpoint,
                            budget,
                            op_seq,
                            messages,
                            paused: true,
                                failure: None,
                        });
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "parallel" => {
                    let children = node["nodes"].as_array().cloned().unwrap_or_default();
                    let child_ids = children
                        .iter()
                        .map(|child| json!(child["id"]))
                        .collect::<Vec<_>>();
                    checkpoint["pendingOperationIds"] = json!(child_ids);
                    checkpoint["parallelMode"] = json!("concurrent");
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if persist {
                        save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                    }
                    let parallel_mode = crate::engine::algorithms::choose_for_agent(
                        ctx.state,
                        "execution.parallelism",
                        ctx.request.agent_id.as_deref(),
                        Some(ctx.execution_id),
                    )
                    .and_then(|algorithm| {
                        algorithm["implementation"].as_str().map(str::to_string)
                    })
                    .unwrap_or_else(|| "concurrent".into());
                    if parallel_mode == "sequential" {
                        let mut paused = false;
                        for child in &children {
                            let frame = execute_nodes(
                                ctx,
                                vec![child.clone()],
                                resolved,
                                checkpoint,
                                budget,
                                op_seq,
                                depth + 1,
                                false,
                                true,
                            )
                            .await?;
                            resolved = frame.resolved;
                            checkpoint = frame.checkpoint;
                            budget = frame.budget;
                            op_seq = frame.op_seq;
                            messages.extend(frame.messages);
                            if frame.paused {
                                paused = true;
                                break;
                            }
                        }
                        checkpoint["pendingOperationIds"] = json!([]);
                        checkpoint["parallelMode"] = json!("sequential");
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if paused {
                            checkpoint["status"] = json!("paused");
                            if persist {
                                save_program_checkpoint(
                                    ctx.state, ctx.execution_id, &checkpoint,
                                );
                            }
                            return Ok(ProgramFrame {
                                resolved, checkpoint, budget, op_seq, messages,
                                paused: true, failure: None,
                            });
                        }
                        mark_program_completed(&mut checkpoint, node_id);
                        crate::engine::algorithms::mark_signal(
                            ctx.state,
                            ctx.execution_id,
                            "execution.parallelism",
                            true,
                        );
                        if persist {
                            save_program_checkpoint(
                                ctx.state, ctx.execution_id, &checkpoint,
                            );
                        }
                        continue;
                    }
                    // Pre-allocate each branch's model/tool budget and a
                    // disjoint operation sequence range BEFORE concurrency.
                    // Merging counters only after join cannot undo paid tools.
                    let shares=budget.parallel_shares(children.len());
                    let mut reserved_until=op_seq;
                    let mut futures=Vec::new();
                    for (child,share) in children.clone().into_iter().zip(shares) {
                        let child_start=reserved_until;
                        reserved_until=reserved_until.saturating_add(share.tool_capacity() as u64);
                        futures.push(execute_nodes(
                            ctx,
                            vec![child],
                            resolved.clone(),
                            checkpoint.clone(),
                            share,
                            child_start,
                            depth + 1,
                            false,
                            true,
                        ));
                    }
                    let results = futures_util::future::join_all(futures).await;
                    let mut paused = false;
                    let mut parallel_last = serde_json::Map::new();
                    for (child, result) in children.iter().zip(results.into_iter()) {
                        let frame = result?;
                        let child_id = child["id"].as_str().unwrap_or("");
                        if let Some(map) = frame.resolved.as_object() {
                            for (key, value) in map {
                                if key != "last" {
                                    resolved[key] = value.clone();
                                }
                            }
                            parallel_last.insert(
                                child_id.to_string(),
                                frame.resolved["last"].clone(),
                            );
                        }
                        if let Some(completed) =
                            frame.checkpoint["completedOperationIds"].as_array()
                        {
                            for id in completed {
                                mark_program_completed(
                                    &mut checkpoint,
                                    id.as_str().unwrap_or(""),
                                );
                            }
                        }
                        mark_program_completed(&mut checkpoint, child_id);
                        budget.merge_usage(&frame.budget);
                        op_seq = op_seq.max(frame.op_seq);
                        messages.extend(frame.messages);
                        paused |= frame.paused;
                    }
                    // Gaps are deliberate: a child need not exhaust its
                    // reservation. The next operation must remain globally
                    // unique and monotonically beyond every reserved range.
                    op_seq=op_seq.max(reserved_until);
                    resolved["last"] = Value::Object(parallel_last);
                    checkpoint["pendingOperationIds"] = json!([]);
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if paused {
                        checkpoint["status"] = json!("paused");
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved,
                            checkpoint,
                            budget,
                            op_seq,
                            messages,
                            paused: true,
                                failure: None,
                        });
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                    crate::engine::algorithms::mark_signal(
                        ctx.state,
                        ctx.execution_id,
                        "execution.parallelism",
                        true,
                    );
                }
                "verify" => {
                    use crate::engine::program::PredicateVerdict;
                    let checks = node["checks"].as_array().cloned().unwrap_or_default();
                    let mut results = Vec::new();
                    let mut ok = true;
                    let mut unknown = false;
                    for check in checks {
                        let verdict = crate::engine::program::predicate_verdict(
                            &check["predicate"],
                            &resolved,
                        )?;
                        match verdict {
                            PredicateVerdict::True => {}
                            PredicateVerdict::False => ok = false,
                            PredicateVerdict::Unknown => {
                                if verification_strict {
                                    ok = false;
                                } else {
                                    unknown = true;
                                }
                            }
                        }
                        results.push(json!({
                            "predicate": check["predicate"],
                            "verdict": verdict.as_str(),
                        }));
                    }
                    resolved["lastVerify"] = json!({
                        "ok": ok && !unknown,
                        "unknown": unknown,
                        "checks": results,
                    });
                    crate::engine::algorithms::mark_signal(
                        ctx.state,
                        ctx.execution_id,
                        "verification.predicate",
                        ok && !unknown,
                    );
                    if !ok {
                        messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "program verify failed: {}",
                                resolved["lastVerify"]
                            )
                        }));
                    } else if unknown {
                        messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "program verify is Unknown (not failed): {}. Add the missing observation before concluding.",
                                resolved["lastVerify"]
                            )
                        }));
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "delegate" => {
                    let mut child = node.clone();
                    child["tool"] = json!("delegate_task");
                    child["inputs"] = json!({
                        "agentId": node["agentId"],
                        "prompt": node["prompt"],
                        "wait": node["wait"],
                        "timeoutSeconds": node["timeoutSeconds"],
                    });
                    let result = execute_program_tool(
                        ctx.state,
                        ctx.tx,
                        ctx.session,
                        ctx.request,
                        ctx.session_id,
                        ctx.execution_id,
                        &child,
                        &mut resolved,
                        &mut checkpoint,
                        ctx.metrics,
                        &mut budget,
                        &mut op_seq,
                        index,
                        persist,
                    )
                    .await?;
                    resolved["last"] = result;
                }
                "loop" => {
                    let body = node["body"].as_array().cloned().unwrap_or_default();
                    let max = node["maxIterations"].as_u64().unwrap_or(3).min(16);
                    if node_id.is_empty() {
                        return Err("loop node needs an id for durable iteration recovery".into());
                    }
                    if !checkpoint["loopProgress"].is_object() {
                        checkpoint["loopProgress"] = json!({});
                    }
                    let mut iterations = checkpoint["loopProgress"][node_id]
                        .as_u64()
                        .unwrap_or(0);
                    while iterations < max {
                        if !crate::engine::program::predicate_holds(
                            &node["predicate"],
                            &resolved,
                        )? {
                            break;
                        }
                        let prefix = format!("{node_id}:{iterations}");
                        let scoped = body
                            .iter()
                            .map(|child| scope_program_node_ids(child, &prefix))
                            .collect::<Vec<_>>();
                        let frame = execute_nodes(
                            ctx,
                            scoped,
                            resolved,
                            checkpoint,
                            budget.clone(),
                            op_seq,
                            depth + 1,
                            persist,
                            true,
                        )
                        .await?;
                        resolved = frame.resolved;
                        checkpoint = frame.checkpoint;
                        budget = frame.budget;
                        op_seq = frame.op_seq;
                        messages.extend(frame.messages);
                        iterations += 1;
                        checkpoint["loopIterations"] = json!(iterations);
                        checkpoint["loopProgress"][node_id] = json!(iterations);
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        if frame.paused {
                            checkpoint["status"] = json!("paused");
                            return Ok(ProgramFrame {
                                resolved,
                                checkpoint,
                                budget,
                                op_seq,
                                messages,
                                paused: true,
                                failure: None,
                            });
                        }
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "model_decision" => {
                    mark_program_completed(&mut checkpoint, node_id);
                    checkpoint["status"] = json!("paused");
                    checkpoint["pausedNodeId"] = json!(node_id);
                    checkpoint["nextIndex"] = json!(index + 1);
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if persist {
                        save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                    }
                    messages.push(json!({
                        "role":"system",
                        "content":"program requested a model decision; control returns to the model loop with saved progress."
                    }));
                    return Ok(ProgramFrame {
                        resolved,
                        checkpoint,
                        budget,
                        op_seq,
                        messages,
                        paused: true,
                                failure: None,
                    });
                }
                other => return Err(format!("unsupported program node type: {other}")),
            }
            checkpoint["resolvedInputs"] = resolved.clone();
            if persist {
                save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
            }
        }
        Ok(ProgramFrame {
            resolved,
            checkpoint,
            budget,
            op_seq,
            messages,
            paused: false,
                                failure: None,
        })
    })
}

pub(super) fn save_revised_program(state: &AppState, execution_id: &str, program: &Value) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["revisedProgram"] = program.clone();
        if let Some(saved) = state.store.put("execution", &execution) {
            state.bus.record("execution", &saved);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn propose_program_revision(
    ctx: &ProgramCtx<'_>,
    program: &Value,
    checkpoint: &Value,
    failure: &Value,
    budget: &mut super::budget::BudgetLedger,
    op_seq: &mut u64,
) -> Result<Option<Value>, String> {
    let provider_id = ctx
        .request
        .provider_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("program revision has no provider")?;
    let model = ctx
        .request
        .model
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("program revision has no model")?;
    if !budget.charge_round() {
        return Ok(None);
    }
    let available = crate::engine::tools::definitions()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let system = "You are the DOMINION program repair planner. A typed execution program failed. Generate up to 3 DISTINCT candidate revisions in one response, not strategy labels. Return JSON: {\"candidateRevisions\":[{\"baseRevision\":\"...\",\"reason\":\"...\",\"evidenceIds\":[\"program-failure\"],\"replacePendingNodes\":[],\"addNodes\":[],\"preflight\":{\"tool\":\"workspace_read\",\"inputs\":{\"path\":\"config.txt\"},\"predicate\":{\"type\":\"equals\",\"from\":\"result\",\"path\":\"/content\",\"value\":\"expected real content\"}}}]}. Each candidate requires a DIFFERENT repair and a read-only preflight with machine-checkable expected observation of current workspace state. The Rust gateway will REALLY execute these tests before choosing. Do not assume tests pass. Cite program-failure, the failed node ID, or an existing checkpoint observationRef as evidence. Preserve completed node IDs and external effects. Available node types are observe, call_tool, branch, sequence, parallel, loop, verify, delegate, model_decision. You MAY return one legacy revision object if meaningful contrasting probes are impossible.";
    // Same HTIR source as native failed-tool repair. Program candidates see
    // the actual failed call's dependency slice, never guessed provenance.
    let trace=crate::engine::adaptation::trace_ir::compile(
        &ctx.state.store.recent_execution_operations(ctx.execution_id,64)?,
        failure["callId"].as_str().unwrap_or(""),
        failure["tool"].as_str().unwrap_or(""),
        &failure["observation"]["arguments"],
    );
    let system = format!(
        "{system} Each candidateRevisions entry MUST also include postflight: {{tool, inputs, predicate}} with a READ-ONLY tool and a machine-checkable result predicate for the state AFTER its proposed revision executes. Postflight is executed through the native gateway; Unknown/False/denied cannot mark repair complete. Candidates with unrelated node replacements are rejected. An added observe/verify node may be composed with a revision of the failed node if both independent preflights pass."
    );
    let prompt = format!(
        "Current program: {program}\nCheckpoint: {checkpoint}\nFailure observation: {failure}\nNative failure provenance: {trace}\nAvailable tools: {available:?}\nReturn only JSON."
    );
    let raw = llm::complete_for_session(
        ctx.state,
        provider_id,
        model,
        &system,
        &prompt,
        ctx.session_id,
    )
    .await?;
    ctx.metrics.lock().unwrap().model_calls += 1;
    let proposal = crate::engine::program::parse_json_object(&raw)
        .ok_or("program revision response is not a JSON object")?;
    let repair_mode = crate::engine::algorithms::choose_for_agent(
        ctx.state,
        "program.repair",
        ctx.request.agent_id.as_deref(),
        Some(ctx.execution_id),
    )
    .and_then(|algorithm| algorithm["parameters"]["mode"].as_str().map(str::to_string))
    .unwrap_or_else(|| "replace".into());
    let mut grounded_failure=failure.clone();
    grounded_failure["trace"]=trace.clone();
    let candidates = crate::engine::adaptation::repair_tournament::prepare(
        &proposal, program, checkpoint, &grounded_failure, &available, &repair_mode,
    )?;
    let preflights = crate::engine::adaptation::repair_tournament::probes(&candidates);
    let mut observations = json!({});
    for probe in preflights {
        if ctx.state.is_cancelled(ctx.session_id)
            || ctx.state.cancellation_generation(ctx.session_id) != ctx.generation {
            return Err("program repair was cancelled before speculative probe".into());
        }
        if !budget.charge_tool() {
            break;
        }
        let Some(id) = probe["id"].as_str() else {continue};
        let Some(tool) = probe["tool"].as_str() else {continue};
        *op_seq += 1;
        let inputs = probe["inputs"].clone();
        let call = ToolCall {
            id: format!("program-repair-probe-{id}-{}", uuid::Uuid::new_v4()),
            name: tool.to_string(),
            arguments: inputs.to_string(),
        };
        let (result, ok, _) = dispatch_tool_call(
            ctx.state, ctx.tx, ctx.session, ctx.request,
            ctx.session_id, ctx.execution_id, &call, inputs, ctx.metrics,
            *op_seq, 0, 0, false, "program_repair_probe",
            None, None, None,
        ).await?;
        // Distinct repair hypotheses may share the same read. One actual
        // gateway call supplies *identical observed bytes* to every alias,
        // preventing both redundant billing and a false Unknown verdict.
        for alias in probe["aliases"].as_array().into_iter().flatten(){
            if let Some(candidate_id)=alias.as_str(){
                observations[candidate_id] = json!({
                    "status":if ok && tool_succeeded(&result){"completed"}else{"failed"},
                    "result":result,
                    "callId":call.id,
                });
            }
        }
    }
    let (mut revised, audit) = crate::engine::adaptation::repair_tournament::select(
        &candidates, &observations,
    )?;
    // Preserve the contrastive decision and native probe call IDs inside the
    // revised program's existing durable execution record. Raw probe outputs
    // remain in execution_operation, never in the audit summary.
    let called=observations.as_object().into_iter().flat_map(|map|map.iter())
        .map(|(id,value)|json!({"candidateId":id,"callId":value["callId"],
            "status":value["status"]})).collect::<Vec<_>>();
    revised["repairTournament"] = json!({
        "decision":audit,"probeReceipts":called,
        "failureTraceFocus":trace["focus"],
    });
    Ok(Some(revised))
}

pub(super) async fn run_program(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    program: &Value,
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::budget::BudgetLedger,
    op_seq: &mut u64,
    generation: u64,
) -> Result<Vec<Value>, String> {
    let existing = state
        .store
        .get("execution", execution_id)
        .and_then(|execution| execution.get("programCheckpoint").cloned());
    let mut checkpoint = existing
        .filter(|checkpoint| {
            checkpoint["programRevision"].as_str() == program["revision"].as_str()
                && checkpoint["status"].as_str() != Some("completed")
        })
        .unwrap_or_else(|| crate::engine::program::new_checkpoint(program));
    let mut current_program = program.clone();
    if checkpoint["programRevision"].as_str() != program["revision"].as_str() {
        match state
            .store
            .get("execution", execution_id)
            .and_then(|execution| execution.get("revisedProgram").cloned())
            .filter(|revised| {
                revised["revision"].as_str()
                    == checkpoint["programRevision"].as_str()
            }) {
            Some(revised) => current_program = revised,
            None => {
                checkpoint = crate::engine::program::new_checkpoint(program);
            }
        }
    }
    let mut resolved = checkpoint["resolvedInputs"].clone();
    let start = checkpoint["nextIndex"].as_u64().unwrap_or(0) as usize;
    let mut remaining = current_program["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .skip(start)
        .collect::<Vec<_>>();
    checkpoint["status"] = json!("running");
    let ctx = ProgramCtx {
        state,
        tx,
        session,
        request,
        session_id,
        execution_id,
        metrics,
        generation,
    };
    let mut budget_local = budget.clone();
    let mut op_seq_local = *op_seq;
    let mut messages = Vec::new();
    let mut revisions = 0usize;
    let max_revisions = crate::engine::algorithms::active(state, "program.repair")
        .and_then(|algorithm| algorithm["parameters"]["maxRevisions"].as_u64())
        .unwrap_or(2) as usize;
    loop {
        let frame = execute_nodes(
            &ctx,
            remaining,
            resolved,
            checkpoint,
            budget_local,
            op_seq_local,
            0,
            true,
            true,
        )
        .await?;
        budget_local = frame.budget;
        op_seq_local = frame.op_seq;
        resolved = frame.resolved;
        checkpoint = frame.checkpoint;
        messages.extend(frame.messages);
        let Some(failure) = frame.failure else {
            break;
        };
        if revisions >= max_revisions {
            messages.push(json!({
                "role":"system",
                "content":"program revision limit reached; stop and report the observed failure."
            }));
            break;
        }
        revisions += 1;
        match propose_program_revision(
            &ctx,
            &current_program,
            &checkpoint,
            &failure,
            &mut budget_local,
            &mut op_seq_local,
        )
        .await
        {
            Ok(Some(revised)) => {
                let mut history = checkpoint["revisionHistory"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                history.push(json!({
                    "from": current_program["revision"],
                    "to": revised["revision"],
                    "reason": revised["appliedProposal"]["reason"],
                    "evidenceIds": revised["appliedProposal"]["evidenceIds"],
                }));
                checkpoint["revisionHistory"] = json!(history);
                checkpoint["programRevision"] = revised["revision"].clone();
                checkpoint["programFailure"] = Value::Null;
                checkpoint["nextIndex"] = json!(0);
                checkpoint["status"] = json!("running");
                save_revised_program(state, execution_id, &revised);
                save_program_checkpoint(state, execution_id, &checkpoint);
                crate::engine::algorithms::mark_signal(
                    state, execution_id, "program.repair", true,
                );
                messages.push(json!({
                    "role":"system",
                    "content": format!(
                        "program revised: {} -> {} (reason: {}). Completed effects are preserved; only pending nodes continue.",
                        current_program["revision"], revised["revision"],
                        revised["appliedProposal"]["reason"]
                    )
                }));
                current_program = revised;
                remaining = current_program["nodes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                continue;
            }
            Ok(None) => break,
            Err(error) => {
                crate::engine::algorithms::mark_signal(
                    state, execution_id, "program.repair", false,
                );
                record_adaptation_error(
                    state,
                    execution_id,
                    &format!("program revision rejected: {error}"),
                );
                messages.push(json!({
                    "role":"system",
                    "content": format!(
                        "program revision was rejected: {error}. The original failure observation is preserved."
                    )
                }));
                break;
            }
        }
    }
    *budget = budget_local;
    *op_seq = op_seq_local;
    let node_count = current_program["nodes"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    checkpoint["resolvedInputs"] = resolved;
    if checkpoint["status"].as_str() == Some("paused") {
        save_program_checkpoint(state, execution_id, &checkpoint);
        return Ok(messages);
    }
    if !checkpoint["programFailure"].is_null() {
        // The program could not be repaired. Do not pretend it completed:
        // preserve the failure and require review/model continuation.
        checkpoint["status"] = json!("needs_review");
        save_program_checkpoint(state, execution_id, &checkpoint);
        messages.push(json!({
            "role":"system",
            "content": format!(
                "program stopped with an unresolved failure at {}: {}",
                checkpoint["programFailure"]["nodeId"],
                checkpoint["programFailure"]["result"]
            )
        }));
        return Ok(messages);
    }
    checkpoint["nextIndex"] = json!(node_count);
    checkpoint["status"] = json!("completed");
    save_program_checkpoint(state, execution_id, &checkpoint);
    messages.push(json!({
        "role": "system",
        "content": format!(
            "A durable execution program (revision {}) completed through the tool gateway. Use its real observations; do not repeat successful actions.",
            program["revision"].as_str().unwrap_or("")
        )
    }));
    Ok(messages)
}

