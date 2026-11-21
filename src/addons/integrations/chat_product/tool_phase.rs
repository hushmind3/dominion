//! MainTurn tool execution, observation and receipt reconciliation.
use super::*;

pub(super) fn begin_tools<'a>(
        turn: &'a mut MainTurn<'_>,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let mut completion = turn.completion.take().ok_or("모델 응답이 없습니다.")?;

            let mut seen_ids = std::collections::HashSet::new();
            for call in completion.tool_calls.iter_mut() {
                if call.id.trim().is_empty() || !seen_ids.insert(call.id.clone()) {
                    call.id = uuid::Uuid::new_v4().to_string();
                    seen_ids.insert(call.id.clone());
                }
            }
            let tool_calls_json = completion
                .tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": { "name": call.name, "arguments": call.arguments }
                    })
                })
                .collect::<Vec<_>>();
            turn.messages.push(json!({
                "role": "assistant",
                "content": completion.content,
                "reasoning_content": completion.reasoning,
                "tool_calls": tool_calls_json,
            }));
            // Persist the tool-call chain so a restart keeps the exact context the
            // model saw instead of only the final answer.
            let saved = turn
                .state
                .store
                .write_message(&json!({
                    "session_id": turn.session_id,
                    "role": "assistant",
                    "content": completion.content,
                    "reasoning": completion.reasoning,
                    "agent_id": turn.request.agent_id,
                    "tool_calls": tool_calls_json,
                    "metadata": turn.metadata,
                }))
                .ok_or("도구 호출 메시지를 저장할 수 없습니다.")?;
            turn.state.bus.message(&saved);

            let ordering = crate::addons::runtime_hooks::choose_for_agent(
                turn.state,
                "execution.ordering",
                turn.request.agent_id.as_deref(),
                Some(turn.execution_id),
            );
            let ordering_kernel = ordering
                .as_ref()
                .and_then(|algorithm| algorithm["implementation"].as_str())
                .unwrap_or("as-written");
            // Native provider calls retain their declared order. Effects are barriers;
            // the shared Rust kernel only batches adjacent independent observations.
            turn.round_calls = completion.tool_calls.clone();
            if ordering_kernel == "read-first"
                && turn
                    .round_calls
                    .iter()
                    .any(|call| !tool_is_read_only(&call.name))
            {
                crate::addons::runtime_hooks::mark_unrated(
                    turn.state,
                    turn.execution_id,
                    "execution.ordering",
                    "effect ordering barrier preserved",
                );
            }
            let combination = crate::addons::runtime_hooks::choose_for_agent(
                turn.state,
                "tool.combination",
                turn.request.agent_id.as_deref(),
                Some(turn.execution_id),
            );
            turn.parallel_reads = combination
                .as_ref()
                .and_then(|algorithm| algorithm["implementation"].as_str())
                == Some("parallel-reads");
            turn.dispatched = std::collections::HashMap::new();
            turn.ordering_used = false;
            turn.ordering_ok = true;

            Ok(turn.round_calls.len())
        })
    }

