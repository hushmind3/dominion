//! Phase-local adapters for SQLite, streaming transports and existing product policies.
//! The native core calls these phases; this module has no model/tool retry loop.
use super::*;
use dominion_core::engine::{Context, Flow, Host, ModelDecision};
use dominion_core::hosted::BoxFuture;

pub(super) struct MainTurn<'a> {
    pub(super) state: &'a AppState,
    pub(super) metrics: &'a std::sync::Mutex<Metrics>,
    pub(super) request: TurnRequest,
    pub(super) owner: Option<crate::engine::job_control::Owner>,
    pub(super) owner_revision: u64,
    pub(super) tx: &'a UnboundedSender<Value>,
    pub(super) session_id: &'a str,
    pub(super) execution_id: &'a str,
    pub(super) task_id: &'a str,
    pub(super) generation: u64,
    pub(super) source_message_id: Option<&'a str>,
    pub(super) session: Value,
    pub(super) strategy_candidates: Vec<Value>,
    pub(super) reply_to: Option<String>,
    pub(super) acceptance: Option<(String, Value)>,
    pub(super) max_acceptance_retries: usize,
    pub(super) max_procedure_repairs: usize,
    pub(super) acceptance_retries: usize,
    pub(super) procedure_repairs: usize,
    pub(super) program_policy: Option<Value>,
    pub(super) planning_policy: Option<Value>,
    pub(super) plan_requested: bool,
    pub(super) op_seq: u64,
    pub(super) metadata: Value,
    pub(super) provider: crate::engine::chat::ProviderRef,
    pub(super) allow_provider_fallback: bool,
    pub(super) max_rounds: usize,
    pub(super) max_tools: usize,
    pub(super) elapsed_limit: Duration,
    pub(super) simple_creation_target: Option<std::path::PathBuf>,
    pub(super) tool_definitions: Vec<Value>,
    pub(super) is_room: bool,
    pub(super) context_overflow: bool,
    pub(super) messages: Vec<Value>,
    pub(super) client: reqwest::Client,
    pub(super) turn_started: std::time::Instant,
    pub(super) final_content: String,
    pub(super) final_reasoning: String,
    pub(super) last_reasoning: String,
    pub(super) continuation_projection: Option<crate::engine::chat::continuity::Projection>,
    pub(super) total_tokens: u64,
    pub(super) prompt_tokens_total: u64,
    pub(super) completion_tokens_total: u64,
    pub(super) cached_prompt_tokens_total: u64,
    pub(super) cache_reporting_complete: bool,
    pub(super) usage_estimated: bool,
    pub(super) started_at: std::time::Instant,
    pub(super) finished: bool,
    pub(super) force_final: bool,
    pub(super) call_counts: std::collections::HashMap<String, usize>,
    pub(super) failed_calls: std::collections::HashMap<String, (String, String)>,
    pub(super) environment_version: u64,
    pub(super) adaptation_attempts: usize,
    pub(super) adaptation_failures: usize,
    pub(super) acceptance_passed: bool,
    pub(super) budget: super::super::budget::BudgetLedger,
    pub(super) program_finished: bool,
    pub(super) round_tools: Vec<Value>,
    pub(super) wire_messages: Vec<Value>,
    pub(super) model_started: std::time::Instant,
    pub(super) model_offset_ms: u64,
    pub(super) completion: Option<crate::engine::chat::Completion>,
    pub(super) round_calls: Vec<ToolCall>,
    pub(super) parallel_reads: bool,
    pub(super) dispatched: std::collections::HashMap<usize, Result<(Value, bool, String), String>>,
    pub(super) ordering_used: bool,
    pub(super) ordering_ok: bool,
    pub(super) prepared_arguments: Value,
    pub(super) prepared_key: String,
    pub(super) completed_operations: Vec<Value>,
}

