//! Durable program execution, loops and interrupted task recovery.
use super::*;

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
    let source_id = source["id"].as_str()
        .ok_or("interrupted source execution has no identity")?;
    let mut target = state.store.get("execution", execution_id)
        .ok_or("target execution was deleted before resumption")?;
    target["programCheckpoint"] = checkpoint.clone();
    for field in ["adaptations", "adaptationCheckpoint", "lastAdaptation", "revisedProgram"] {
        if let Some(value) = source.get(field) {
            target[field] = value.clone();
        }
    }
    target["resumedFromExecutionId"] = json!(source_id);
    let mut old = source.clone();
    old["programResumeApproved"] = json!(false);
    old["resumedByExecutionId"] = json!(execution_id);
    // These two execution records form one resumption. Persisting only one
    // could let two workers consume the same old checkpoint or lose its IR.
    let records = [
        ("execution".to_string(), target),
        ("execution".to_string(), old),
    ];
    if !state.store.transaction_if_fields(
        &[
            ("execution", source_id, "status", json!("interrupted")),
            ("execution", source_id, "programResumeApproved", json!(true)),
            ("execution", execution_id, "status", json!("running")),
        ],
        &records,
    )? {
        return Err("program recovery cancelled: source approval or target ownership changed".into());
    }
    for (_, record) in records {
        state.bus.record("execution", &record);
    }
    state.store.audit(
        "program.resumed",
        json!({"from": source_id, "to": execution_id, "revision": program["revision"]}),
    );
    Ok(())
}


pub(super) fn save_program_checkpoint(
    state: &AppState, execution_id: &str, checkpoint: &Value,
) -> Result<(), String> {
    let mut execution = state.store.get("execution", execution_id)
        .ok_or("program checkpoint has no durable execution record")?;
    execution["programCheckpoint"] = checkpoint.clone();
    let saved = state.store.put("execution", &execution)
        .ok_or("program checkpoint write failed; refusing tool dispatch")?;
    state.bus.record("execution", &saved);
    Ok(())
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
    budget: &mut super::super::budget::BudgetLedger,
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
        save_program_checkpoint(state, execution_id, checkpoint)?;
    }
    let (result, _ok, _content) = dispatch_tool_call(
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
    // Any operation that can mutate the workspace or external world makes a
    // previously passing verification stale, including an attempted effect
    // whose outcome needs inspection. Read-only observations do not.
    if !dominion_agent_sdk::runtime::builtin_is_read_only(tool) {
        checkpoint["mutationEpoch"] = json!(
            checkpoint["mutationEpoch"].as_u64().unwrap_or(0).saturating_add(1)
        );
        if resolved["lastVerify"]["ok"] == true {
            resolved["lastVerify"] = Value::Null;
            checkpoint["staleVerification"] = json!(true);
            checkpoint["verificationEpoch"] = Value::Null;
        }
    }
    mark_program_completed(checkpoint, node_id);
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
        save_program_checkpoint(state, execution_id, checkpoint)?;
    }
    Ok(result)
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
    state: &'a AppState,
    tx: &'a UnboundedSender<Value>,
    session: &'a Value,
    request: &'a TurnRequest,
    session_id: &'a str,
    execution_id: &'a str,
    metrics: &'a std::sync::Mutex<Metrics>,
    generation: u64,
}