pub(super) fn prepare_tool<'a>(
        turn: &'a mut MainTurn<'_>,
        _ctx: &'a mut Context<'_>,
        call_index: usize,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            if turn.owner_changed()? {
                return Ok(false);
            }

            let call = turn.round_calls[call_index].clone();

            // A disconnected SSE client must not stop work already promised by
            // the model; explicit cancel and budgets remain the stop signals.
            if !turn.dispatched.contains_key(&call_index) && !turn.budget.charge_proposal() {
                return Err(format!(
                    "도구 제안 예산({}회)을 초과했습니다.",
                    turn.max_tools.saturating_mul(2)
                ));
            }
            if turn.state.is_cancelled(turn.session_id)
                || turn.state.cancellation_generation(turn.session_id) != turn.generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            let arguments = match serde_json::from_str::<Value>(&call.arguments) {
                Ok(value) if value.is_object() => value,
                _ => {
                    mark_tool_argument_signal(turn.state, &turn.request, turn.execution_id, false);
                    let error = format!(
                        "invalid tool arguments: {} 도구 인수가 올바른 JSON 객체가 아닙니다. 인자를 고쳐 다시 호출하세요.",
                        call.name
                    );
                    record_soft_tool_error(
                        turn.state,
                        turn.tx,
                        turn.session_id,
                        &turn.request,
                        turn.execution_id,
                        &call.name,
                        &call.id,
                        &error,
                        turn.metrics,
                    );
                    turn.messages.push(json!({
                        "role":"tool","tool_call_id":call.id,
                        "content": json!({"error":error}).to_string()
                    }));
                    return Ok(false);
                }
            };
            let completed_operations = turn
                .state
                .store
                .execution_operations(turn.execution_id)?
                .into_iter()
                .filter(|operation| operation["status"].as_str() == Some("completed"))
                .collect::<Vec<_>>();
            let procedure_violation = turn.strategy_candidates.iter().find_map(|strategy| {
                if !crate::engine::procedure::applies(
                    strategy,
                    &turn.request.input,
                    Some(&call.name),
                    Some(&arguments),
                ) {
                    return None;
                }
                crate::engine::procedure::missing_requirement(
                    strategy,
                    &call.name,
                    &arguments,
                    &completed_operations,
                )
            });
            if let Some(error) = procedure_violation {
                // A learned procedure guides the model; it does not replace its
                // judgment unless the strategy explicitly opts into enforcement.
                let enforced = turn.strategy_candidates.iter().any(|strategy| {
                    crate::engine::procedure::applies(
                        strategy,
                        &turn.request.input,
                        Some(&call.name),
                        Some(&arguments),
                    ) && (strategy["enforce"].as_bool() == Some(true)
                        || strategy["verification"]["enforce"].as_bool() == Some(true))
                });
                if enforced {
                    record_soft_tool_error(
                        turn.state,
                        turn.tx,
                        turn.session_id,
                        &turn.request,
                        turn.execution_id,
                        &call.name,
                        &call.id,
                        &error,
                        turn.metrics,
                    );
                    turn.messages.push(json!({
                        "role":"tool","tool_call_id":call.id,
                        "content": json!({"error":error,"procedureViolation":true}).to_string()
                    }));
                    return Ok(false);
                }
                turn.messages.push(json!({
                    "role":"system",
                    "content": format!(
                        "절차 관찰: {error}. 이 전략은 강제가 아니므로 지금 실행을 계속할 수 있습니다. 필요하면 순서를 바꾸세요."
                    )
                }));
            }
            let mutating_call = matches!(
                call.name.as_str(),
                "workspace_write"
                    | "workspace_replace"
                    | "workspace_replace_range"
                    | "file_write"
                    | "shell_exec"
                    | "terminal_exec"
            );
            let has_prior_read = completed_operations.iter().any(|operation| {
                matches!(
                    operation["tool"].as_str(),
                    Some(
                        "workspace_list"
                            | "workspace_read"
                            | "workspace_read_range"
                            | "file_list"
                            | "file_read"
                    )
                )
            });
            if mutating_call && !has_prior_read {
                let tool_selection = crate::addons::runtime_hooks::choose_for_agent(
                    turn.state,
                    "tool.selection",
                    turn.request.agent_id.as_deref(),
                    Some(turn.execution_id),
                )
                .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
                .unwrap_or_else(|| "read-before-write".into());
                if tool_selection == "block-write-without-read" {
                    let error = "도구 선택 알고리즘(block-write-without-read)이 읽기/목록 확인 없는 쓰기를 차단했습니다. workspace_list 또는 workspace_read를 먼저 실행하세요.";
                    record_soft_tool_error(
                        turn.state,
                        turn.tx,
                        turn.session_id,
                        &turn.request,
                        turn.execution_id,
                        &call.name,
                        &call.id,
                        error,
                        turn.metrics,
                    );
                    crate::addons::runtime_hooks::mark_signal(
                        turn.state,
                        turn.execution_id,
                        "tool.selection",
                        false,
                    );
                    turn.messages.push(json!({
                        "role":"tool","tool_call_id":call.id,
                        "content": json!({"error":error,"toolSelectionBlocked":true}).to_string()
                    }));
                    return Ok(false);
                } else if tool_selection == "read-before-write" {
                    turn.messages.push(json!({
                        "role":"system",
                        "content":"도구 선택 알고리즘(read-before-write): 쓰기 전에 workspace_list/workspace_read로 대상 상태를 먼저 확인하는 것이 권장됩니다. 이 힌트는 실행을 막지 않습니다."
                    }));
                }
            }
            let call_key = format!("{}:{}", call.name, arguments);
            let current_fingerprint = environment_fingerprint(turn.state, &call.name, &arguments);
            let recovery_mode = crate::addons::runtime_hooks::choose_for_agent(
                turn.state,
                "recovery.strategy",
                turn.request.agent_id.as_deref(),
                Some(turn.execution_id),
            )
            .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
            .unwrap_or_else(|| "fingerprint".into());
            let repeat_allowed = match recovery_mode.as_str() {
                "allow-all" => true,
                "block-identical" => false,
                _ => turn
                    .failed_calls
                    .get(&call_key)
                    .is_some_and(|(_, fingerprint)| fingerprint != &current_fingerprint),
            };
            if repeat_allowed {
                turn.failed_calls.remove(&call_key);
            }
            // Read-only calls are idempotent. Blocking them only removes
            // information the model needs; mutating calls still require an
            // actually changed environment fingerprint.
            let mutating = matches!(
                call.name.as_str(),
                "workspace_write"
                    | "workspace_replace"
                    | "workspace_replace_range"
                    | "file_write"
                    | "shell_exec"
                    | "terminal_exec"
                    | "github_api"
                    | "github_file_update" | "github_file_replace"
                    | "delegate_task"
                        | "task_integrate"
            );
            if mutating {
                if recovery_mode != "allow-all" {
                    if let Some((previous_error, _)) = turn.failed_calls.get(&call_key) {
                        // Section 11-1: an unchanged repeat of a failed mutating call
                        // is not re-executed. The model must choose a changed action.
                        // A retry is allowed once the environment/state actually changed.
                        let previous_error = previous_error.clone();
                        let error =
                            crate::engine::adaptation_loop::repeated_failure_message(&call.name);
                        record_soft_tool_error(
                            turn.state,
                            turn.tx,
                            turn.session_id,
                            &turn.request,
                            turn.execution_id,
                            &call.name,
                            &call.id,
                            &error,
                            turn.metrics,
                        );
                        turn.messages.push(json!({
                            "role":"tool","tool_call_id":call.id,
                            "content": json!({
                                "error": error,
                                "previousError": previous_error,
                                "repeatedFailedCall": true,
                            }).to_string()
                        }));
                        turn.messages.push(json!({
                    "role":"system",
                    "content":"직전 실패와 동일한 호출이 차단되었습니다. 다른 도구/인자로 실행하거나, 원인을 확인하는 probe를 선택하거나, 실제로 막혔다면 그 이유를 최종 답변에 쓰세요."
                }));
                        return Ok(false);
                    }
                }
            }
            let repeat = {
                let counter = turn.call_counts.entry(call_key.clone()).or_insert(0);
                *counter += 1;
                *counter
            };
            if repeat > 2 {
                // Repeated calls are an observation, not a hard stop. The model
                // sees the same real result and decides whether to change tack.
                turn.messages.push(json!({
                    "role":"system",
                    "content": format!(
                        "`{}` 호출이 {}회 반복되었습니다. 같은 결과가 나온다면 다른 접근을 선택하세요.",
                        call.name, repeat
                    )
                }));
            }

            turn.prepared_arguments = arguments;
            turn.prepared_key = call_key;
            turn.completed_operations = completed_operations;
            Ok(true)
        })
    }

