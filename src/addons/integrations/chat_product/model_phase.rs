//! MainTurn model preparation, completion and verification.
use super::*;

// A zero tool budget forbids tools; it must not forbid the model's first answer.
fn tool_budget_exhausted(used: usize, limit: usize) -> bool {
    if limit == 0 { used > 0 } else { used >= limit }
}

pub(super) fn prepare_model<'a>(
        turn: &'a mut MainTurn<'_>,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Vec<Value>, String>> {
        Box::pin(async move {
            turn.apply_owner_directives().await?;
            let metrics = turn.metrics;
            let max_tools = turn.max_tools;

            if turn.budget.expired() {
                return Err("실행 시간 제한(10분)을 초과했습니다.".into());
            }
            if turn.state.is_cancelled(turn.session_id)
                || turn.state.cancellation_generation(turn.session_id) != turn.generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            if turn.started_at.elapsed() > turn.elapsed_limit {
                return Err("실행 시간 제한(10분)을 초과했습니다.".into());
            }
            if tool_budget_exhausted(turn.metrics.lock().unwrap().tool_calls as usize, turn.max_tools) {
                mark_budget_exhausted(turn.state, turn.execution_id);
                return Err(format!("도구 호출 제한({max_tools}회)을 초과했습니다."));
            }
            // DECIDE: an active durable program supplies the next step; otherwise
            // the model decides. Program steps and model steps share this loop,
            // the same tool gateway and the same checkpoint.
            if !turn.program_finished && turn.request.tools_execution_enabled {
                if let Some(policy) = &turn.program_policy {
                    if let Some(program) = policy["parameters"]["program"].as_object() {
                        let program = crate::engine::program::validate(&json!(program))?;
                        restore_program_state(
                            turn.state,
                            turn.task_id,
                            turn.execution_id,
                            &program,
                        )?;
                        let program_messages = run_program(
                            turn.state,
                            turn.tx,
                            &turn.session,
                            &turn.request,
                            turn.session_id,
                            turn.execution_id,
                            &program,
                            metrics,
                            &mut turn.budget,
                            &mut turn.op_seq,
                            turn.generation,
                        )
                        .await?;
                        turn.messages.extend(program_messages);
                        let status = turn
                            .state
                            .store
                            .get("execution", turn.execution_id)
                            .and_then(|execution| {
                                execution["programCheckpoint"]["status"]
                                    .as_str()
                                    .map(str::to_string)
                            });
                        turn.program_finished = status.as_deref() == Some("completed");
                    }
                }
            }

            {
                let chars: usize = turn
                    .messages
                    .iter()
                    .filter_map(|message| message["content"].as_str())
                    .map(str::len)
                    .sum();
                if chars > turn.provider.context_tokens.unwrap_or(16_384) as usize * 4 {
                    if !turn.context_overflow {
                        turn.context_overflow = true;
                        if let Some(mut execution) =
                            turn.state.store.get("execution", turn.execution_id)
                        {
                            execution["contextOverflow"] = json!(true);
                            turn.state.store.put("execution", &execution);
                        }
                    }
                }
            }
            turn.model_round.model_started = std::time::Instant::now();
            turn.model_round.model_offset_ms = turn.turn_started.elapsed().as_millis() as u64;
            turn.model_round.round_tools = if turn.force_final || turn.max_tools == 0 {
                Vec::new()
            } else {
                turn.tool_definitions.clone()
            };
            if !turn.model_round.round_tools.is_empty() {
                let tool_schema_budget = turn
                    .provider
                    .context_tokens
                    .map(|tokens| {
                        let reserve = turn
                            .provider
                            .max_output_tokens
                            .unwrap_or(tokens / 8)
                            .min(tokens / 4)
                            .max(128);
                        ((tokens.saturating_sub(reserve) as usize) * 4 / 2).clamp(4_000, 60_000)
                    })
                    .unwrap_or(60_000);
                turn.model_round.round_tools = trim_tool_definitions(
                    std::mem::take(&mut turn.model_round.round_tools),
                    tool_schema_budget,
                );
            }
            let trace_budget=(turn.provider.context_tokens.unwrap_or(16_384) as usize)
                .saturating_mul(4).saturating_div(3).clamp(4_000,24_000);
            let projection=dominion_algorithms::tool_history::project(&turn.messages,trace_budget);
            turn.model_round.wire_messages=projection.messages;
            if let Some(guidance)=dominion_algorithms::failure_guidance::latest(&ctx.snapshot.receipts){
                turn.model_round.wire_messages.push(json!({"role":"user","content":
                    json!({"runtimeFailureGuidance":guidance.for_catalog(&turn.model_round.round_tools)}).to_string()}));
            }
            if turn.is_room {
                if let Some(mut execution) = turn.state.store.get("execution", turn.execution_id) {
                    execution["inputContext"]["preparedInputTokensEstimate"] =
                        json!(crate::engine::chat::room_context::input_estimate(
                            &turn.model_round.wire_messages,
                            &turn.model_round.round_tools
                        ));
                    execution["inputContext"]["preProviderDigest"] =
                        json!(crate::engine::memory::stable_id(&[
                            &json!({"messages":turn.model_round.wire_messages,"tools":turn.model_round.round_tools})
                                .to_string()
                        ]));
                    turn.state.store.put("execution", &execution);
                }
            }
            Ok(turn.model_round.round_tools.clone())
        })
    }