pub(super) struct ProgramFrame {
    resolved: Value,
    checkpoint: Value,
    budget: super::super::budget::BudgetLedger,
    op_seq: u64,
    messages: Vec<Value>,
    paused: bool,
    failure: Option<Value>,
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

/// Conservative join of parallel evidence. A verifier running concurrently
/// with an effect cannot prove a post-effect state: only a subsequent,
/// ordered verification may clear the stale flag. All writes from child
/// branches contribute to the parent's mutation epoch.
pub(super) fn settle_parallel_verification(
    checkpoint: &mut Value,
    resolved: &mut Value,
    baseline_epoch: u64,
    observed_mutations: u64,
    branch_stale: bool,
) {
    let epoch = baseline_epoch.saturating_add(observed_mutations);
    checkpoint["mutationEpoch"] = json!(epoch);
    if branch_stale || (observed_mutations > 0 && resolved["lastVerify"]["ok"] == true) {
        checkpoint["staleVerification"] = json!(true);
        checkpoint["verificationEpoch"] = Value::Null;
        resolved["lastVerify"] = Value::Null;
    } else if resolved["lastVerify"]["ok"] == true {
        checkpoint["staleVerification"] = json!(false);
        checkpoint["verificationEpoch"] = json!(epoch);
    }
}

/// Executes nodes recursively so Sequence/Branch/Parallel/Loop can contain any
/// supported node. All side effects go through the same tool gateway.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_nodes<'a>(
    ctx: &'a ProgramCtx<'a>,
    nodes: Vec<Value>,
    mut resolved: Value,
    mut checkpoint: Value,
    mut budget: super::super::budget::BudgetLedger,
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
                save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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
                            )?;
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
                                )?;
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
                    if let Some(failure) = frame.failure {
                        checkpoint["programFailure"] = failure.clone();
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: false, failure: Some(failure),
                        });
                    }
                    if frame.paused {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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
                    if let Some(failure) = frame.failure {
                        checkpoint["programFailure"] = failure.clone();
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: false, failure: Some(failure),
                        });
                    }
                    if frame.paused {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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
                        save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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
                            // A failed verifier inside a sequence/parallel
                            // branch must propagate to the parent. Previously
                            // the parent marked the whole parallel step done.
                            if let Some(failure) = frame.failure {
                                checkpoint["pendingOperationIds"] = json!([]);
                                checkpoint["programFailure"] = failure.clone();
                                checkpoint["resolvedInputs"] = resolved.clone();
                                if persist {
                                    save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
                                }
                                return Ok(ProgramFrame {
                                    resolved, checkpoint, budget, op_seq, messages,
                                    paused: false, failure: Some(failure),
                                });
                            }
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
                                )?;
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
                            )?;
                        }
                        continue;
                    }
                    let baseline_mutation_epoch = checkpoint["mutationEpoch"].as_u64().unwrap_or(0);
                    let base = budget.branch();
                    let mut futures = Vec::new();
                    for child in children.clone() {
                        futures.push(execute_nodes(
                            ctx,
                            vec![child],
                            resolved.clone(),
                            checkpoint.clone(),
                            base.branch(),
                            op_seq,
                            depth + 1,
                            false,
                            true,
                        ));
                    }
                    let results = futures_util::future::join_all(futures).await;
                    let mut paused = false;
                    let mut first_failure: Option<Value> = None;
                    let mut total_mutations = 0u64;
                    let mut branch_stale = false;
                    let mut parallel_last = serde_json::Map::new();
                    for (child, result) in children.iter().zip(results.into_iter()) {
                        let frame = result?;
                        total_mutations = total_mutations.saturating_add(
                            frame.checkpoint["mutationEpoch"].as_u64().unwrap_or(baseline_mutation_epoch)
                                .saturating_sub(baseline_mutation_epoch)
                        );
                        branch_stale |= frame.checkpoint["staleVerification"] == true;
                        if first_failure.is_none() {
                            first_failure = frame.failure.clone();
                        }
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
                        if frame.failure.is_none() && !frame.paused {
                            mark_program_completed(&mut checkpoint, child_id);
                        }
                        budget.merge_usage(&frame.budget);
                        op_seq = op_seq.max(frame.op_seq);
                        messages.extend(frame.messages);
                        paused |= frame.paused;
                    }
                    resolved["last"] = Value::Object(parallel_last);
                    checkpoint["pendingOperationIds"] = json!([]);
                    settle_parallel_verification(
                        &mut checkpoint, &mut resolved, baseline_mutation_epoch,
                        total_mutations, branch_stale,
                    );
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if let Some(failure) = first_failure {
                        checkpoint["programFailure"] = failure.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: false, failure: Some(failure),
                        });
                    }
                    if paused {
                        checkpoint["status"] = json!("paused");
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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
                    let checks = node["checks"].as_array().ok_or("verify.checks missing")?;
                    let outcome = crate::engine::program::evaluate_verification_checks(
                        checks, &resolved,
                    )?;
                    let failed = outcome["failed"] == true;
                    let unknown = outcome["unknown"] == true;
                    resolved["lastVerify"] = outcome;
                    // A failed or unknown verification is NOT a completed
                    // operation. Earlier code marked it complete, allowing an
                    // unverified program to reach a successful terminal state.
                    if failed {
                        crate::engine::algorithms::mark_signal(
                            ctx.state, ctx.execution_id, "verification.predicate", false,
                        );
                        let failure = json!({
                            "nodeId": node_id,
                            "kind": "verification_failed",
                            "tool": "verify",
                            "result": resolved["lastVerify"],
                            "observation": {
                                "kind": "predicate_refuted",
                                "verdicts": resolved["lastVerify"]["checks"],
                            },
                        });
                        checkpoint["programFailure"] = failure.clone();
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
                        }
                        messages.push(json!({
                            "role": "system",
                            "content": format!(
                                "program verification refuted at node {node_id}: {}. Repair pending work or stop.",
                                resolved["lastVerify"]
                            )
                        }));
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: false, failure: Some(failure),
                        });
                    }
                    if unknown {
                        checkpoint["status"] = json!("paused");
                        checkpoint["pausedReason"] = json!(
                            if verification_strict {
                                "verification_unknown_strict"
                            } else {
                                "verification_unknown"
                            }
                        );
                        checkpoint["pausedNodeId"] = json!(node_id);
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
                        }
                        messages.push(json!({
                            "role": "system",
                            "content": format!(
                                "program verification has missing evidence at node {node_id}: {}. No downstream tool was admitted.",
                                resolved["lastVerify"]
                            )
                        }));
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: true, failure: None,
                        });
                    }
                    // An in-loop pass is not a final quality signal: a later
                    // mutation can invalidate it. Credit is given only at
                    // terminal settlement with the same mutation epoch.
                    checkpoint["staleVerification"] = json!(false);
                    checkpoint["verificationEpoch"] = checkpoint["mutationEpoch"].clone();
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
                        if let Some(failure) = frame.failure {
                            // The failed iteration is not complete. Preserve
                            // each settled child receipt; do not advance the
                            // loop counter or declare the parent completed.
                            checkpoint["programFailure"] = failure.clone();
                            checkpoint["resolvedInputs"] = resolved.clone();
                            if persist {
                                save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
                            }
                            return Ok(ProgramFrame {
                                resolved, checkpoint, budget, op_seq, messages,
                                paused: false, failure: Some(failure),
                            });
                        }
                        iterations += 1;
                        checkpoint["loopIterations"] = json!(iterations);
                        checkpoint["loopProgress"][node_id] = json!(iterations);
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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
                        save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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
                save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint)?;
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