pub(super) fn dispatch_tool<'a>(
        turn: &'a mut MainTurn<'_>,
        ctx: &'a mut Context<'_>,
        call_index: usize,
    ) -> BoxFuture<'a, Result<ToolReply, String>> {
        Box::pin(async move {
            let round = ctx.round;
            let metrics = turn.metrics;
            let max_tools = turn.max_tools;
            let call = turn.round_calls[call_index].clone();
            let arguments = turn.prepared_arguments.clone();
            let (result, ok, tool_content) =
                if let Some(result) = turn.dispatched.remove(&call_index) {
                    result?
                } else {
                    if !turn.budget.charge_tool() {
                        mark_budget_exhausted(turn.state, turn.execution_id);
                        return Err(format!("도구 호출 제한({max_tools}회)을 초과했습니다."));
                    }
                    let mut end = call_index + 1;
                    if turn.parallel_reads && tool_is_read_only(&call.name) {
                        let candidate_end = dominion_core::hosted::read_batch_end(
                            &turn.round_calls,
                            call_index,
                            4,
                            |call| {
                                tool_is_read_only(&call.name)
                                    && dominion_core::journal::parallel_safe(&call.name)
                            },
                        );
                        while end < candidate_end {
                            let next = &turn.round_calls[end];
                            if !parallel_read_admitted(
                                next,
                                &turn.session,
                                &turn.round_tools,
                                &turn.strategy_candidates,
                                &turn.request.input,
                                &turn.completed_operations,
                            ) || !turn.budget.can_execute_tool()
                            {
                                break;
                            }
                            if !turn.budget.charge_proposal() || !turn.budget.charge_tool() {
                                break;
                            }
                            end += 1;
                        }
                    }
                    let mut prepared = Vec::new();
                    for index in call_index..end {
                        turn.op_seq += 1;
                        let parsed = if index == call_index {
                            arguments.clone()
                        } else {
                            serde_json::from_str(&turn.round_calls[index].arguments)
                                .map_err(|error| format!("도구 인수 해석 실패: {error}"))?
                        };
                        prepared.push((index, turn.op_seq, parsed));
                    }
                    ctx.snapshot.step = round + 1;
                    ctx.snapshot.model_calls = turn.metrics.lock().unwrap().model_calls as usize;
                    ctx.snapshot.charged_tokens = turn.total_tokens;
                    let actions = prepared
                        .iter()
                        .map(|(index, _, args)| dominion_core::hosted::Action {
                            id: format!("{round}:{}", turn.round_calls[*index].id),
                            tool: turn.round_calls[*index].name.clone(),
                            arguments: args.clone(),
                            depends_on: vec![],
                        })
                        .collect::<Vec<_>>();
                    let dispatcher = ChatKernelDispatcher {
                        state: turn.state,
                        tx: turn.tx,
                        session: &turn.session,
                        request: &turn.request,
                        session_id: turn.session_id,
                        execution_id: turn.execution_id,
                        metrics,
                        prepared: &prepared,
                        calls: &turn.round_calls,
                        round,
                        compat: turn.provider.tool_mode == "compat",
                        definitions: &turn.round_tools,
                    };
                    let results = dominion_core::hosted::dispatch_batch(
                        &mut ctx.snapshot,
                        &actions,
                        &dispatcher,
                        ctx.journal,
                    )
                    .await?;
                    if prepared.len() > 1 {
                        crate::addons::runtime_hooks::mark_signal(
                            turn.state,
                            turn.execution_id,
                            "tool.combination",
                            results
                                .iter()
                                .all(|result| result.as_ref().is_ok_and(|(_, ok, _)| *ok)),
                        );
                    }
                    for ((index, _, _), result) in prepared.into_iter().zip(results) {
                        turn.dispatched.insert(index, result);
                    }
                    let observed = turn
                        .dispatched
                        .remove(&call_index)
                        .ok_or("도구 배치 결과가 없습니다.")?;
                    // A per-call rejection (unknown tool name, invalid
                    // arguments) is a tool result the model can recover from,
                    // not a turn failure. The durable receipt already recorded
                    // the rejection, so no effect is lost by continuing.
                    observed.unwrap_or_else(|error| {
                        (
                            json!({"error": error.clone()}),
                            false,
                            format!("TOOL ERROR: {error}"),
                        )
                    })
                };

            Ok((result, ok, tool_content))
        })
    }