pub(super) fn model<'a>(
        turn: &'a mut MainTurn<'_>,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<ProviderReply, String>> {
        Box::pin(async move {
            let round = ctx.round;
            {
                let mut counters = turn.metrics.lock().unwrap();
                counters.model_calls += 1;
                counters.iterations = round as u32 + 1;
            }
            let completion_result = stream_completion(
                turn.state,
                &turn.client,
                &turn.provider,
                &turn.model_round.wire_messages,
                &turn.model_round.round_tools,
                &turn.request,
                turn.session_id,
                turn.tx,
                turn.allow_provider_fallback,
            )
            .await;
            {
                let mut counters = turn.metrics.lock().unwrap();
                counters.model_elapsed_ms += turn.model_round.model_started.elapsed().as_millis() as u64;
                if counters.first_token_ms.is_none() {
                    if let Ok((completion, _effective_provider)) = &completion_result {
                        counters.first_token_ms = completion
                            .first_delta_ms
                            .map(|ms| turn.model_round.model_offset_ms + ms);
                    }
                }
            }

            completion_result
        })
    }

pub(super) fn observe_model<'a>(
        turn: &'a mut MainTurn<'_>,
        _ctx: &'a mut Context<'_>,
        reply: ProviderReply,
    ) -> BoxFuture<'a, Result<ModelDecision, String>> {
        Box::pin(async move {
            let (mut completion, selected_provider) = reply;
            if selected_provider.id != turn.provider.id {
                // The streamed response came from a fallback route; later rounds
                // must continue on that route rather than retrying the dead one.
                if let Some(mut execution) = turn.state.store.get("execution", turn.execution_id) {
                    execution["effectiveProviderId"] = json!(selected_provider.id);
                    execution["effectiveModel"] = json!(selected_provider.model);
                    execution["failoverAt"] = json!(chrono::Utc::now().to_rfc3339());
                    turn.state
                        .store
                        .put("execution", &execution)
                        .ok_or("대체 모델을 실행 기록에 저장할 수 없습니다.")?;
                }
                if let Some(agent_id) = turn.request.agent_id.as_deref() {
                    crate::engine::router::rotate_agent_route(
                        turn.state,
                        agent_id,
                        &selected_provider.id,
                        &selected_provider.model,
                    );
                }
                turn.provider = selected_provider;
            }

            let has_dsml_call = completion.content.contains("invoke name=\"")
                && completion.content.contains("parameter name=\"");
            if (!turn.request.tools_enabled || !turn.request.tools_execution_enabled)
                && (!completion.tool_calls.is_empty() || has_dsml_call)
            {
                return Err("도구가 비활성인 대화에서 모델이 도구 실행을 요청했습니다.".into());
            }
            turn.metadata["continuity"] = completion.continuity.clone().unwrap_or(Value::Null);
            turn.continuation_projection = completion.projection.take();
            if let Some(mut execution) = turn.state.store.get("execution", turn.execution_id) {
                execution["modelContinuity"] = turn.metadata["continuity"].clone();
                if let Some(estimate) =
                    turn.metadata["continuity"]["workingView"]["estimatedInputTokens"].as_u64()
                {
                    execution["inputContext"]["projectedInputTokensEstimate"] = json!(estimate);
                }
                turn.state.store.put("execution", &execution);
            }

            if !completion.reasoning.is_empty() {
                turn.final_reasoning.push_str(&completion.reasoning);
            }
            turn.last_reasoning = completion.reasoning.clone();
            // Intermediate preambles are stored with their tool-call message, so
            // the final message keeps only the last substantive answer.
            // Exact provider usage when available; otherwise estimate from the
            // prompt and the streamed output so the UI always shows a number.
            let (call_prompt, call_completion) = if completion.usage_exact {
                (completion.prompt_tokens, completion.completion_tokens)
            } else {
                turn.usage_estimated = true;
                (
                    if completion.prompt_tokens>0 {
                        completion.prompt_tokens
                    } else {
                        turn.metadata["continuity"]["workingView"]["estimatedInputTokens"]
                            .as_u64()
                            .unwrap_or_else(|| {
                                crate::engine::chat::room_context::input_estimate(
                                    &turn.model_round.wire_messages,&turn.model_round.round_tools,
                                ) as u64
                            })
                    },
                    if completion.completion_tokens>0 {
                        completion.completion_tokens
                    } else {
                        crate::engine::chat::room_context::estimate(&format!(
                            "{}{}",completion.content,completion.reasoning
                        )).max(1) as u64
                    },
                )
            };
            turn.prompt_tokens_total += call_prompt;
            turn.completion_tokens_total += call_completion;
            if completion.usage_exact {
                if let Some(cached) = completion.cached_prompt_tokens {
                    turn.cached_prompt_tokens_total = turn
                        .cached_prompt_tokens_total
                        .saturating_add(cached.min(call_prompt));
                } else {
                    turn.cache_reporting_complete = false;
                }
            } else {
                turn.cache_reporting_complete = false;
            }
            let call_total = if completion.usage > 0 {
                completion.usage
            } else {
                call_prompt + call_completion
            };
            turn.total_tokens += call_total;
            if let Some(mut execution) = turn.state.store.get("execution", turn.execution_id) {
                    // Attribute each actual provider model call, including
                    // ordinary 1:1 chat, not only group-room speakers.
                    // A missing cache field stays unknown, never a fake zero.
                    let cached = if completion.usage_exact {
                        completion.cached_prompt_tokens.map(|value|value.min(call_prompt))
                    } else { None };
                    let mut calls=execution["inputContext"]["modelCallUsage"]
                        .as_array().cloned().unwrap_or_default();
                    calls.push(json!({
                        "completedCallIndex":calls.len()+1,
                        "providerId":turn.provider.id,"model":turn.provider.model,
                        "promptTokens":completion.usage_exact.then_some(call_prompt),
                        "cachedPromptTokens":cached,
                        "newPromptTokens":cached.map(|value|call_prompt.saturating_sub(value)),
                        "outputTokens":completion.usage_exact.then_some(call_completion),
                        "estimatedPromptTokens":(!completion.usage_exact).then_some(call_prompt),
                        "estimatedOutputTokens":(!completion.usage_exact).then_some(call_completion),
                        "usageExact":completion.usage_exact,
                        "finishReason":completion.finish_reason,
                        "workingView":{
                            "rebaseReason":turn.metadata["continuity"]["workingView"]["rebaseReason"],
                            "reusedFrames":turn.metadata["continuity"]["workingView"]["reusedFrames"],
                            "appendedFrames":turn.metadata["continuity"]["workingView"]["appendedFrames"],
                            "estimatedReusedHistoryTokens":turn.metadata["continuity"]["workingView"]["estimatedReusedHistoryTokens"],
                            "estimatedNewHistoryTokens":turn.metadata["continuity"]["workingView"]["estimatedNewHistoryTokens"]
                        },
                        "incrementalTransport":turn.metadata["continuity"]["incrementalTransport"]
                    }));
                    execution["inputContext"]["modelCallUsage"]=json!(calls);
                    execution["inputContext"]["providerPromptTokens"] =
                        json!(turn.prompt_tokens_total);
                    execution["inputContext"]["providerCachedPromptTokens"] =
                        if turn.cache_reporting_complete {
                            json!(turn.cached_prompt_tokens_total)
                        } else {
                            Value::Null
                        };
                    execution["inputContext"]["providerNewPromptTokens"] =
                        if turn.cache_reporting_complete {
                            json!(turn
                                .prompt_tokens_total
                                .saturating_sub(turn.cached_prompt_tokens_total))
                        } else {
                            Value::Null
                        };
                    execution["inputContext"]["providerOutputTokens"] =
                        json!(turn.completion_tokens_total);
                    execution["inputContext"]["providerUsageExact"] = json!(!turn.usage_estimated);
                    execution["lastModelFinishReason"] = json!(completion.finish_reason);
                    turn.state.store.put("execution", &execution);
            }
            turn.metrics.lock().unwrap().tokens = turn.total_tokens;
            if turn.total_tokens > MAX_TOKENS
            {
                return Err(format!(
                    "토큰 예산({MAX_TOKENS})을 초과했습니다. 요청을 나눠 다시 시도하세요."
                ));
            }

            if turn.owner_changed()? {
                // Cancellation/owner revision wins over a late empty model
                // frame. The charged call is already in the usage ledger.
                turn.state.store.audit(
                    "job.stale_model_reply",
                    json!({"executionId":turn.execution_id,"revision":turn.owner_revision}),
                );
                return Ok(ModelDecision::Continue);
            }
            if let Some(error) = terminal_model_error(&completion) {
                return Err(error.to_string());
            }
            if turn.force_final {
                // The deterministic checks already passed. Ignore any late tool
                // request and let the model produce the final answer.
                completion.tool_calls.clear();
            }

            if has_dsml_call && turn.model_round.round_tools.is_empty() {
                return Err("모델이 이번 턴에 제공되지 않은 도구를 호출하려 했습니다.".into());
            }
            if completion.tool_calls.is_empty()
                && turn.request.tools_enabled
                && (turn.provider.tool_mode != "native" || has_dsml_call)
                && !turn.force_final
            {
                let known = turn.model_round
                    .round_tools
                    .iter()
                    .map(tool_name)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect::<std::collections::HashSet<_>>();
                let (parsed, cleaned) = crate::engine::chat::protocol::parse_text_tool_calls_known(
                    &completion.content,
                    &known,
                );
                if !parsed.is_empty() {
                    completion.content = cleaned;
                    completion.tool_calls = parsed
                        .into_iter()
                        .map(|(name, arguments)| ToolCall {
                            id: uuid::Uuid::new_v4().to_string(),
                            name,
                            arguments,
                        })
                        .collect();
                } else if has_dsml_call {
                    // A malformed/unknown DSML call must not be shown as a final
                    // answer. Remove the protocol block and fail the turn.
                    completion.content = cleaned;
                    return Err("모델이 지원되지 않는 도구 호출 형식을 출력했습니다.".into());
                }
            }

            let decision = if completion.tool_calls.is_empty() {
                ModelDecision::Verify
            } else {
                ModelDecision::Tools
            };
            turn.model_round.completion = Some(completion);
            Ok(decision)
        })
    }