/// Persist the executable revision together with its checkpoint in one
/// SQLite record mutation. A crash may not leave a new checkpoint pointing
/// to the OLD executable, nor an edited executable with a stale cursor.
pub(super) fn commit_revised_program(
    state: &AppState, execution_id: &str,
    program: &Value, checkpoint: &Value,
) -> Result<(), String> {
    if program["revision"] != checkpoint["programRevision"] {
        return Err("revision and checkpoint must be stored together".into());
    }
    let mut execution = state.store.get("execution", execution_id)
        .ok_or("execution vanished while persisting program revision")?;
    execution["revisedProgram"] = program.clone();
    execution["programCheckpoint"] = checkpoint.clone();
    let saved = state.store.put("execution", &execution)
        .ok_or("failed to durably store revised program and cursor")?;
    state.bus.record("execution", &saved);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn propose_program_revision(
    ctx: &ProgramCtx<'_>,
    program: &Value,
    checkpoint: &Value,
    failure: &Value,
    budget: &mut super::super::budget::BudgetLedger,
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
    let system = "You are DOMINION's evidence-bounded program repair proposer. An already executed typed Program IR failed. Return one JSON object with either the legacy single proposal or {\"candidates\":[proposal1,proposal2,proposal3]}. Each proposal must contain {\"baseRevision\":\"...\",\"reason\":\"...\",\"evidenceIds\":[],\"replacePendingNodes\":[],\"addNodes\":[],\"dependencyChanges\":[],\"expectedObservations\":[]}. Keep exactly the current revision and only change PENDING operations; never retry settled external effects. cite evidenceIds only if actually present in checkpoint.observationRefs or current failure.nodeId. Candidate plans may use observe, call_tool, branch, sequence, parallel, loop, verify, delegate and model_decision. Call_tool needs id,tool,inputs. For verification_failed, place a recovery call_tool BEFORE the original failed verifier by setting addNodes[].insertBefore to the failing nodeId; never weaken, replace or hide original verification checks. Mere appends after a refuted gate cannot repair it. Prefer independent repair routes rather than cosmetic variants. The harness will check tool permissions, dependency preservation, observed evidence and non-dominated side-effect risk; it may decline all repairs. Output JSON only. No invented success claims.";
    let prompt = format!(
        "Current program: {program}\nCheckpoint: {checkpoint}\nFailure observation: {failure}\nAvailable tools: {available:?}\nReturn only the JSON proposal."
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
    let revised = crate::engine::program::adjudicate_revision_proposals(
        program, checkpoint, &proposal, &available, 64, &repair_mode,
    )?;
    Ok(Some(revised))
}

/// Restore the exact durable executable revision before touching any tool.
/// A revision that exists only in the checkpoint but not the persisted
/// executable IR is an ambiguous crash state; never replay the base program.
fn restore_program(
    original: &Value,
    prior_checkpoint: Option<Value>,
    saved_revision: Option<Value>,
) -> Result<(Value, Value), String> {
    let mut checkpoint = prior_checkpoint
        .filter(|prior| prior["status"] != "completed")
        .unwrap_or_else(|| crate::engine::program::new_checkpoint(original));
    if checkpoint["pendingOperationIds"].as_array()
        .is_some_and(|ids| !ids.is_empty())
    {
        return Err("program recovery stopped: an in-flight effect has no confirmed result".into());
    }
    let revision = checkpoint["programRevision"].as_str()
        .ok_or("program recovery stopped: checkpoint revision missing")?;
    let current = if original["revision"].as_str() == Some(revision) {
        original.clone()
    } else {
        let revised = saved_revision
            .ok_or("program recovery stopped: missing persisted revised program")?;
        if revised["revision"].as_str() != Some(revision) {
            return Err("program recovery stopped: persisted revision does not match checkpoint".into());
        }
        crate::engine::program::validate(&revised)?;
        revised
    };
    if !checkpoint["resolvedInputs"].is_object() {
        return Err("program recovery stopped: resolved inputs are invalid".into());
    }
    let total = current["nodes"].as_array().ok_or("program nodes missing")?.len();
    if checkpoint["nextIndex"].as_u64().is_none_or(|idx| idx > total as u64) {
        return Err("program recovery stopped: checkpoint position exceeds program".into());
    }
    if !checkpoint["mutationEpoch"].is_u64() {
        checkpoint["mutationEpoch"] = json!(0u64);
    }
    Ok((checkpoint, current))
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
    budget: &mut super::super::budget::BudgetLedger,
    op_seq: &mut u64,
    generation: u64,
) -> Result<Vec<Value>, String> {
    let saved_execution = state.store.get("execution", execution_id);
    let previous_checkpoint = saved_execution.as_ref()
        .and_then(|execution| execution.get("programCheckpoint").cloned());
    let persisted_revision = saved_execution.as_ref()
        .and_then(|execution| execution.get("revisedProgram").cloned());
    let (mut checkpoint, mut current_program) = restore_program(
        program, previous_checkpoint, persisted_revision,
    )?;
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
                    "candidateAdjudication": revised["revisionAdjudication"],
                }));
                checkpoint["revisionHistory"] = json!(history);
                checkpoint["programRevision"] = revised["revision"].clone();
                checkpoint["programFailure"] = Value::Null;
                checkpoint["nextIndex"] = json!(0);
                checkpoint["status"] = json!("running");
                commit_revised_program(state, execution_id, &revised, &checkpoint)?;
                // A syntactically admitted revision is NOT evidence of repair.
                // Credit is assigned only after the revised program settles.
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
    let had_revision = checkpoint["revisionHistory"].as_array()
        .is_some_and(|history| !history.is_empty());
    if checkpoint["status"].as_str() == Some("paused") {
        // An inconclusive verification must not increase the success rate.
        if had_revision {
            crate::engine::algorithms::mark_unrated(
                state, execution_id, "program.repair",
                "revised program paused: no verified repair outcome",
            );
        }
        save_program_checkpoint(state, execution_id, &checkpoint)?;
        return Ok(messages);
    }
    if !checkpoint["programFailure"].is_null() {
        // The program could not be repaired. Do not pretend it completed:
        // preserve the failure and require review/model continuation.
        checkpoint["status"] = json!("needs_review");
        if had_revision {
            crate::engine::algorithms::mark_signal(
                state, execution_id, "program.repair", false,
            );
        }
        save_program_checkpoint(state, execution_id, &checkpoint)?;
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
    // A verification before a later mutation is not current evidence.
    // Require a NEW passing check, rather than reusing a completed verify
    // node whose observation predated the latest side effect.
    if checkpoint["staleVerification"] == true {
        checkpoint["status"] = json!("needs_review");
        checkpoint["pausedReason"] = json!("verification_stale_after_mutation");
        if had_revision {
            crate::engine::algorithms::mark_unrated(
                state, execution_id, "program.repair",
                "revision ended with verification invalidated by a later mutation",
            );
        }
        save_program_checkpoint(state, execution_id, &checkpoint)?;
        messages.push(json!({
            "role":"system",
            "content":"program cannot complete: a later mutation invalidated the previous verification. Add an observation and verify again."
        }));
        return Ok(messages);
    }
    checkpoint["nextIndex"] = json!(node_count);
    checkpoint["status"] = json!("completed");
    let fresh_verification = checkpoint["resolvedInputs"]["lastVerify"]["ok"] == true
        && checkpoint["verificationEpoch"].as_u64().is_some()
        && checkpoint["verificationEpoch"].as_u64()
            == checkpoint["mutationEpoch"].as_u64();
    if fresh_verification {
        crate::engine::algorithms::mark_signal(
            state, execution_id, "verification.predicate", true,
        );
    }
    if had_revision {
        // Program completion and owner acceptance are different facts. Only a
        // predicate check that actually ran and passed is positive evidence
        // for the repair algorithm; unchecked repairs remain unrated.
        if fresh_verification {
            crate::engine::algorithms::mark_signal(
                state, execution_id, "program.repair", true,
            );
        } else {
            crate::engine::algorithms::mark_unrated(
                state, execution_id, "program.repair",
                "revised program completed without a passing verification predicate",
            );
        }
    }
    save_program_checkpoint(state, execution_id, &checkpoint)?;
    messages.push(json!({
        "role": "system",
        "content": format!(
            "A durable execution program (revision {}) completed through the tool gateway. Use its real observations; do not repeat successful actions.",
            program["revision"].as_str().unwrap_or("")
        )
    }));
    Ok(messages)
}