pub(super) fn observe_tool<'a>(
        turn: &'a mut MainTurn<'_>,
        ctx: &'a mut Context<'_>,
        call_index: usize,
        observed: ToolReply,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let round = ctx.round;
            let call = turn.round_calls[call_index].clone();
            let arguments = turn.prepared_arguments.clone();
            let call_key = turn.prepared_key.clone();
            let (result, ok, tool_content) = observed;
            if turn.owner_changed()? {
                turn.messages
                    .push(json!({"role":"tool","tool_call_id":call.id,"content":tool_content}));
                return Ok(());
            }

            turn.ordering_used = true;
            if !ok {
                turn.ordering_ok = false;
            }
            crate::addons::runtime_hooks::mark_signal(
                turn.state,
                turn.execution_id,
                "tool.selection",
                ok,
            );
            crate::addons::runtime_hooks::mark_signal(
                turn.state,
                turn.execution_id,
                "recovery.strategy",
                ok,
            );
            if ok {
                turn.failed_calls.remove(&call_key);
                // The model called the stable discovery bridge, but the native
                // dispatcher executed and receipted the underlying real tool.
                // An actual mutation must invalidate environment observations.
                let executed_tool = if call.name == crate::engine::capability::CALL {
                    serde_json::from_str::<Value>(&call.arguments).ok()
                        .and_then(|args| args["name"].as_str().map(str::to_string))
                        .unwrap_or_else(|| call.name.clone())
                } else {
                    call.name.clone()
                };
                if matches!(
                    executed_tool.as_str(),
                    "workspace_write"
                        | "workspace_replace"
                        | "workspace_replace_range"
                        | "file_write"
                        | "shell_exec"
                        | "terminal_exec"
                        | "github_api"
                        | "github_file_update" | "github_file_replace"
                        | "delegate_task"
                        | "task_integrate"
                ) {
                    turn.environment_version += 1;
                }
            } else if result["preflightBlocked"] != true && result["invalidWorkspacePath"] != true {
                let error_text = result["error"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| tool_content.chars().take(600).collect::<String>());
                turn.failed_calls.insert(
                    call_key.clone(),
                    (
                        error_text.clone(),
                        environment_fingerprint(turn.state, &call.name, &arguments),
                    ),
                );
                crate::engine::adaptation_loop::record_tool_failure(
                    turn.state,
                    turn.execution_id,
                    round,
                    &call.id,
                    &call.name,
                    &arguments,
                    &error_text,
                );
                if turn.adaptation_attempts < 3
                    && crate::addons::runtime_hooks::strategies_enabled(
                        turn.state,turn.session_id,turn.request.agent_id.as_deref(),
                    )
                    && !turn.request.provider_id.as_deref().unwrap_or("").is_empty()
                {
                    turn.adaptation_attempts += 1;
                    match attempt_model_adaptation(
                        turn.state,
                        turn.tx,
                        &turn.session,
                        &turn.request,
                        turn.session_id,
                        turn.execution_id,
                        round,
                        &call.id,
                        &call.name,
                        &arguments,
                        &error_text,
                        turn.metrics,
                        &mut turn.budget,
                        &mut turn.op_seq,
                        turn.environment_version,
                    )
                    .await
                    {
                        Ok(adapted) => {
                            let selected = &adapted["selected"];
                            let result = &adapted["result"];
                            let acceptance = &adapted["acceptanceDecision"];
                            turn.messages.push(json!({
                                "role":"system",
                                "content": format!(
                                    "실제 복구 실행: 후보 {} (tool={}) 결과={}. \
                                     진단 증거={}. 동결된 완료조건 재검증={}. \
                                     완료조건에 진전이 없으면 동일한 변경을 반복하지 말고 \
                                     해당 자원의 읽기/검증부터 다시 수행하세요.",
                                    selected["id"],selected["tool"],result,
                                    adapted["diagnosticEvidence"],acceptance
                                )
                            }));
                            let changed_environment = tool_succeeded(result)
                                && matches!(selected["tool"].as_str(),Some(
                                    "workspace_write" | "workspace_replace"
                                    | "workspace_replace_range" | "file_write"
                                    | "shell_exec" | "terminal_exec"
                                    | "github_api" | "github_file_update"
                                    | "github_file_replace" | "delegate_task"
                                    | "task_integrate"
                                ));
                            // A modification changes the world even if the
                            // verifier rejects it: future preconditions must
                            // never reuse a stale environment fingerprint.
                            if changed_environment {
                                turn.environment_version += 1;
                            }
                            let decision = acceptance["decision"].as_str()
                                .unwrap_or("not_applicable");
                            if decision == "improved" {
                                turn.adaptation_failures = 0;
                            } else if decision == "not_applicable" {
                                if changed_environment {
                                    turn.adaptation_failures = 0;
                                } else {
                                    turn.adaptation_failures += 1;
                                }
                            } else {
                                // A tool transport success is not recovery.
                                turn.adaptation_failures += 1;
                                let _ = turn.tx.send(json!({
                                    "type":"adaptation_acceptance",
                                    "executionId":turn.execution_id,
                                    "decision":decision,
                                    "resolved":acceptance["resolved"],
                                    "regressed":acceptance["regressed"]
                                }));
                            }
                        }
                        Err(error) => {
                            record_adaptation_error(turn.state, turn.execution_id, &error);
                            turn.messages.push(json!({
                                "role":"system",
                                "content": format!(
                                    "적응 후보 생성/실행에 실패했습니다: {error}. 모델은 원인 가설과 다른 도구/인자를 직접 선택하세요."
                                )
                            }));
                        }
                    }
                }
                if turn.adaptation_failures >= 2 {
                    turn.force_final = true;
                    turn.messages.push(json!({
                        "role":"system",
                        "content":"연속 적응 실행이 진전 없이 실패했습니다. 추가 반복을 중단하고 실제 관측과 막힌 원인을 최종 답변에 쓰세요."
                    }));
                }
            }
            // Keep full evidence in the in-memory turn. Per-provider
            // request assembly clips only when *that model's* configured
            // context is too small, so switching local -> API can recover it.
            turn.messages.push(json!({
                "role":"tool","tool_call_id":call.id,"content":tool_content,
            }));

            Ok(())
        })
    }