impl Host for MainTurn<'_> {
    type Reply = (
        crate::engine::chat::Completion,
        crate::engine::chat::ProviderRef,
    );
    type ToolResult = (Value, bool, String);
    fn reload(&self) -> Result<dominion_core::hosted::Snapshot, String> {
        load_native_checkpoint(self.state, self.execution_id, &self.request.input)
    }
    fn initialize<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            self.apply_owner_directives().await?;
            let metrics = self.metrics;
            if let Some(path) = self.simple_creation_target.as_ref().filter(|path| {
                std::fs::metadata(path).is_ok_and(|file_metadata| file_metadata.is_file())
                    && !self.strategy_candidates.iter().any(|strategy| {
                        strategy["verification"]["type"] == "procedure"
                            && crate::engine::procedure::applies(
                                strategy,
                                &self.request.input,
                                None,
                                None,
                            )
                    })
            }) {
                // Creation is a desired state, not an instruction to overwrite an
                // existing file until its hash changes. The host can report this
                // independently observed state without a paid model or a mutation.
                self.final_content = format!(
                    "파일이 이미 있습니다: `{}`. 내용을 유지했습니다.",
                    path.display()
                );
                self.acceptance_passed = true;
                self.finished = true;
                self.metadata["completionSource"] = json!("existing_artifact");
                if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                    execution["deterministicShortcut"] =
                        json!({"kind":"file_already_exists","modelCalls":0,"tokens":0});
                    execution["artifactVerification"] =
                        json!({"path":path,"exists":true,"verified":true});
                    self.state
                        .store
                        .put("execution", &execution)
                        .ok_or("파일 확인 결과를 저장할 수 없습니다.")?;
                }
                let _ = self
                    .tx
                    .send(json!({"type":"text","text":self.final_content}));
            }

            // `toolsEnabled` controls whether the model may call tools. It is not an
            // execution permission for the Rust harness; harness nodes use the same
            // approval, cancellation and budget policy as model tool calls.
            let harness_messages = if self.request.tools_execution_enabled && !self.finished {
                run_harness_nodes(
                    self.state,
                    self.tx,
                    &self.session,
                    &self.request,
                    self.session_id,
                    self.execution_id,
                    &self.strategy_candidates,
                    metrics,
                    &mut self.budget,
                    &mut self.op_seq,
                    self.generation,
                )
                .await?
            } else {
                Vec::new()
            };
            self.messages.extend(harness_messages);

            Ok(if self.finished {
                Flow::Complete
            } else {
                Flow::Continue
            })
        })
    }
    fn wants_plan(&self) -> bool {
        self.acceptance.is_some()
            && self.request.tools_enabled
            && self.source_message_id.is_none()
            && self.plan_requested
    }
    fn plan<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            self.metrics.lock().unwrap().model_calls += 1;
            if let Some(plan) = action_plan(
                self.state,
                &self.request,
                self.acceptance
                    .as_ref()
                    .map(|(text, _)| text.as_str())
                    .unwrap_or(""),
            )
            .await
            {
                // Persist the plan and its semantic score so policy experiments can
                // compare planning quality instead of only file success.
                if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                    execution["plan"] = json!(plan.chars().take(4_000).collect::<String>());
                    execution["planDigest"] = json!(crate::engine::memory::stable_id(&[&plan]));
                    execution["planQuality"] =
                        crate::engine::plan_quality::score(&plan, &self.request.input);
                    execution["planQualityIndependent"] =
                        crate::engine::plan_quality::score_independent(&plan, &self.request.input);
                    if let Some(policy) = &self.planning_policy {
                        execution["planPolicyId"] = policy["id"].clone();
                        execution["planPolicyRevision"] = policy["revision"].clone();
                    }
                    self.state.store.put("execution", &execution);
                }
                self.messages.insert(1, json!({"role":"system","content":format!("Internal execution plan. Verify every step with real tools, never claims:\n{plan}")}));
                return Ok(Some(plan));
            }
            Ok(None)
        })
    }
    fn prepare_model<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Vec<Value>, String>> {
        Box::pin(async move {
            self.apply_owner_directives().await?;
            let metrics = self.metrics;
            let max_tools = self.max_tools;

            if self.budget.expired() {
                return Err("실행 시간 제한(10분)을 초과했습니다.".into());
            }
            if self.state.is_cancelled(self.session_id)
                || self.state.cancellation_generation(self.session_id) != self.generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            if self.started_at.elapsed() > self.elapsed_limit {
                return Err("실행 시간 제한(10분)을 초과했습니다.".into());
            }
            if self.metrics.lock().unwrap().tool_calls as usize >= self.max_tools {
                mark_budget_exhausted(self.state, self.execution_id);
                return Err(format!("도구 호출 제한({max_tools}회)을 초과했습니다."));
            }
            // DECIDE: an active durable program supplies the next step; otherwise
            // the model decides. Program steps and model steps share this loop,
            // the same tool gateway and the same checkpoint.
            if !self.program_finished && self.request.tools_execution_enabled {
                if let Some(policy) = &self.program_policy {
                    if let Some(program) = policy["parameters"]["program"].as_object() {
                        let program = crate::engine::program::validate(&json!(program))?;
                        restore_program_state(
                            self.state,
                            self.task_id,
                            self.execution_id,
                            &program,
                        )?;
                        let program_messages = run_program(
                            self.state,
                            self.tx,
                            &self.session,
                            &self.request,
                            self.session_id,
                            self.execution_id,
                            &program,
                            metrics,
                            &mut self.budget,
                            &mut self.op_seq,
                            self.generation,
                        )
                        .await?;
                        self.messages.extend(program_messages);
                        let status = self
                            .state
                            .store
                            .get("execution", self.execution_id)
                            .and_then(|execution| {
                                execution["programCheckpoint"]["status"]
                                    .as_str()
                                    .map(str::to_string)
                            });
                        self.program_finished = status.as_deref() == Some("completed");
                    }
                }
            }

            {
                let chars: usize = self
                    .messages
                    .iter()
                    .filter_map(|message| message["content"].as_str())
                    .map(str::len)
                    .sum();
                if chars > self.provider.context_tokens.unwrap_or(16_384) as usize * 4 {
                    if !self.context_overflow {
                        self.context_overflow = true;
                        if let Some(mut execution) =
                            self.state.store.get("execution", self.execution_id)
                        {
                            execution["contextOverflow"] = json!(true);
                            self.state.store.put("execution", &execution);
                        }
                    }
                }
            }
            self.model_started = std::time::Instant::now();
            self.model_offset_ms = self.turn_started.elapsed().as_millis() as u64;
            self.round_tools = if self.force_final {
                Vec::new()
            } else {
                self.tool_definitions.clone()
            };
            if !self.round_tools.is_empty() {
                let tool_schema_budget = self
                    .provider
                    .context_tokens
                    .map(|tokens| {
                        let reserve = self
                            .provider
                            .max_output_tokens
                            .unwrap_or(tokens / 8)
                            .min(tokens / 4)
                            .max(128);
                        ((tokens.saturating_sub(reserve) as usize) * 4 / 2).clamp(4_000, 60_000)
                    })
                    .unwrap_or(60_000);
                self.round_tools = trim_tool_definitions(
                    std::mem::take(&mut self.round_tools),
                    tool_schema_budget,
                );
            }
            self.wire_messages = self.messages.clone();
            if self.is_room {
                if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                    execution["inputContext"]["preparedInputTokensEstimate"] =
                        json!(crate::engine::chat::room_context::input_estimate(
                            &self.wire_messages,
                            &self.round_tools
                        ));
                    execution["inputContext"]["preProviderDigest"] =
                        json!(crate::engine::memory::stable_id(&[
                            &json!({"messages":self.wire_messages,"tools":self.round_tools})
                                .to_string()
                        ]));
                    self.state.store.put("execution", &execution);
                }
            }
            Ok(self.round_tools.clone())
        })
    }
    fn model<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Self::Reply, String>> {
        Box::pin(async move {
            let round = ctx.round;
            {
                let mut counters = self.metrics.lock().unwrap();
                counters.model_calls += 1;
                counters.iterations = round as u32 + 1;
            }
            let completion_result = stream_completion(
                self.state,
                &self.client,
                &self.provider,
                &self.wire_messages,
                &self.round_tools,
                &self.request,
                self.session_id,
                self.tx,
                self.allow_provider_fallback,
            )
            .await;
            {
                let mut counters = self.metrics.lock().unwrap();
                counters.model_elapsed_ms += self.model_started.elapsed().as_millis() as u64;
                if counters.first_token_ms.is_none() {
                    if let Ok((completion, _effective_provider)) = &completion_result {
                        counters.first_token_ms = completion
                            .first_delta_ms
                            .map(|ms| self.model_offset_ms + ms);
                    }
                }
            }

            completion_result
        })
    }
    fn observe_model<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
        reply: Self::Reply,
    ) -> BoxFuture<'a, Result<ModelDecision, String>> {
        Box::pin(async move {
            let (mut completion, selected_provider) = reply;
            if selected_provider.id != self.provider.id {
                // The streamed response came from a fallback route; later rounds
                // must continue on that route rather than retrying the dead one.
                if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                    execution["effectiveProviderId"] = json!(selected_provider.id);
                    execution["effectiveModel"] = json!(selected_provider.model);
                    execution["failoverAt"] = json!(chrono::Utc::now().to_rfc3339());
                    self.state
                        .store
                        .put("execution", &execution)
                        .ok_or("대체 모델을 실행 기록에 저장할 수 없습니다.")?;
                }
                if let Some(agent_id) = self.request.agent_id.as_deref() {
                    crate::engine::router::rotate_agent_route(
                        self.state,
                        agent_id,
                        &selected_provider.id,
                        &selected_provider.model,
                    );
                }
                self.provider = selected_provider;
            }

            let has_dsml_call = completion.content.contains("invoke name=\"")
                && completion.content.contains("parameter name=\"");
            if (!self.request.tools_enabled || !self.request.tools_execution_enabled)
                && (!completion.tool_calls.is_empty() || has_dsml_call)
            {
                return Err("도구가 비활성인 대화에서 모델이 도구 실행을 요청했습니다.".into());
            }
            self.metadata["continuity"] = completion.continuity.clone().unwrap_or(Value::Null);
            self.continuation_projection = completion.projection.take();
            if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                execution["modelContinuity"] = self.metadata["continuity"].clone();
                if let Some(estimate) =
                    self.metadata["continuity"]["workingView"]["estimatedInputTokens"].as_u64()
                {
                    execution["inputContext"]["projectedInputTokensEstimate"] = json!(estimate);
                }
                self.state.store.put("execution", &execution);
            }

            if !completion.reasoning.is_empty() {
                self.final_reasoning.push_str(&completion.reasoning);
            }
            self.last_reasoning = completion.reasoning.clone();
            // Intermediate preambles are stored with their tool-call message, so
            // the final message keeps only the last substantive answer.
            // Exact provider usage when available; otherwise estimate from the
            // prompt and the streamed output so the UI always shows a number.
            let (call_prompt, call_completion) = if completion.usage_exact {
                (completion.prompt_tokens, completion.completion_tokens)
            } else {
                self.usage_estimated = true;
                (
                    self.metadata["continuity"]["workingView"]["estimatedInputTokens"]
                        .as_u64()
                        .unwrap_or_else(|| {
                            crate::engine::chat::room_context::input_estimate(
                                &self.wire_messages,
                                &self.round_tools,
                            ) as u64
                        }),
                    crate::engine::chat::room_context::estimate(&format!(
                        "{}{}",
                        completion.content, completion.reasoning
                    ))
                    .max(1) as u64,
                )
            };
            self.prompt_tokens_total += call_prompt;
            self.completion_tokens_total += call_completion;
            if completion.usage_exact {
                if let Some(cached) = completion.cached_prompt_tokens {
                    self.cached_prompt_tokens_total = self
                        .cached_prompt_tokens_total
                        .saturating_add(cached.min(call_prompt));
                } else {
                    self.cache_reporting_complete = false;
                }
            } else {
                self.cache_reporting_complete = false;
            }
            let call_total = if completion.usage > 0 {
                completion.usage
            } else {
                call_prompt + call_completion
            };
            self.total_tokens += call_total;
            if self.is_room {
                if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                    execution["inputContext"]["providerPromptTokens"] =
                        json!(self.prompt_tokens_total);
                    execution["inputContext"]["providerCachedPromptTokens"] =
                        if self.cache_reporting_complete {
                            json!(self.cached_prompt_tokens_total)
                        } else {
                            Value::Null
                        };
                    execution["inputContext"]["providerNewPromptTokens"] =
                        if self.cache_reporting_complete {
                            json!(self
                                .prompt_tokens_total
                                .saturating_sub(self.cached_prompt_tokens_total))
                        } else {
                            Value::Null
                        };
                    execution["inputContext"]["providerOutputTokens"] =
                        json!(self.completion_tokens_total);
                    execution["inputContext"]["providerUsageExact"] = json!(!self.usage_estimated);
                    self.state.store.put("execution", &execution);
                }
            }
            self.metrics.lock().unwrap().tokens = self.total_tokens;
            if self.total_tokens > MAX_TOKENS
            {
                return Err(format!(
                    "토큰 예산({MAX_TOKENS})을 초과했습니다. 요청을 나눠 다시 시도하세요."
                ));
            }

            if self.owner_changed()? {
                self.state.store.audit(
                    "job.stale_model_reply",
                    json!({"executionId":self.execution_id,"revision":self.owner_revision}),
                );
                return Ok(ModelDecision::Continue);
            }
            if self.force_final {
                // The deterministic checks already passed. Ignore any late tool
                // request and let the model produce the final answer.
                completion.tool_calls.clear();
            }

            if has_dsml_call && self.round_tools.is_empty() {
                return Err("모델이 이번 턴에 제공되지 않은 도구를 호출하려 했습니다.".into());
            }
            if completion.tool_calls.is_empty()
                && self.request.tools_enabled
                && (self.provider.tool_mode != "native" || has_dsml_call)
                && !self.force_final
            {
                let known = self
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
            self.completion = Some(completion);
            Ok(decision)
        })
    }
    fn verify<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            if self.owner_changed()? {
                return Ok(Flow::Continue);
            }

            let round = ctx.round;
            let completion = self.completion.take().ok_or("모델 응답이 없습니다.")?;

            {
                let mut operations = self
                    .state
                    .store
                    .execution_operations(self.execution_id)?
                    .into_iter()
                    .filter(|operation| operation["status"].as_str() == Some("completed"))
                    .collect::<Vec<_>>();
                crate::engine::procedure::sort_operations(&mut operations);
                let violations = self
                    .strategy_candidates
                    .iter()
                    .filter(|strategy| {
                        crate::engine::procedure::applies(strategy, &self.request.input, None, None)
                    })
                    .filter_map(|strategy| {
                        let result = crate::engine::procedure::evaluate(
                            strategy,
                            &operations,
                            &self.state.config.workspace,
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
                    if !self.force_final && self.procedure_repairs < self.max_procedure_repairs {
                        self.procedure_repairs += 1;
                        let _ = self.tx.send(json!({
                            "type": "procedure_retry",
                            "violations": violations,
                        }));
                        self.messages.push(json!({
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
            self.final_content = if self.force_final {
                // Never save a raw tool-call fragment as the answer.
                let (_, cleaned) = parse_text_tool_calls(&completion.content);
                cleaned
            } else {
                completion.content
            };

            // Ordinary-chat evidence gate: a model cannot complete an action
            // request by claiming success without successful tool results.
            if let Some((acceptance_text, checks)) = &self.acceptance {
                let evidence = self
                    .reply_to
                    .as_deref()
                    .map(|source| {
                        evidence_for_source(self.state, self.session_id, source, self.execution_id)
                    })
                    .unwrap_or_default();
                let mut gaps =
                    verification::check_acceptance(self.state, acceptance_text, checks, &evidence);
                let progress = crate::engine::recovery::acceptance_progress(
                    self.state, self.execution_id, round, &gaps, &evidence,
                )?;
                let mut next_action = progress["nextAction"]
                    .as_str().unwrap_or("").to_string();
                if !gaps.is_empty() {
                    // Independent reviewer: deterministic checks still win, but
                    // the reviewer supplies a concrete repair strategy.
                    let precise_replacement = checks.as_array().is_some_and(|checks| {
                        checks
                            .iter()
                            .any(|check| check["type"] == "file_replace_exact")
                    });
                    let review = if precise_replacement {
                        None
                    } else {
                        self.metrics.lock().unwrap().model_calls += 1;
                        dominion_core::engine::model_call(
                            &mut ctx.snapshot,
                            ctx.journal,
                            "acceptance_review",
                            review_action(
                                self.state,
                                &self.request,
                                acceptance_text,
                                &self.final_content,
                                &evidence,
                                &gaps,
                            ),
                        )
                        .await?
                    };
                    if self.owner_changed()? {
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
                        }
                        if !next_action.trim().is_empty() {
                            let scope = crate::engine::memory::MemoryScope::for_session(
                                &self.state.store,
                                self.session_id,
                                self.request.agent_id.as_deref(),
                            );
                            let mut evidence_ids = evidence
                                .iter()
                                .filter_map(|operation| {
                                    operation["id"].as_str().map(str::to_string)
                                })
                                .take(8)
                                .collect::<Vec<_>>();
                            if evidence_ids.is_empty() {
                                evidence_ids.push(self.execution_id.to_string());
                            }
                            let _ = crate::engine::improve::propose(
                                self.state,
                                &scope,
                                "strategy",
                                &next_action,
                                &format!("리뷰어가 실패 원인을 분석해 제안: {}", gaps.join("; ")),
                                &evidence_ids,
                                None,
                                "shared",
                            );
                        }
                        self.state.store.audit(
                            "acceptance.review",
                            json!({
                                "sessionId": self.session_id,
                                "executionId": self.execution_id,
                                "gaps": gaps,
                                "nextAction": next_action,
                            }),
                        );
                    }
                    if !self.request.tools_enabled {
                        return Err(format!(
                            "도구가 비활성 상태라 요청한 작업을 실제로 수행할 수 없습니다: {}",
                            gaps.join("; ")
                        ));
                    }
                    if self.acceptance_retries < self.max_acceptance_retries
                        && round + 1 < self.max_rounds
                        && self.budget.can_execute_tool()
                    {
                        self.acceptance_retries += 1;
                        let _ = self.tx.send(json!({
                            "type": "acceptance_retry",
                            "attempt": self.acceptance_retries,
                            "gaps": gaps,
                            "progress": progress,
                        }));
                        self.state.store.audit(
                            "acceptance.retry",
                            json!({
                                "sessionId": self.session_id,
                                "executionId": self.execution_id,
                                "round": round,
                                "gaps": gaps,
                            }),
                        );
                        self.messages.push(json!({
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
                    self.state.store.audit(
                        "acceptance.failed",
                        json!({
                            "sessionId": self.session_id,
                            "executionId": self.execution_id,
                            "gaps": gaps,
                        }),
                    );
                    return Err(format!(
                        "실행 증거가 부족하여 완료하지 못했습니다: {}",
                        gaps.join("; ")
                    ));
                }
                self.acceptance_passed = true;
                self.state.store.audit(
                    "acceptance.passed",
                    json!({"sessionId": self.session_id, "executionId": self.execution_id, "round": round}),
                );
            }

            return Ok(Flow::Complete);
        })
    }
    fn begin_tools<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let mut completion = self.completion.take().ok_or("모델 응답이 없습니다.")?;

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
            self.messages.push(json!({
                "role": "assistant",
                "content": completion.content,
                "reasoning_content": completion.reasoning,
                "tool_calls": tool_calls_json,
            }));
            // Persist the tool-call chain so a restart keeps the exact context the
            // model saw instead of only the final answer.
            let saved = self
                .state
                .store
                .write_message(&json!({
                    "session_id": self.session_id,
                    "role": "assistant",
                    "content": completion.content,
                    "reasoning": completion.reasoning,
                    "agent_id": self.request.agent_id,
                    "tool_calls": tool_calls_json,
                    "metadata": self.metadata,
                }))
                .ok_or("도구 호출 메시지를 저장할 수 없습니다.")?;
            self.state.bus.message(&saved);

            let ordering = crate::engine::algorithms::choose_for_agent(
                self.state,
                "execution.ordering",
                self.request.agent_id.as_deref(),
                Some(self.execution_id),
            );
            let ordering_kernel = ordering
                .as_ref()
                .and_then(|algorithm| algorithm["implementation"].as_str())
                .unwrap_or("as-written");
            // Native provider calls retain their declared order. Effects are barriers;
            // the shared Rust kernel only batches adjacent independent observations.
            self.round_calls = completion.tool_calls.clone();
            if ordering_kernel == "read-first"
                && self
                    .round_calls
                    .iter()
                    .any(|call| !tool_is_read_only(&call.name))
            {
                crate::engine::algorithms::mark_unrated(
                    self.state,
                    self.execution_id,
                    "execution.ordering",
                    "effect ordering barrier preserved",
                );
            }
            let combination = crate::engine::algorithms::choose_for_agent(
                self.state,
                "tool.combination",
                self.request.agent_id.as_deref(),
                Some(self.execution_id),
            );
            self.parallel_reads = combination
                .as_ref()
                .and_then(|algorithm| algorithm["implementation"].as_str())
                == Some("parallel-reads");
            self.dispatched = std::collections::HashMap::new();
            self.ordering_used = false;
            self.ordering_ok = true;

            Ok(self.round_calls.len())
        })
    }
    fn prepare_tool<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
        call_index: usize,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            if self.owner_changed()? {
                return Ok(false);
            }

            let call = self.round_calls[call_index].clone();

            // A disconnected SSE client must not stop work already promised by
            // the model; explicit cancel and budgets remain the stop signals.
            if !self.dispatched.contains_key(&call_index) && !self.budget.charge_proposal() {
                return Err(format!(
                    "도구 제안 예산({}회)을 초과했습니다.",
                    self.max_tools.saturating_mul(2)
                ));
            }
            if self.state.is_cancelled(self.session_id)
                || self.state.cancellation_generation(self.session_id) != self.generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            let arguments = match serde_json::from_str::<Value>(&call.arguments) {
                Ok(value) if value.is_object() => value,
                _ => {
                    mark_tool_argument_signal(self.state, &self.request, self.execution_id, false);
                    let error = format!(
                        "invalid tool arguments: {} 도구 인수가 올바른 JSON 객체가 아닙니다. 인자를 고쳐 다시 호출하세요.",
                        call.name
                    );
                    record_soft_tool_error(
                        self.state,
                        self.tx,
                        self.session_id,
                        &self.request,
                        self.execution_id,
                        &call.name,
                        &call.id,
                        &error,
                        self.metrics,
                    );
                    self.messages.push(json!({
                        "role":"tool","tool_call_id":call.id,
                        "content": json!({"error":error}).to_string()
                    }));
                    return Ok(false);
                }
            };
            let completed_operations = self
                .state
                .store
                .execution_operations(self.execution_id)?
                .into_iter()
                .filter(|operation| operation["status"].as_str() == Some("completed"))
                .collect::<Vec<_>>();
            let procedure_violation = self.strategy_candidates.iter().find_map(|strategy| {
                if !crate::engine::procedure::applies(
                    strategy,
                    &self.request.input,
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
                let enforced = self.strategy_candidates.iter().any(|strategy| {
                    crate::engine::procedure::applies(
                        strategy,
                        &self.request.input,
                        Some(&call.name),
                        Some(&arguments),
                    ) && (strategy["enforce"].as_bool() == Some(true)
                        || strategy["verification"]["enforce"].as_bool() == Some(true))
                });
                if enforced {
                    record_soft_tool_error(
                        self.state,
                        self.tx,
                        self.session_id,
                        &self.request,
                        self.execution_id,
                        &call.name,
                        &call.id,
                        &error,
                        self.metrics,
                    );
                    self.messages.push(json!({
                        "role":"tool","tool_call_id":call.id,
                        "content": json!({"error":error,"procedureViolation":true}).to_string()
                    }));
                    return Ok(false);
                }
                self.messages.push(json!({
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
                let tool_selection = crate::engine::algorithms::choose_for_agent(
                    self.state,
                    "tool.selection",
                    self.request.agent_id.as_deref(),
                    Some(self.execution_id),
                )
                .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
                .unwrap_or_else(|| "read-before-write".into());
                if tool_selection == "block-write-without-read" {
                    let error = "도구 선택 알고리즘(block-write-without-read)이 읽기/목록 확인 없는 쓰기를 차단했습니다. workspace_list 또는 workspace_read를 먼저 실행하세요.";
                    record_soft_tool_error(
                        self.state,
                        self.tx,
                        self.session_id,
                        &self.request,
                        self.execution_id,
                        &call.name,
                        &call.id,
                        error,
                        self.metrics,
                    );
                    crate::engine::algorithms::mark_signal(
                        self.state,
                        self.execution_id,
                        "tool.selection",
                        false,
                    );
                    self.messages.push(json!({
                        "role":"tool","tool_call_id":call.id,
                        "content": json!({"error":error,"toolSelectionBlocked":true}).to_string()
                    }));
                    return Ok(false);
                } else if tool_selection == "read-before-write" {
                    self.messages.push(json!({
                        "role":"system",
                        "content":"도구 선택 알고리즘(read-before-write): 쓰기 전에 workspace_list/workspace_read로 대상 상태를 먼저 확인하는 것이 권장됩니다. 이 힌트는 실행을 막지 않습니다."
                    }));
                }
            }
            let call_key = format!("{}:{}", call.name, arguments);
            let current_fingerprint = environment_fingerprint(self.state, &call.name, &arguments);
            let recovery_mode = crate::engine::algorithms::choose_for_agent(
                self.state,
                "recovery.strategy",
                self.request.agent_id.as_deref(),
                Some(self.execution_id),
            )
            .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
            .unwrap_or_else(|| "fingerprint".into());
            let repeat_allowed = match recovery_mode.as_str() {
                "allow-all" => true,
                "block-identical" => false,
                _ => self
                    .failed_calls
                    .get(&call_key)
                    .is_some_and(|(_, fingerprint)| fingerprint != &current_fingerprint),
            };
            if repeat_allowed {
                self.failed_calls.remove(&call_key);
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
                    if let Some((previous_error, _)) = self.failed_calls.get(&call_key) {
                        // Section 11-1: an unchanged repeat of a failed mutating call
                        // is not re-executed. The model must choose a changed action.
                        // A retry is allowed once the environment/state actually changed.
                        let previous_error = previous_error.clone();
                        let error =
                            crate::engine::adaptation_loop::repeated_failure_message(&call.name);
                        record_soft_tool_error(
                            self.state,
                            self.tx,
                            self.session_id,
                            &self.request,
                            self.execution_id,
                            &call.name,
                            &call.id,
                            &error,
                            self.metrics,
                        );
                        self.messages.push(json!({
                            "role":"tool","tool_call_id":call.id,
                            "content": json!({
                                "error": error,
                                "previousError": previous_error,
                                "repeatedFailedCall": true,
                            }).to_string()
                        }));
                        self.messages.push(json!({
                    "role":"system",
                    "content":"직전 실패와 동일한 호출이 차단되었습니다. 다른 도구/인자로 실행하거나, 원인을 확인하는 probe를 선택하거나, 실제로 막혔다면 그 이유를 최종 답변에 쓰세요."
                }));
                        return Ok(false);
                    }
                }
            }
            let repeat = {
                let counter = self.call_counts.entry(call_key.clone()).or_insert(0);
                *counter += 1;
                *counter
            };
            if repeat > 2 {
                // Repeated calls are an observation, not a hard stop. The model
                // sees the same real result and decides whether to change tack.
                self.messages.push(json!({
                    "role":"system",
                    "content": format!(
                        "`{}` 호출이 {}회 반복되었습니다. 같은 결과가 나온다면 다른 접근을 선택하세요.",
                        call.name, repeat
                    )
                }));
            }

            self.prepared_arguments = arguments;
            self.prepared_key = call_key;
            self.completed_operations = completed_operations;
            Ok(true)
        })
    }
    fn dispatch_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        call_index: usize,
    ) -> BoxFuture<'a, Result<Self::ToolResult, String>> {
        Box::pin(async move {
            let round = ctx.round;
            let metrics = self.metrics;
            let max_tools = self.max_tools;
            let call = self.round_calls[call_index].clone();
            let arguments = self.prepared_arguments.clone();
            let (result, ok, tool_content) =
                if let Some(result) = self.dispatched.remove(&call_index) {
                    result?
                } else {
                    if !self.budget.charge_tool() {
                        mark_budget_exhausted(self.state, self.execution_id);
                        return Err(format!("도구 호출 제한({max_tools}회)을 초과했습니다."));
                    }
                    let mut end = call_index + 1;
                    if self.parallel_reads && tool_is_read_only(&call.name) {
                        let candidate_end = dominion_core::hosted::read_batch_end(
                            &self.round_calls,
                            call_index,
                            4,
                            |call| {
                                tool_is_read_only(&call.name)
                                    && dominion_core::journal::parallel_safe(&call.name)
                            },
                        );
                        while end < candidate_end {
                            let next = &self.round_calls[end];
                            if !parallel_read_admitted(
                                next,
                                &self.session,
                                &self.round_tools,
                                &self.strategy_candidates,
                                &self.request.input,
                                &self.completed_operations,
                            ) || !self.budget.can_execute_tool()
                            {
                                break;
                            }
                            if !self.budget.charge_proposal() || !self.budget.charge_tool() {
                                break;
                            }
                            end += 1;
                        }
                    }
                    let mut prepared = Vec::new();
                    for index in call_index..end {
                        self.op_seq += 1;
                        let parsed = if index == call_index {
                            arguments.clone()
                        } else {
                            serde_json::from_str(&self.round_calls[index].arguments)
                                .map_err(|error| format!("도구 인수 해석 실패: {error}"))?
                        };
                        prepared.push((index, self.op_seq, parsed));
                    }
                    ctx.snapshot.step = round + 1;
                    ctx.snapshot.model_calls = self.metrics.lock().unwrap().model_calls as usize;
                    ctx.snapshot.charged_tokens = self.total_tokens;
                    let actions = prepared
                        .iter()
                        .map(|(index, _, args)| dominion_core::hosted::Action {
                            id: format!("{round}:{}", self.round_calls[*index].id),
                            tool: self.round_calls[*index].name.clone(),
                            arguments: args.clone(),
                            depends_on: vec![],
                        })
                        .collect::<Vec<_>>();
                    let dispatcher = ChatKernelDispatcher {
                        state: self.state,
                        tx: self.tx,
                        session: &self.session,
                        request: &self.request,
                        session_id: self.session_id,
                        execution_id: self.execution_id,
                        metrics,
                        prepared: &prepared,
                        calls: &self.round_calls,
                        round,
                        compat: self.provider.tool_mode == "compat",
                        definitions: &self.round_tools,
                    };
                    let results = dominion_core::hosted::dispatch_batch(
                        &mut ctx.snapshot,
                        &actions,
                        &dispatcher,
                        ctx.journal,
                    )
                    .await?;
                    if prepared.len() > 1 {
                        crate::engine::algorithms::mark_signal(
                            self.state,
                            self.execution_id,
                            "tool.combination",
                            results
                                .iter()
                                .all(|result| result.as_ref().is_ok_and(|(_, ok, _)| *ok)),
                        );
                    }
                    for ((index, _, _), result) in prepared.into_iter().zip(results) {
                        self.dispatched.insert(index, result);
                    }
                    self.dispatched
                        .remove(&call_index)
                        .ok_or("도구 배치 결과가 없습니다.")??
                };

            Ok((result, ok, tool_content))
        })
    }
    fn observe_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        call_index: usize,
        observed: Self::ToolResult,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let round = ctx.round;
            let call = self.round_calls[call_index].clone();
            let arguments = self.prepared_arguments.clone();
            let call_key = self.prepared_key.clone();
            let (result, ok, tool_content) = observed;
            if self.owner_changed()? {
                self.messages
                    .push(json!({"role":"tool","tool_call_id":call.id,"content":tool_content}));
                return Ok(());
            }

            self.ordering_used = true;
            if !ok {
                self.ordering_ok = false;
            }
            crate::engine::algorithms::mark_signal(
                self.state,
                self.execution_id,
                "tool.selection",
                ok,
            );
            crate::engine::algorithms::mark_signal(
                self.state,
                self.execution_id,
                "recovery.strategy",
                ok,
            );
            if ok {
                self.failed_calls.remove(&call_key);
                if matches!(
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
                ) {
                    self.environment_version += 1;
                }
            } else if result["preflightBlocked"] != true && result["invalidWorkspacePath"] != true {
                let error_text = result["error"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| tool_content.chars().take(600).collect::<String>());
                self.failed_calls.insert(
                    call_key.clone(),
                    (
                        error_text.clone(),
                        environment_fingerprint(self.state, &call.name, &arguments),
                    ),
                );
                crate::engine::adaptation_loop::record_tool_failure(
                    self.state,
                    self.execution_id,
                    round,
                    &call.id,
                    &call.name,
                    &arguments,
                    &error_text,
                );
                if self.adaptation_attempts < 3
                    && !self.request.provider_id.as_deref().unwrap_or("").is_empty()
                {
                    self.adaptation_attempts += 1;
                    match attempt_model_adaptation(
                        self.state,
                        self.tx,
                        &self.session,
                        &self.request,
                        self.session_id,
                        self.execution_id,
                        round,
                        &call.id,
                        &call.name,
                        &arguments,
                        &error_text,
                        self.metrics,
                        &mut self.budget,
                        &mut self.op_seq,
                        self.environment_version,
                    )
                    .await
                    {
                        Ok(adapted) => {
                            let selected = &adapted["selected"];
                            let result = &adapted["result"];
                            self.messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "적응 실행: 선택된 개입 `{}` (tool={}) 결과={}. 가설 판정={}. 같은 실패 호출을 반복하지 마세요.",
                                selected["id"], selected["tool"], result, adapted["hypothesisResults"]
                            )
                        }));
                            if tool_succeeded(result)
                                && matches!(
                                    selected["tool"].as_str(),
                                    Some(
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
                                    )
                                )
                            {
                                self.environment_version += 1;
                                self.adaptation_failures = 0;
                            } else {
                                self.adaptation_failures += 1;
                            }
                        }
                        Err(error) => {
                            record_adaptation_error(self.state, self.execution_id, &error);
                            self.messages.push(json!({
                                "role":"system",
                                "content": format!(
                                    "적응 후보 생성/실행에 실패했습니다: {error}. 모델은 원인 가설과 다른 도구/인자를 직접 선택하세요."
                                )
                            }));
                        }
                    }
                }
                if self.adaptation_failures >= 2 {
                    self.force_final = true;
                    self.messages.push(json!({
                        "role":"system",
                        "content":"연속 적응 실행이 진전 없이 실패했습니다. 추가 반복을 중단하고 실제 관측과 막힌 원인을 최종 답변에 쓰세요."
                    }));
                }
            }
            // Keep full evidence in the in-memory turn. Per-provider
            // request assembly clips only when *that model's* configured
            // context is too small, so switching local -> API can recover it.
            self.messages.push(json!({
                "role":"tool","tool_call_id":call.id,"content":tool_content,
            }));

            Ok(())
        })
    }
    fn verify_tools<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            let round = ctx.round;

            if self.ordering_used {
                crate::engine::algorithms::mark_signal(
                    self.state,
                    self.execution_id,
                    "execution.ordering",
                    self.ordering_ok,
                );
            }
            // Check acceptance after the tool round, not only when the model
            // decides to stop. A model that keeps verifying can otherwise burn the
            // whole round budget after the work is already complete.
            if !self.force_final {
                if let Some((acceptance_text, checks)) = &self.acceptance {
                    let evidence = self
                        .reply_to
                        .as_deref()
                        .map(|source| {
                            evidence_for_source(
                                self.state,
                                self.session_id,
                                source,
                                self.execution_id,
                            )
                        })
                        .unwrap_or_default();
                    let current_gaps = verification::check_acceptance(
                        self.state, acceptance_text, checks, &evidence,
                    );
                    let progress = crate::engine::recovery::acceptance_progress(
                        self.state, self.execution_id, round, &current_gaps, &evidence,
                    )?;
                    if progress["mode"] == "reconsider" && !self.force_final {
                        let _ = self.tx.send(json!({
                            "type":"acceptance_replan",
                            "progress": progress,
                        }));
                        self.messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "Execution verifier detected stalled progress. {}",
                                progress["nextAction"].as_str().unwrap_or("")
                            )
                        }));
                    }
                    if current_gaps.is_empty() && !evidence.is_empty() {
                        self.acceptance_passed = true;
                        self.force_final = true;
                        self.state.store.audit(
                        "acceptance.passed",
                        json!({"sessionId": self.session_id, "executionId": self.execution_id, "round": round}),
                    );
                        self.messages.push(json!({
                        "role": "system",
                        "content": "모든 검증 조건이 충족되었습니다. 추가 도구 호출 없이 최종 답변만 작성하세요."
                    }));
                        let _ = self.tx.send(json!({"type": "acceptance_passed"}));
                        if let Some(path) = self.simple_creation_target.as_ref().filter(|path| {
                            std::fs::metadata(path).is_ok_and(|file_metadata| {
                                file_metadata.is_file() && file_metadata.len() == 0
                            }) && !self.strategy_candidates.iter().any(|strategy| {
                                strategy["verification"]["type"] == "procedure"
                                    && crate::engine::procedure::applies(
                                        strategy,
                                        &self.request.input,
                                        None,
                                        None,
                                    )
                            })
                        }) {
                            // The native action has already finished and the host
                            // verified the exact owner-bound artifact. Reporting
                            // that fact does not require another paid model turn.
                            self.final_content =
                                format!("파일을 만들었습니다: `{}`", path.display());
                            self.metadata["completionSource"] = json!("verified_artifact");
                            // This provider projection predates the just-finished
                            // tool exchange. Let the next turn derive that full
                            // exchange from the durable transcript before adding
                            // this host-generated summary to the working view.
                            self.continuation_projection = None;
                            let _ = self
                                .tx
                                .send(json!({"type":"text","text":self.final_content}));
                            return Ok(Flow::Complete);
                        }
                    }
                }
            }

            Ok(Flow::Continue)
        })
    }
    fn exhausted(&mut self) -> Result<(), String> {
        if !self.acceptance_passed {
            return Err(format!(
                "모델 반복 제한({}회)에 도달하여 실행을 마치지 못했습니다.",
                self.max_rounds
            ));
        }
        if self.final_content.trim().is_empty() {
            self.final_content =
                "요청한 작업을 실제 도구로 수행했고 검증 조건을 충족했습니다.".into();
        }
        Ok(())
    }
    fn confirm_completion(&mut self) -> Result<Flow, String> {
        if let Some(owner) = &self.owner {
            if !owner.close(self.state, self.owner_revision)? {
                return Ok(Flow::Continue);
            }
        }
        Ok(Flow::Complete)
    }
    fn completion(&self) -> Result<(String, usize, u64), String> {
        if self.final_content.trim().is_empty() && self.final_reasoning.trim().is_empty() {
            return Err("모델이 빈 응답을 반환했습니다.".into());
        }
        Ok((
            self.final_content.clone(),
            self.metrics.lock().unwrap().model_calls as usize,
            self.total_tokens,
        ))
    }
}
impl MainTurn<'_> {
    fn owner_changed(&self) -> Result<bool, String> {
        self.owner
            .as_ref()
            .map(|owner| {
                owner
                    .revision(self.state)
                    .map(|revision| revision != self.owner_revision)
            })
            .unwrap_or(Ok(false))
    }
    async fn apply_owner_directives(&mut self) -> Result<(), String> {
        let Some(owner) = self.owner.clone() else {
            return Ok(());
        };
        let row = owner.record(self.state)?;
        let revision = row["steeringRevision"].as_u64().unwrap_or(0);
        if revision == self.owner_revision {
            if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                execution["ownerControl"] =
                    json!({"kind":owner.kind,"id":owner.id,"revision":revision});
                self.state
                    .store
                    .put("execution", &execution)
                    .ok_or("작업 지시 권한을 저장할 수 없습니다.")?;
            }
            return Ok(());
        }
        let base = if owner.kind == "job" {
            row["input"]["prompt"].as_str()
        } else {
            row["instruction"].as_str()
        }
        .ok_or("원래 작업 지시가 없습니다.")?;
        let goal = crate::engine::job_control::intent(base, &row);
        let updates = row["steering"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["revision"].as_u64().unwrap_or(0) > self.owner_revision)
            .collect::<Vec<_>>();
        let replaced = updates.iter().any(|d| d["mode"] == "replace");
        let mut checks = if replaced {
            prepare_owner_action(self.state, &goal)
                .map(|(_, c)| c)
                .unwrap_or(json!([]))
        } else {
            self.acceptance
                .as_ref()
                .map(|(_, c)| c.clone())
                .unwrap_or(json!([]))
        };
        if !replaced {
            for directive in &updates {
                if let Some((_, extra)) = directive["instruction"]
                    .as_str()
                    .and_then(|text| prepare_owner_action(self.state, text))
                {
                    for check in extra.as_array().into_iter().flatten() {
                        if !checks.as_array().unwrap().contains(check) {
                            checks.as_array_mut().unwrap().push(check.clone());
                        }
                    }
                }
            }
        }
        if let Some(command) = self.session["nativeSdkVerifyCommand"]
            .as_str()
            .or_else(|| self.session["nativeJobVerifyCommand"].as_str())
        {
            let check = json!({"type":"command_succeeded","command":command});
            if !checks.as_array().unwrap().contains(&check) {
                checks.as_array_mut().unwrap().push(check);
            }
        }
        if let Some(items) = checks.as_array_mut() {
            items.retain(|check| {
                !(check["type"] == "file_replace_exact"
                    && check["expectedSha256"].as_str().is_none())
            });
        }
        let mut last_source = None;
        for directive in &updates {
            let id = format!(
                "owner-steer-{}-{}-{}",
                owner.kind,
                owner.id,
                directive["revision"].as_u64().unwrap_or(0)
            );
            let message = if let Some(saved) = self.state.store.message(&id) {
                saved
            } else {
                let saved=self.state.store.write_message(&json!({"id":id,"session_id":self.session_id,"role":"user",
                    "content":directive["instruction"],"metadata":{"authorKind":"owner","ownerControl":owner.id,
                        "ownerRevision":directive["revision"],"mode":directive["mode"]}})).ok_or("사용자 지시를 저장할 수 없습니다.")?;
                self.state.bus.message(&saved);
                saved
            };
            last_source = message["id"].as_str().map(str::to_owned);
        }
        let epoch = if replaced {
            self.state
                .store
                .execution_operations(self.execution_id)?
                .into_iter()
                .filter_map(|op| op["opSeq"].as_u64())
                .max()
                .unwrap_or(0)
        } else {
            self.state
                .store
                .get("execution", self.execution_id)
                .and_then(|e| e["ownerEvidenceEpoch"].as_u64())
                .unwrap_or(0)
        };
        let old_revision = self.owner_revision;
        let mut execution = self
            .state
            .store
            .get("execution", self.execution_id)
            .ok_or("실행 기록이 없습니다.")?;
        execution["ownerControl"] = json!({"kind":owner.kind,"id":owner.id,"revision":revision});
        execution["ownerEvidenceEpoch"] = json!(epoch);
        execution["ownerGoalChanged"] = json!(true);
        execution["acceptanceText"] = json!(goal);
        execution["acceptanceChecks"] = checks.clone();
        if replaced {
            execution["programCheckpoint"]["status"] = json!("superseded_by_owner");
        }
        self.state.store.mutate_record("task_episode",self.task_id,json!({"id":self.task_id}),|episode|{
            let previous=episode["contract"].clone();
            let mut contract:crate::engine::adaptation::types::TaskContract=serde_json::from_value(previous.clone()).map_err(|e|e.to_string())?;
            contract.intent=bounded_contract_intent(&goal);contract.mandatory=checks.as_array().cloned().unwrap_or_default();
            contract.revision=crate::engine::adaptation::types::revision_hash(&json!({"previous":previous,"ownerKind":owner.kind,"ownerId":owner.id,"ownerRevision":revision,"intent":contract.intent,"checks":checks}));
            contract.validate()?;
            let mut history=episode["ownerRevisionHistory"].as_array().cloned().unwrap_or_default();
            history.push(json!({"contract":previous,"ownerRevision":old_revision}));episode["ownerRevisionHistory"]=json!(history);
            episode["contract"]=serde_json::to_value(&contract).map_err(|e|e.to_string())?;
            episode["ownerText"]=json!(goal);episode["rootOutcome"]=json!("running");
            if let Some(source)=last_source.as_deref(){episode["sourceMessageId"]=json!(source);}
            execution["taskContractRevision"]=json!(contract.revision);
            Ok(vec![("execution".into(),execution)])
        })?;
        if replaced {
            self.reply_to = last_source;
            self.program_finished = true;
        }
        self.request.input = goal.clone();
        self.acceptance = if checks.as_array().is_some_and(|c| !c.is_empty()) {
            Some((goal.clone(), checks))
        } else {
            None
        };
        self.acceptance_passed = false;
        self.force_final = false;
        self.finished = false;
        self.final_content.clear();
        self.acceptance_retries = 0;
        self.procedure_repairs = 0;
        self.simple_creation_target = simple_file_creation(self.state, &goal, &self.acceptance);
        self.messages.push(json!({"role":"user","content":format!("사용자가 작업 지시를 갱신했습니다 (revision={revision}). 현재 유효한 전체 목표:\n{goal}\n이미 수행한 변경은 보존하고 새 목표의 검증 조건으로 계속하세요.")}));
        if self.request.tools_enabled && self.request.tools_execution_enabled {
            self.tool_definitions = tools::definitions();
            self.tool_definitions
                .extend(mcp::definitions(self.state).await);
            if self.session["readOnly"] == true {
                self.tool_definitions
                    .retain(|tool| tools::observational(tool_name(tool)));
            }
        }
        self.metadata.as_object_mut().unwrap().remove("completionSource");
        self.metadata["reasoningLanguageEffective"]=json!(self.request.effective_reasoning_language());
        self.continuation_projection=None;
        let mut position=0;
        self.messages.retain(|message|{
            let at=position;position+=1;
            let text=message["content"].as_str().unwrap_or("");
            !(at>0&&message["role"]=="system"&&(
                text.starts_with("모든 검증 조건이 충족되었습니다.")
                ||text.starts_with("연속 적응 실행이 진전 없이 실패했습니다.")
                ||(replaced&&(text.starts_with("Internal execution plan.")||text.starts_with("Engine-owned task decomposition.")))))
        });
        if self.session["delegationWorkspace"].is_object(){self.tool_definitions.retain(|tool|!tool_name(tool).starts_with("mcp__")&&!matches!(tool_name(tool),"github_api"|"github_repositories"|"github_file_read"|"github_file_update"|"github_file_replace"|"browser_control"));}
        self.owner_revision = revision;
        self.state.store.audit("job.owner_revision_applied",json!({"executionId":self.execution_id,"revision":revision,"mode":if replaced{"replace"}else{"append"}}));
        Ok(())
    }
    pub(super) async fn save(&mut self) -> Result<Option<Value>, String> {
        // Preserve provider reasoning as received. Display language requests are
        // best effort; never send private reasoning to a hidden translation API.
        self.metadata["providerReasoning"] = json!(self.last_reasoning);

        if self.final_content.trim().is_empty() && self.final_reasoning.trim().is_empty() {
            return Err("모델이 빈 응답을 반환했습니다.".into());
        }

        if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
            execution["acceptancePassed"] = json!(self.acceptance_passed);
            execution["acceptanceText"] = self
                .acceptance
                .as_ref()
                .map(|(text, _)| json!(text))
                .unwrap_or(Value::Null);
            execution["finalAnswer"] =
                json!(self.final_content.chars().take(600).collect::<String>());
            execution["toolsUsed"] = json!(self
                .call_counts
                .keys()
                .filter_map(|key| key.split(':').next().map(str::to_string))
                .collect::<Vec<_>>());
            execution["budget"] = self.budget.snapshot();
            execution["runtime"] = json!("dominion-core");
            self.state.store.put("execution", &execution);
        }
        // Attach the usage to the message so every client can show what the turn
        // cost without a second lookup.
        self.metadata["usage"] = json!({
            "promptTokens": self.prompt_tokens_total,
            "cachedPromptTokens": if self.cache_reporting_complete { json!(self.cached_prompt_tokens_total) } else { Value::Null },
            "uncachedPromptTokens": if self.cache_reporting_complete { json!(self.prompt_tokens_total.saturating_sub(self.cached_prompt_tokens_total)) } else { Value::Null },
            "completionTokens": self.completion_tokens_total,
            "totalTokens": self.total_tokens,
            "estimated": self.usage_estimated,
            "providerId": self.provider.id,
            "model": self.provider.model,
        });
        let saved = self
            .state
            .store
            .write_message(&json!({
                "session_id": self.session_id,
                "role": "assistant",
                "content": self.final_content,
                "reasoning": self.final_reasoning,
                "agent_id": self.request.agent_id,
                "metadata": self.metadata,
            }))
            .ok_or("최종 응답을 저장할 수 없습니다.")?;
        self.state.bus.message(&saved);
        crate::engine::chat::continuity::commit(self.state, &self.metadata["continuity"], &saved);
        if let Some(projection) = self.continuation_projection.take() {
            crate::engine::chat::continuity::commit_projection(
                self.state,
                &self.metadata["continuity"],
                &saved,
                projection,
            );
        }
        if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
            execution["finalMessageId"] = saved["id"].clone();
            self.state.store.put("execution", &execution);
        }
        Ok(Some(saved))
    }
}