#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn durable_revised_program_resumes_its_own_ir_and_skips_committed_effects() {
        let base = json!({"revision":"base","nodes":[
            {"type":"observe","id":"done"},
            {"type":"observe","id":"pending"}
        ]});
        let revised = json!({"revision":"base:rev:a","nodes":[
            {"type":"observe","id":"done"},
            {"type":"observe","id":"replacement"}
        ]});
        let mut checkpoint = crate::engine::program::new_checkpoint(&base);
        checkpoint["programRevision"] = json!("base:rev:a");
        checkpoint["completedOperationIds"] = json!(["done"]);
        checkpoint["nextIndex"] = json!(1);
        let (saved, executable) = restore_program(&base,Some(checkpoint),Some(revised)).unwrap();
        assert_eq!(executable["nodes"][1]["id"],"replacement");
        assert_eq!(saved["completedOperationIds"],json!(["done"]));
        assert_eq!(saved["nextIndex"],1);
    }
    #[test]
    fn missing_or_conflicting_revision_cannot_replay_original_completed_tool() {
        let base=json!({"revision":"base","nodes":[
            {"type":"call_tool","id":"write","tool":"workspace_write",
                "inputs":{"path":"file","content":"x"}}
        ]});
        let mut cp=crate::engine::program::new_checkpoint(&base);
        cp["programRevision"]=json!("base:rev:committed");
        cp["completedOperationIds"]=json!(["write"]);
        assert!(restore_program(&base,Some(cp.clone()),None)
            .unwrap_err().contains("missing persisted revised"));
        let mismatched=json!({"revision":"different","nodes":[
            {"type":"observe","id":"replaced"}
        ]});
        assert!(restore_program(&base,Some(cp),Some(mismatched))
            .unwrap_err().contains("does not match checkpoint"));
    }
    #[test]
    fn unfinished_effect_checkpoint_fails_closed_without_duplicate_dispatch() {
        let base=json!({"revision":"base","nodes":[
            {"type":"call_tool","id":"write","tool":"workspace_write",
             "inputs":{"path":"output","content":"once"}}
        ]});
        let mut cp=crate::engine::program::new_checkpoint(&base);
        cp["pendingOperationIds"]=json!(["write"]);
        assert!(restore_program(&base,Some(cp),None)
            .unwrap_err().contains("in-flight effect"));
    }
    #[test]
    fn interrupted_run_transfers_checkpoint_and_revision_as_one_transaction() {
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "sub3-program-resume-{}",uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        let original=json!({"revision":"base","nodes":[
            {"type":"observe","id":"old"}
        ]});
        let revised=json!({"revision":"base:rev:a","nodes":[
            {"type":"observe","id":"new"}
        ]});
        let mut old_cp=crate::engine::program::new_checkpoint(&original);
        old_cp["programRevision"]=json!("base:rev:a");
        old_cp["completedOperationIds"]=json!(["old"]);
        assert!(save_program_checkpoint(&state,"missing",&old_cp).is_err(),
            "missing execution cannot authorize unjournaled tool effects");
        let source=json!({"id":"source","taskId":"task","status":"interrupted",
            "programResumeApproved":true,"startedAt":"2026-11-17T19:00:00Z",
            "programCheckpoint":old_cp,"revisedProgram":revised});
        state.store.put("execution",&source).unwrap();
        assert!(restore_program_state(&state,"task","target",&original).is_err());
        assert_eq!(state.store.get("execution","source").unwrap()["programResumeApproved"],true);
        state.store.put("execution",&json!({
            "id":"target","taskId":"task","status":"running"
        })).unwrap();
        restore_program_state(&state,"task","target",&original).unwrap();
        assert!(restore_program_state(&state,"task","another_target",&original).is_ok(),
            "already consumed source cannot be claimed twice");
        let saved=state.store.get("execution","target").unwrap();
        let spent=state.store.get("execution","source").unwrap();
        assert_eq!(saved["programCheckpoint"]["programRevision"],"base:rev:a");
        assert_eq!(saved["revisedProgram"]["nodes"][0]["id"],"new");
        assert_eq!(spent["programResumeApproved"],false);
        assert_eq!(spent["resumedByExecutionId"],"target");
        let (restored_cp,restored_ir)=restore_program(
            &original,Some(saved["programCheckpoint"].clone()),
            Some(saved["revisedProgram"].clone())
        ).unwrap();
        assert_eq!(restored_ir["nodes"][0]["id"],"new");
        assert_eq!(restored_cp["completedOperationIds"],json!(["old"]));
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn previously_completed_original_checkpoint_starts_fresh_without_stale_revision() {
        let base=json!({"revision":"base","nodes":[
            {"type":"observe","id":"safe"}
        ]});
        let mut previous=crate::engine::program::new_checkpoint(&base);
        previous["status"]=json!("completed");
        previous["programRevision"]=json!("obsolete");
        let (checkpoint,restored)=restore_program(&base,Some(previous),None).unwrap();
        assert_eq!(checkpoint["programRevision"],"base");
        assert_eq!(restored["revision"],"base");
    }
}