pub(super) fn verify_tools<'a>(
        turn: &'a mut MainTurn<'_>,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            let round = ctx.round;

            if turn.ordering_used {
                crate::addons::runtime_hooks::mark_signal(
                    turn.state,
                    turn.execution_id,
                    "execution.ordering",
                    turn.ordering_ok,
                );
            }
            // Check acceptance after the tool round, not only when the model
            // decides to stop. A model that keeps verifying can otherwise burn the
            // whole round budget after the work is already complete.
            if !turn.force_final {
                if let Some((acceptance_text, checks)) = &turn.acceptance {
                    let evidence = turn
                        .reply_to
                        .as_deref()
                        .map(|source| {
                            evidence_for_source(
                                turn.state,
                                turn.session_id,
                                source,
                                turn.execution_id,
                            )
                        })
                        .unwrap_or_default();
                    let current_gaps = verification::check_acceptance(
                        turn.state, acceptance_text, checks, &evidence,
                    );
                    let progress = crate::engine::recovery::acceptance_progress(
                        turn.state, turn.execution_id, round, &current_gaps, &evidence,
                    )?;
                    if progress["mode"] == "reconsider" && !turn.force_final {
                        let _ = turn.tx.send(json!({
                            "type":"acceptance_replan",
                            "progress": progress,
                        }));
                        turn.messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "Execution verifier detected stalled progress. {}",
                                progress["nextAction"].as_str().unwrap_or("")
                            )
                        }));
                    }
                    if current_gaps.is_empty() && !evidence.is_empty() {
                        turn.acceptance_passed = true;
                        turn.force_final = true;
                        turn.state.store.audit(
                        "acceptance.passed",
                        json!({"sessionId": turn.session_id, "executionId": turn.execution_id, "round": round}),
                    );
                        turn.messages.push(json!({
                        "role": "system",
                        "content": "모든 검증 조건이 충족되었습니다. 추가 도구 호출 없이 최종 답변만 작성하세요."
                    }));
                        let _ = turn.tx.send(json!({"type": "acceptance_passed"}));
                        if let Some(path) = turn.simple_creation_target.as_ref().filter(|path| {
                            std::fs::metadata(path).is_ok_and(|file_metadata| {
                                file_metadata.is_file() && file_metadata.len() == 0
                            }) && !turn.strategy_candidates.iter().any(|strategy| {
                                strategy["verification"]["type"] == "procedure"
                                    && crate::engine::procedure::applies(
                                        strategy,
                                        &turn.request.input,
                                        None,
                                        None,
                                    )
                            })
                        }) {
                            // The native action has already finished and the host
                            // verified the exact owner-bound artifact. Reporting
                            // that fact does not require another paid model turn.
                            turn.final_content =
                                format!("파일을 만들었습니다: `{}`", path.display());
                            turn.metadata["completionSource"] = json!("verified_artifact");
                            // This provider projection predates the just-finished
                            // tool exchange. Let the next turn derive that full
                            // exchange from the durable transcript before adding
                            // this host-generated summary to the working view.
                            turn.continuation_projection = None;
                            // Do not stream completion before the owner
                            // revision gate and durable assistant save succeed.
                            return Ok(Flow::Complete);
                        }
                    }
                }
            }

            Ok(Flow::Continue)
        })
    }