pub(super) fn verify<'a>(turn: &'a mut MainTurn<'_>, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            if turn.owner_changed()? {
                return Ok(Flow::Continue);
            }

            let round = ctx.round;
            let completion = turn.model_round.completion.take().ok_or("모델 응답이 없습니다.")?;

            {
                let mut operations = turn
                    .state
                    .store
                    .execution_operations(turn.execution_id)?
                    .into_iter()
                    .filter(|operation| operation["status"].as_str() == Some("completed"))
                    .collect::<Vec<_>>();
                crate::engine::procedure::sort_operations(&mut operations);
                let violations = turn
                    .strategy_candidates
                    .iter()
                    .filter(|strategy| {
                        crate::engine::procedure::applies(strategy, &turn.request.input, None, None)
                    })
                    .filter_map(|strategy| {
                        let result = crate::engine::procedure::evaluate(
                            strategy,
                            &operations,
                            &turn.state.config.workspace,
                        );
                        if result["verified"].as_bool() == Some(true)
                            && result["followed"].as_bool() == Some(false)
                        {
                            Some(json!({
                                "strategyId": strategy["id"],
                                "text": strategy["text"],
                                "authority": strategy["authority"],
                                "violations": result["violations"],
                            }))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if !violations.is_empty() {
                    let standing = violations
                        .iter()
                        .any(|violation| violation["authority"].as_str() == Some("standing"));
                    if !turn.force_final && turn.procedure_repairs < turn.max_procedure_repairs {
                        turn.procedure_repairs += 1;
                        let _ = turn.tx.send(json!({
                            "type": "procedure_retry",
                            "violations": violations,
                        }));
                        turn.messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "방금 실행은 학습된 절차를 위반했습니다. 도구를 다시 호출해 아래 위반을 수정한 뒤 최종 답변을 작성하세요. 위반: {}",
                                serde_json::to_string(&violations).unwrap_or_default()
                            )
                        }));
                        return Ok(Flow::Continue);
                    }
                    if standing {
                        return Err(format!(
                            "필수 절차 위반으로 완료할 수 없습니다: {}",
                            serde_json::to_string(&violations).unwrap_or_default()
                        ));
                    }
                }
            }
            turn.final_content = if turn.force_final {
                // Never save a raw tool-call fragment as the answer.
                let (_, cleaned) = parse_text_tool_calls(&completion.content);
                cleaned
            } else {
                completion.content
            };

            // Ordinary-chat evidence gate: a model cannot complete an action
            // request by claiming success without successful tool results.
            if let Some((acceptance_text, checks)) = &turn.acceptance {
                let evidence = turn
                    .reply_to
                    .as_deref()
                    .map(|source| {
                        evidence_for_source(turn.state, turn.session_id, source, turn.execution_id)
                    })
                    .unwrap_or_default();
                let mut gaps =
                    verification::check_acceptance(turn.state, acceptance_text, checks, &evidence);
                let progress = crate::engine::recovery::acceptance_progress(
                    turn.state, turn.execution_id, round, &gaps, &evidence,
                )?;
                let mut next_action = progress["nextAction"]
                    .as_str().unwrap_or("").to_string();
                if !gaps.is_empty() {
                    let can_retry = turn.request.tools_enabled
                        && turn.request.tools_execution_enabled
                        && turn.acceptance_retries < turn.max_acceptance_retries
                        && round + 1 < turn.max_rounds
                        && turn.budget.can_execute_tool();
                    let repair = acceptance_repair(&progress, checks, can_retry);
                    turn.state.store.audit("acceptance.repair_strategy", json!({
                        "executionId":turn.execution_id,"round":round,
                        "strategy":format!("{repair:?}"),"canRetry":can_retry,
                        "stagnantRounds":progress["stagnantRounds"]
                    }));
                    if repair == AcceptanceRepair::Reconcile {
                        return Err(
                            "검증 대상에 결과가 확정되지 않은 외부 변경이 있습니다. \
                             기존 실행의 실제 효과를 먼저 확인·조정해야 하므로 자동 재시도를 중단합니다.".into()
                        );
                    }
                    // Prefer a verifier-derived, receipt-scoped next action.
                    // Pay for independent model review only when there is
                    // actual retry budget and no fresh typed repair frontier.
                    let review = if repair != AcceptanceRepair::Reviewer {
                        None
                    } else {
                        {
                            let mut counters=turn.metrics.lock().unwrap();
                            counters.model_calls+=1;
                            counters.auxiliary_calls+=1;
                        }
                        dominion_algorithms::engine::model_call(
                            &mut ctx.snapshot,
                            ctx.journal,
                            "acceptance_review",
                            review_action(
                                turn.state,
                                &turn.request,
                                acceptance_text,
                                &turn.final_content,
                                &evidence,
                                &gaps,
                                turn.execution_id,
                            ),
                        )
                        .await?
                    };
                    if turn.owner_changed()? {
                        return Ok(Flow::Continue);
                    }
                    if let Some(review) = review {
                        gaps.extend(review.gaps);
                        gaps.sort();
                        gaps.dedup();
                        if let Some(review_action) = review.next_action
                            .filter(|text| !text.trim().is_empty()) {
                            next_action = format!("{}\nReviewer proposal: {}",
                                next_action, review_action);
                            // Only independent reviewer suggestions are
                            // candidates for reusable learning.
                            let scope = crate::engine::memory::MemoryScope::for_session(
                                &turn.state.store,
                                turn.session_id,
                                turn.request.agent_id.as_deref(),
                            );
                            let mut evidence_ids = evidence
                                .iter()
                                .filter_map(|operation| {
                                    operation["id"].as_str().map(str::to_string)
                                })
                                .take(8)
                                .collect::<Vec<_>>();
                            if evidence_ids.is_empty() {
                                evidence_ids.push(turn.execution_id.to_string());
                            }
                            let _ = crate::engine::improve::propose(
                                turn.state,
                                &scope,
                                "strategy",
                                &next_action,
                                &format!("리뷰어가 실패 원인을 분석해 제안: {}", gaps.join("; ")),
                                &evidence_ids,
                                None,
                                "shared",
                            );
                        }
                        turn.state.store.audit(
                            "acceptance.review",
                            json!({
                                "sessionId": turn.session_id,
                                "executionId": turn.execution_id,
                                "gaps": gaps,
                                "nextAction": next_action,
                            }),
                        );
                    }
                    if !turn.request.tools_enabled {
                        return Err(format!(
                            "도구가 비활성 상태라 요청한 작업을 실제로 수행할 수 없습니다: {}",
                            gaps.join("; ")
                        ));
                    }
                    if can_retry {
                        turn.acceptance_retries += 1;
                        let _ = turn.tx.send(json!({
                            "type": "acceptance_retry",
                            "attempt": turn.acceptance_retries,
                            "gaps": gaps,
                            "progress": progress,
                        }));
                        turn.state.store.audit(
                            "acceptance.retry",
                            json!({
                                "sessionId": turn.session_id,
                                "executionId": turn.execution_id,
                                "round": round,
                                "gaps": gaps,
                            }),
                        );
                        turn.messages.push(json!({
                            "role": "user",
                            "content": format!(
                                "자동 검증/리뷰 결과 아직 완료되지 않았습니다:\n- {}\n\n{}\n말로만 완료하지 말고, 지금 실제 도구를 호출해 빠진 작업을 수행한 뒤 결과를 확인하세요.",
                                gaps.join("\n- "),
                                if next_action.trim().is_empty() {
                                    "전략을 바꿔 다시 시도하세요.".to_string()
                                } else {
                                    format!("다음 전략: {next_action}")
                                }
                            )
                        }));
                        return Ok(Flow::Continue);
                    }
                    turn.state.store.audit(
                        "acceptance.failed",
                        json!({
                            "sessionId": turn.session_id,
                            "executionId": turn.execution_id,
                            "gaps": gaps,
                        }),
                    );
                    return Err(format!(
                        "실행 증거가 부족하여 완료하지 못했습니다: {}",
                        gaps.join("; ")
                    ));
                }
                turn.acceptance_passed = true;
                turn.state.store.audit(
                    "acceptance.passed",
                    json!({"sessionId": turn.session_id, "executionId": turn.execution_id, "round": round}),
                );
            }

            return Ok(Flow::Complete);
        })
    }


#[cfg(test)]
mod direct_chat_budget_tests {
    use super::tool_budget_exhausted;

    #[test]
    fn zero_tool_budget_allows_model_answer_but_never_a_tool_call() {
        assert!(!tool_budget_exhausted(0, 0));
        assert!(tool_budget_exhausted(1, 0));
        assert!(!tool_budget_exhausted(0, 1));
        assert!(tool_budget_exhausted(1, 1));
    }
}
