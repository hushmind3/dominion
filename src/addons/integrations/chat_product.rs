//! Phase-local adapters for SQLite, streaming transports and existing product policies.
//! The native core calls these phases; this module has no model/tool retry loop.
use super::*;
use dominion_algorithms::engine::{Context, Flow, Host, ModelDecision};
use dominion_core::hosted::BoxFuture;
#[path = "chat_product/round_state.rs"]
mod round_state;
pub(super) use round_state::{ModelRound, ToolRound};

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
    pub(super) model_round: ModelRound,
    pub(super) tool_round: ToolRound,
}

/// The provider can complete a charged model call without a visible answer.
/// Classify that after usage has been durably recorded, never as success.
fn terminal_model_error(completion: &crate::engine::chat::Completion) -> Option<&'static str> {
    match completion.finish_reason.as_deref() {
        Some("length") => return Some("모델이 출력 토큰 제한에 도달해 답변이 잘렸습니다."),
        Some("content_filter") => return Some("모델 응답이 콘텐츠 필터로 중단되었습니다."),
        _ => {}
    }
    if completion.content.trim().is_empty() && completion.tool_calls.is_empty() {
        return Some(if completion.reasoning.trim().is_empty() {
            "모델이 사용량/종료 정보만 반환하고 읽을 수 있는 답변을 반환하지 않았습니다."
        } else {
            "모델이 추론만 반환하고 최종 답변을 반환하지 않았습니다."
        });
    }
    None
}

/// A last-turn receipt may be committed after the final tool batch even when
/// no model round remains to describe it. Only the frozen owner-bound checks and
/// durable completed operations can recover that outcome. A file existing by
/// itself, or an assistant claim, never counts as sufficient evidence.
fn terminal_acceptance_receipt(
    state: &AppState,
    session_id: &str,
    source_id: Option<&str>,
    execution_id: &str,
    acceptance: Option<&(String, Value)>,
) -> Option<usize> {
    let (goal, checks) = acceptance?;
    // An empty inferred contract only proves that some tool was run, not
    // that the requested result is complete. It needs a model final review.
    if checks.as_array().is_none_or(Vec::is_empty) { return None; }
    let source_id = source_id?;
    let evidence = evidence_for_source(state, session_id, source_id, execution_id);
    if evidence.is_empty() || !verification::check_acceptance(state, goal, checks, &evidence).is_empty() {
        return None;
    }
    Some(evidence.len())
}

#[path = "chat_product/model_phase.rs"]
mod model_phase;
#[path = "chat_product/tool_phase.rs"]
mod tool_phase;