pub(super) fn reconcile_verified<'a>(
        turn: &'a mut MainTurn<'_>,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            // Durable, owner-bound evidence is the only path to a
            // zero-model-call terminal transition.
            if !turn.force_final || !turn.acceptance_passed
                || !turn.closure_admitted()?
            {
                return Ok(Flow::Continue);
            }
            let Some(receipts) = terminal_acceptance_receipt(
                turn.state,
                turn.session_id,
                turn.reply_to.as_deref(),
                turn.execution_id,
                turn.acceptance.as_ref(),
            ) else {
                // A file or contract changed after the first check. Permit
                // normal repair; never retain a stale passed flag.
                turn.acceptance_passed = false;
                turn.force_final = false;
                return Ok(Flow::Continue);
            };
            let checks = turn.acceptance.as_ref()
                .and_then(|(_, checks)| checks.as_array());
            let checked = checks.map_or(0, Vec::len);
            // An earlier provisional reply can be stale after a repair.
            // Final text must come from this latest verified checkpoint.
            let paths = checks.into_iter().flatten()
                .filter_map(|check| check["path"].as_str())
                .filter(|path| !path.trim().is_empty())
                .take(8)
                .map(|path| format!("`{path}`"))
                .collect::<Vec<_>>();
            turn.final_content = if paths.is_empty() {
                format!("실제 도구 실행 {receipts}건을 확인하고 검증 조건 {checked}개를 충족했습니다.")
            } else {
                format!("실제 도구 실행 {receipts}건을 확인하고 검증 조건 {checked}개를 충족했습니다. 확인된 파일: {}.",paths.join(", "))
            };
            // Publish only after the owner revision has been confirmed and
            // the final assistant message is durable in save().
            turn.metadata["completionSource"] = json!("kernel_verified_checkpoint");
            // The provider projection predates the completed tool exchange.
            // The next request must rebuild tool-call/result pairing from the
            // durable transcript rather than project a provisional reply.
            turn.continuation_projection = None;
            turn.state.store.audit("acceptance.kernel_reconciled", json!({
                "executionId": turn.execution_id,
                "sessionId": turn.session_id,
                "receiptCount": receipts,
                "obligationCount": checked,
                "modelRoundSaved": true,
            }));
            Ok(Flow::Complete)
        })
    }