#[cfg(test)]
mod evidence_join_tests {
    use super::*;

    #[test]
    fn concurrent_write_invalidates_parallel_verifier() {
        let mut checkpoint = json!({"mutationEpoch":4,"verificationEpoch":4,
            "staleVerification":false});
        let mut resolved = json!({"lastVerify":{"ok":true,"checks":[{"verdict":"true"}]}});
        settle_parallel_verification(&mut checkpoint,&mut resolved,4,1,false);
        assert_eq!(checkpoint["mutationEpoch"], 5);
        assert_eq!(checkpoint["staleVerification"], true);
        assert!(checkpoint["verificationEpoch"].is_null());
        assert!(resolved["lastVerify"].is_null());
    }

    #[test]
    fn read_only_join_preserves_valid_evidence_and_stale_branch_blocks_it() {
        let mut checkpoint = json!({"mutationEpoch":3});
        let mut resolved = json!({"lastVerify":{"ok":true}});
        settle_parallel_verification(&mut checkpoint,&mut resolved,3,0,false);
        assert_eq!(checkpoint["staleVerification"], false);
        assert_eq!(checkpoint["verificationEpoch"], 3);
        settle_parallel_verification(&mut checkpoint,&mut resolved,3,0,true);
        assert_eq!(checkpoint["staleVerification"], true);
        assert!(resolved["lastVerify"].is_null());
    }
}