type ProviderReply = (crate::engine::chat::Completion, crate::engine::chat::ProviderRef);
type ToolReply = (Value, bool, String);

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
            // Explicit research is an owner instruction with an observable
            // postcondition. The model must not bypass it by writing fluent
            // unsourced prose. Keep ordinary chats on the zero-search path.
            if !self.is_room && !self.request.direct_chat {
                let observations=tool_dispatch::run_required_web_search(
                    self.state,&self.request,self.tx,self.session_id,
                    self.execution_id,&mut self.op_seq,metrics,
                ).await?;
                self.messages.extend(observations);
            }
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
            {
                let mut counters=self.metrics.lock().unwrap();
                counters.model_calls+=1;
                counters.auxiliary_calls+=1;
            }
            if let Some(plan) = action_plan(
                self.state,
                &self.request,
                self.acceptance
                    .as_ref()
                    .map(|(text, _)| text.as_str())
                    .unwrap_or(""),
                self.execution_id,
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
        model_phase::prepare_model(self, _ctx)
    }
    fn model<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Self::Reply, String>> {
        model_phase::model(self, ctx)
    }
    fn observe_model<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
        reply: Self::Reply,
    ) -> BoxFuture<'a, Result<ModelDecision, String>> {
        model_phase::observe_model(self, _ctx, reply)
    }
    fn verify<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        model_phase::verify(self, ctx)
    }
    fn begin_tools<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<usize, String>> {
        tool_phase::begin_tools(self, _ctx)
    }
    fn prepare_tool<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
        call_index: usize,
    ) -> BoxFuture<'a, Result<bool, String>> {
        tool_phase::prepare_tool(self, _ctx, call_index)
    }
    fn dispatch_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        call_index: usize,
    ) -> BoxFuture<'a, Result<Self::ToolResult, String>> {
        tool_phase::dispatch_tool(self, ctx, call_index)
    }
    fn observe_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        call_index: usize,
        observed: Self::ToolResult,
    ) -> BoxFuture<'a, Result<(), String>> {
        tool_phase::observe_tool(self, ctx, call_index, observed)
    }
    fn verify_tools<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        tool_phase::verify_tools(self, ctx)
    }
    fn reconcile_verified<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        tool_phase::reconcile_verified(self, _ctx)
    }
    fn exhausted(&mut self) -> Result<(), String> {
        // A completed file cannot release an uncertain in-flight effect, a
        // superseded owner revision, or an unsatisfied standing procedure.
        let safe_to_close = self.closure_admitted()?;
        if !safe_to_close { self.acceptance_passed = false; }
        // The final model round may finish an effectful tool call with no paid
        // round left to emit the completion prose. Re-read persisted receipts
        // rather than paying a ninth call or treating a mere file as success.
        if !self.acceptance_passed && safe_to_close {
            if let Some(receipts) = terminal_acceptance_receipt(
                self.state, self.session_id, self.reply_to.as_deref(),
                self.execution_id, self.acceptance.as_ref(),
            ) {
                self.acceptance_passed = true;
                self.metadata["completionSource"] = json!("terminal_verified_receipts");
                self.state.store.audit("acceptance.terminal_verified", json!({
                    "executionId": self.execution_id,
                    "sessionId": self.session_id,
                    "verifiedReceiptCount": receipts,
                }));
            }
        }
        if !self.acceptance_passed {
            // Preserve the actual missing obligations. External benchmark
            // artifacts can exist even when another frozen requirement failed.
            // This evidence is diagnostic only: it cannot waive the checks.
            if let Some((goal, checks)) = &self.acceptance {
                let evidence = self.reply_to.as_deref().map(|source| {
                    evidence_for_source(self.state, self.session_id, source, self.execution_id)
                }).unwrap_or_default();
                let gaps = verification::check_acceptance(self.state, goal, checks, &evidence);
                let diagnostic = json!({
                    "reason": "model_rounds_exhausted",
                    "maxRounds": self.max_rounds,
                    "receiptCount": evidence.len(),
                    "missingObligations": gaps,
                });
                if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
                    execution["terminalReview"] = diagnostic.clone();
                    self.state.store.put("execution", &execution)
                        .ok_or("종료 검증 진단을 저장할 수 없습니다.")?;
                }
                self.state.store.audit("acceptance.terminal_incomplete", json!({
                    "executionId":self.execution_id,"review":diagnostic
                }));
            }
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
    /// Shared terminal safety gate for both early receipt completion and
    /// exhausted-budget reconciliation. Unresolved effects never become a
    /// success just because another action produced the expected file.
    fn closure_admitted(&self) -> Result<bool, String> {
        if !self.program_finished || self.owner_changed()? {
            return Ok(false);
        }
        let operations = self.state.store.execution_operations(self.execution_id)?;
        if operations.iter().any(|op| {
            op["outcomeUnknown"] == true
                || matches!(op["status"].as_str(), Some("running" | "interrupted"))
        }) {
            return Ok(false);
        }
        let mut completed = operations.into_iter()
            .filter(|op| op["status"] == "completed").collect::<Vec<_>>();
        crate::engine::procedure::sort_operations(&mut completed);
        let standing_violation = self.strategy_candidates.iter().any(|strategy| {
            strategy["authority"] == "standing"
                && crate::engine::procedure::applies(
                    strategy, &self.request.input, None, None,
                )
                && {
                    let verdict = crate::engine::procedure::evaluate(
                        strategy, &completed, &self.state.config.workspace,
                    );
                    verdict["verified"] == true && verdict["followed"] == false
                }
        });
        Ok(!standing_violation)
    }
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
            self.tool_definitions = crate::engine::extensions::tools::catalog(
                self.state,self.session_id,self.request.agent_id.as_deref()).await;
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
        if self.simple_creation_target.is_none() && !self.tool_definitions.is_empty() {
            self.tool_definitions = tool_catalog::progressive_tools(
                std::mem::take(&mut self.tool_definitions), &goal, self.acceptance.is_some(),
            );
        }
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
        if matches!(self.metadata["completionSource"].as_str(),
            Some("kernel_verified_checkpoint" | "verified_artifact")) {
            let _ = self.tx.send(json!({"type":"text","text":self.final_content}));
        }
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

#[cfg(test)]
#[path = "chat_product/terminal_model_error_tests.rs"]
mod terminal_model_error_tests;

#[cfg(test)]
mod terminal_receipt_tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    #[test]
    fn terminal_receipt_requires_exact_goal_and_durable_effect() {
        let root = std::env::temp_dir().join(format!(
            "dominion-terminal-receipt-{}", uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let config = Config {
            port: 0, data_dir: root.clone(), web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        std::fs::write(root.join("done.txt"), "verified").unwrap();
        let accepted = Some((
            "write done.txt with verified".to_owned(),
            json!([{"type":"file_equals","path":"done.txt","content":"verified"}]),
        ));
        // An already-existing artifact cannot silently complete a failed job.
        assert_eq!(terminal_acceptance_receipt(
            &state, "s", Some("owner"), "execution-1", accepted.as_ref(),
        ), None);
        state.store.put("execution_operation", &json!({
            "id":"write-1","runId":"execution-1","status":"completed",
            "tool":"workspace_write",
            "arguments":{"path":"done.txt","content":"verified"},
            "result":{"success":true},
        }));
        assert_eq!(terminal_acceptance_receipt(
            &state, "s", Some("owner"), "execution-1", accepted.as_ref(),
        ), Some(1));
        state.store.put("execution_operation", &json!({
            "id":"read-2","runId":"execution-read-only","status":"completed",
            "tool":"workspace_read",
            "arguments":{"path":"done.txt"},
            "result":{"content":"verified"},
        }));
        assert_eq!(terminal_acceptance_receipt(
            &state, "s", Some("owner"), "execution-read-only", accepted.as_ref(),
        ), None, "a read-only observation cannot prove an actionable write");
        let no_concrete_checks = Some(("write something".to_owned(), json!([])));
        assert_eq!(terminal_acceptance_receipt(
            &state, "s", Some("owner"), "execution-1",
            no_concrete_checks.as_ref(),
        ), None, "an effect without explicit postconditions is not a verified result");
        let wrong_content = Some((
            "write done.txt with something else".to_owned(),
            json!([{"type":"file_equals","path":"done.txt","content":"wrong"}]),
        ));
        assert_eq!(terminal_acceptance_receipt(
            &state, "s", Some("owner"), "execution-1", wrong_content.as_ref(),
        ), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
