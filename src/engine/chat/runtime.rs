//! Product entrypoints and SQLite/tool/model adapters for the native execution driver.
#[path = "../../addons/integrations/chat_product.rs"]
mod product;
#[path = "outcome.rs"]
mod outcome;
#[path = "../../addons/strategies/chat_program.rs"]
mod program;
#[path = "runtime/tool_dispatch.rs"]
mod tool_dispatch;
#[path = "runtime/finalization.rs"]
mod finalization;
#[path = "../../addons/strategies/chat_tool_catalog.rs"]
mod tool_catalog;
#[path = "../../addons/strategies/chat_adaptation.rs"]
mod adaptation;
use adaptation::{failure_observation, environment_fingerprint, record_adaptation_error, attempt_model_adaptation};
#[path = "runtime/recovery.rs"]
mod recovery;
use recovery::{restore_program_state,load_native_checkpoint};
use tool_catalog::{parallel_read_admitted, tool_is_read_only, normalize_tool_arguments, mark_tool_argument_signal,
    tool_name, recent_room_tools, select_room_tools, select_tool_definitions, trim_tool_definitions};
pub(super) use finalization::{finalize_execution, tool_succeeded};
use finalization::{simple_file_creation, action_plan, review_action,
    acceptance_repair, AcceptanceRepair, record_success_experience, record_selection_snapshot,
    finish_attempt, mark_budget_exhausted, retitle_if_placeholder, ensure_session, record_tool_stat};
use tool_dispatch::{frozen_replacements, run_frozen_replacements, dispatch_tool_call, record_soft_tool_error};
use program::{execute_program_tool, run_program};
use outcome::{compliance_detail, compliance_summary, determine_task_outcome, finalization_payload_hash};
use super::{
    approval::{close_pending_approvals, request_approval},
    context::history,
    provider::{select_provider, stream_completion},
};
use super::{
    parse_text_tool_calls, Metrics, ToolCall, TurnRequest, MAX_ELAPSED, MAX_ROUNDS, MAX_TOKENS, MAX_TOOL_CALLS,
};
use super::evidence::{evidence_for_source, prepare_owner_action};
use crate::engine::adaptation::types::TaskOutcome;
use crate::engine::verification;
use crate::{
    engine::{
        llm, extensions,
        tools::{self, ToolContext},
    },
    state::AppState,
};
use serde_json::{json, Value};
use dominion_core::strategy::TurnStrategyPort;
#[cfg(test)]
use crate::addons::turn_policy::needs_plan;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// Task contracts bound intent to 4096 characters. Keep the full model prompt
/// intact but derive a bounded, content-unique value for the contract.
fn bounded_contract_intent(input: &str) -> String {
    if input.chars().count() <= 4096 {
        return input.to_string();
    }
    format!(
        "{} [{}]",
        input.chars().take(4000).collect::<String>(),
        crate::engine::memory::stable_id(&[input])
    )
}

/// The owner instruction, not an archived research objective, defines the task.
fn task_input<'a>(_session:&Value,input:&'a str)->&'a str {input}

/// Resolve turn, agent and provider settings without confusing a saved
/// legacy thinking=false with a per-turn off request. Explicit per-turn off
/// wins; otherwise an explicit/saved effort tier wins over legacy booleans.
fn resolve_reasoning_effort(
    explicit_effort: Option<&str>,
    requested_thinking: Option<bool>,
    agent_effort: Option<&str>,
    agent_thinking: Option<bool>,
    provider_effort: Option<&str>,
    provider_kind: &str,
) -> (Option<String>, Option<bool>) {
    let effort = [explicit_effort, agent_effort, provider_effort]
        .into_iter().flatten().find(|value| !value.is_empty());
    if requested_thinking == Some(false) || effort == Some("none") {
        return ((provider_kind == "codex").then(|| "none".to_string()), Some(false));
    }
    if let Some(effort) = effort {
        return (Some(effort.to_string()), requested_thinking.or(Some(true)));
    }
    (None, requested_thinking.or(agent_thinking))
}


pub async fn run_turn(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, true, true, None, None).await
}

/// HTTP chat admission owns a durable epoch snapshot. A stop before the
/// spawned native turn begins wins without resuming the older request.
pub(crate) async fn run_turn_http(
    state: &AppState, request: &TurnRequest, tx: &UnboundedSender<Value>,
    accepted_cancel_generation: Option<u64>,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, true, true, None, accepted_cancel_generation).await
}

pub async fn run_turn_background(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, false, true, None, None).await
}

/// Resume path for a rejected/woken run: the original owner message is already
/// stored, so the turn must not append a duplicate user message.
pub async fn run_turn_resume(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, false, false, None, None).await
}

// Shared room input is already durably stored by the owner-message API.

pub async fn run_room_turn(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    source_message_id: &str,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, false, false, Some(source_message_id), None).await
}

async fn run_turn_with_policy(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    reset_cancel: bool,
    record_input: bool,
    source_message_id: Option<&str>,
    accepted_cancel_generation: Option<u64>,
) -> Result<Option<Value>, String> {
    let session = ensure_session(state, request)?;
    if record_input && session["roomId"].is_string() {
        return Err("그룹 대화는 그룹 대화 화면 또는 /api/rooms/:id/messages에서 보내세요.".into());
    }
    if !record_input && source_message_id.is_some() && !session["roomId"].is_string() {
        return Err("공용 대화 기록이 연결된 그룹 세션이 필요합니다.".into());
    }
    if !record_input && source_message_id.is_none() {
        let existing = session["id"]
            .as_str()
            .map(|id| state.store.messages(id))
            .unwrap_or_default();
        if existing.is_empty() {
            return Err("재개할 대화 기록이 없습니다.".into());
        }
    }
    let session_id = session
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or("Session has no id")?
        .to_string();
    let mut workspace_state=state.clone();
    if let Some(path)=crate::engine::delegation_worktree::scoped_workspace(state,&session)? {
        workspace_state.config.workspace=path;
    } else if let Some(raw)=session["workspaceDir"].as_str(){
        workspace_state.config.workspace=crate::engine::workspace_scope::resolve(state,raw)?;
    }
    let state=&workspace_state;
    if !state.begin_session(&session_id) {
        return Err("Session is already running".into());
    }
    let _session_guard = state.session_guard(&session_id);
    // Durable ownership: a crashed process leaves a lease that startup
    // recovery releases, while a live owner (even across restarts) is refused.
    let _lease = crate::engine::lease::LeaseGuard::acquire(state, &session_id)
        .ok_or("다른 엔진 프로세스가 이 세션을 실행 중입니다.")?;
    // Admission happens while both the in-process guard and the durable lease
    // are held. A reconnect/retry of an already-written owner turn may not
    // create another root task or repeat an effectful agent tool call.
    if let Some(client_turn_id) = request.client_turn_id.as_deref() {
        if state.store.has_owner_turn_key(&session_id,client_turn_id)? {
            return Err("동일한 요청이 이미 저장되었습니다. 대화 기록을 확인하세요.".into());
        }
    }
    if reset_cancel {
        if let Some(generation) = accepted_cancel_generation {
            // The HTTP API captured the epoch before launching this task.
            // A STOP that wins the scheduling race must never be cleared by
            // the ordinary new-turn reset.
            if !state.resume_session_if_generation(&session_id, generation) {
                return Err("실행이 중단되었습니다.".into());
            }
        } else {
            state.clear_cancel(&session_id);
        }
    }
    if state.is_cancelled(&session_id) {
        return Err("실행이 중단되었습니다.".into());
    }
    let generation = state.cancellation_generation(&session_id);
    let mut request = request.clone();
    if request.direct_chat {
        // A direct chat must not inherit the agent used earlier in this session.
        request.agent_id = None;
        request.tools_enabled = false;
        request.tools_execution_enabled = false;
        request.auto_learning = false;
    } else if request.agent_id.is_none() {
        request.agent_id = session
            .get("agentId")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    let agent = if let Some(id) = &request.agent_id {
        Some(state.store.get("agent", id)
            .ok_or_else(||format!("에이전트를 찾을 수 없습니다: {id}"))?)
    } else { None };
    let provider = select_provider(state, &request, &session)?;
    let provider_settings = state.store.get("provider", &provider.id);
    let (effort, thinking) = resolve_reasoning_effort(
        request.reasoning_effort.as_deref(),
        request.thinking,
        agent.as_ref().and_then(|value|value["reasoningEffort"].as_str()),
        agent.as_ref().and_then(|value|value["thinking"].as_bool()),
        provider_settings.as_ref().and_then(|value|value["reasoningEffort"].as_str()),
        &provider.kind,
    );
    request.reasoning_effort = effort;
    request.thinking = thinking;
    request.provider_id = Some(provider.id.clone());
    request.model = Some(provider.model.clone());
    let mut session = session;
    session["providerId"] = json!(provider.id);
    session["model"] = json!(provider.model);
    // A room shares a transcript but each speaker keeps its own identity.
    if session.get("roomId").is_none() {
        session["agentId"] = json!(request.agent_id);
        session["chatMode"] = json!(if request.direct_chat { "chat" } else { "agent" });
    }
    state
        .store
        .put("session", &session)
        .ok_or("세션 설정을 저장할 수 없습니다.")?;

    let execution_id = uuid::Uuid::new_v4().to_string();
    let (applied_improvement_ids,strategy_candidates) = if request.direct_chat {
        // Plain conversations keep transcript context, not agent strategy/procedure overlays.
        (Vec::new(), Vec::new())
    } else {
        crate::addons::runtime_hooks::prepare_turn(
            state,&session_id,request.agent_id.as_deref(),&execution_id,
        )
    };
    // Derived prompts (goal iterations, room speakers) attach to the root the
    // parent lifecycle created. Only an owner-facing chat turn creates a root.
    let task_id = request.task_id.clone().unwrap_or_else(|| execution_id.clone());
    let parent_task_id = request.parent_task_id.clone();
    let verification_owner = request
        .verification_owner
        .clone()
        .unwrap_or_else(|| "self".into());
    if request.task_id.is_some() {
        let Some(episode) = state.store.get("task_episode", &task_id) else {
            return Err("연결할 상위 작업(task)을 찾을 수 없습니다.".into());
        };
        if episode["contract"]["scope"]["sessionId"].as_str() != Some(session_id.as_str()) {
            return Err("상위 작업이 이 세션에 속하지 않습니다.".into());
        }
        if let Some(expected) = request.task_revision.as_deref() {
            if episode["contract"]["revision"].as_str() != Some(expected) {
                return Err("상위 작업의 계약 revision이 변경되었습니다.".into());
            }
        }
    }
    // The task contract bounds intent to 4096 characters. A self-improvement
    // prompt carries a repository index and can be much larger; keep the full
    // prompt for the model, but derive a bounded, unique contract intent.
    let contract_intent = bounded_contract_intent(task_input(&session,&request.input));
    let root_spec = crate::engine::episode::RootSpec {
        task_id: &task_id,
        intent: &contract_intent,
        family: "chat",
        source: "owner",
        source_message_id: None,
        scope: json!({"partition": "local", "sessionId": session_id}),
        mandatory: Vec::new(),
    };
    let task_revision = crate::engine::episode::revision_for(&root_spec)
        .ok_or("작업 계약을 준비할 수 없습니다.")?;
    let root = crate::engine::episode::root_value(&root_spec)
        .ok_or("작업 계약을 준비할 수 없습니다.")?;
    let execution_document = json!({
        "id": execution_id,
        "taskId": task_id,
        "taskRevision": task_revision,
        "parentTaskId": parent_task_id,
        "sessionId": session_id,
        "agentId": request.agent_id,
        "providerId": request.provider_id,
        "model": request.model,
        "reasoningProtocol":provider.reasoning_protocol,
        "thinkingRequested":request.thinking,"reasoningEffortRequested":request.reasoning_effort,
        "status": "running",
        "phase": "queued",
        "startedAt": chrono::Utc::now().to_rfc3339(),
        "iterations": 0,
        "modelCalls": 0,
        "toolCalls": 0,
        "tokens": 0,
        "request": request.input,
        "appliedImprovementIds": applied_improvement_ids,
        "strategyCandidates": strategy_candidates,
    });
    let attempt = json!({
        "executionId": execution_id,
        "agentId": request.agent_id,
        "verificationOwner": verification_owner,
        "status": "running",
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    let execution = state
        .store
        .create_attempt(&root, &execution_document, &attempt)
        .ok_or("실행 기록을 저장할 수 없습니다.")?;
    state.bus.record("execution", &execution);
    crate::engine::run_state::apply(state, &execution_id, "start");
    let metrics = std::sync::Mutex::new(Metrics::default());
    let execution_budget = match crate::engine::execution_budget::begin(state,&session,&execution_id){
        Ok(budget)=>budget,Err(error)=>{finish_attempt(state,&execution_id,&Err(error.clone()),&metrics);return Err(error);}
    };
    if provider.context_tokens.is_some() {
        if let Some(mut execution) = state.store.get("execution", &execution_id) {
            execution["contextChecked"] = json!(true);
            execution["contextOverflow"] = json!(false);
            state.store.put("execution", &execution);
        }
    }
    let turn_limit=MAX_ELAPSED;
    let result = tokio::select! {
        result = tokio::time::timeout(turn_limit, run_turn_body(state, &request, tx, &session_id, &metrics, &execution_id, &task_id, record_input, generation, source_message_id)) =>
            result.unwrap_or_else(|_| Err("설정한 실행 시간 제한을 초과했습니다.".into())),
        // A closed SSE client must not destroy work already in flight. The turn
        // keeps running in its spawned task; only an explicit cancel or the
        // elapsed budget stops it. Results stay durable for later reads.
        _ = wait_cancelled(state, &session_id, generation) => Err("실행이 중단되었습니다.".into()),
    };
    finish_attempt(state, &execution_id, &result, &metrics);
    execution_budget.finish();
    close_pending_approvals(state, &session_id);

    crate::addons::runtime_hooks::after_turn(
        state,&request,&session_id,&execution_id,&result,
    );
    if result.is_ok() {
        // Older messages become a checkpoint before the next turn reads them.
        // The work runs in the background; this turn's answer is already done.
        crate::engine::compact::spawn_auto(state, &session_id);
    }
    result
}

async fn wait_cancelled(state: &AppState, session_id: &str, generation: u64) {
    while !state.is_cancelled(session_id) && state.cancellation_generation(session_id) == generation
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn run_turn_body(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    metrics: &std::sync::Mutex<Metrics>,
    execution_id: &str,
    task_id: &str,
    record_input: bool,
    generation: u64,
    source_message_id: Option<&str>,
) -> Result<Option<Value>, String> {
    let turn_started = std::time::Instant::now();
    let session = state
        .store
        .get("session", session_id)
        .ok_or("Session not found")?;
    let strategy_candidates = state
        .store
        .get("execution", execution_id)
        .and_then(|execution| execution["strategyCandidates"].as_array().cloned())
        .unwrap_or_default();

    let recorded_source = if record_input {
        let attachments =
            crate::engine::attachments::validate_ids(state, session_id, &request.attachment_ids)?;
        let user_message = state
            .store
            .write_message(&json!({
                "session_id": session_id,"role": "user","content": request.input,
                "agent_id": request.agent_id,"metadata":{"authorKind":"owner",
                    "attachments":attachments,"clientTurnId":request.client_turn_id},
            }))
            .ok_or("사용자 메시지를 저장할 수 없습니다.")?;
        state.bus.message(&user_message);
        // A stable message ID is the authoritative acknowledgment of owner
        // persistence. UI must not infer it from repeated text or stream timing.
        let _ = tx.send(json!({
            "type":"owner_saved",
            "sessionId":session_id,
            "messageId":user_message["id"],
        }));
        crate::engine::episode::attach_source(
            state,
            task_id,
            user_message["id"].as_str().unwrap_or(""),
            &request.input,
        );
        retitle_if_placeholder(state, &session, &request.input);
        user_message["id"].as_str().map(str::to_string)
    } else { None };
    let reply_to = if let Some(id) = source_message_id {
        let source = state
            .store
            .message(id)
            .ok_or("주인의 원문 메시지를 찾을 수 없습니다.")?;
        if source["session_id"] != session_id
            || source["role"] != "user"
            || source["metadata"]["authorKind"] != "owner"
        {
            return Err("그룹 대화 원문과 실행이 일치하지 않습니다.".into());
        }
        Some(id.to_string())
    } else {
        recorded_source.or_else(|| state
            .store
            .messages(session_id)
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["id"].as_str().map(str::to_string)))
    };
    // The acceptance baseline is captured BEFORE any tool runs. For rooms the
    // original owner message is the only source of truth; the synthesized
    // group prompt is never treated as an instruction.
    let acceptance_source = match source_message_id {
        Some(id) => state
            .store
            .message(id)
            .and_then(|message| message["content"].as_str().map(str::to_string)),
        None => Some(task_input(&session,&request.input).to_string()),
    };
    let mut acceptance = if request.direct_chat || request.verification_owner.as_deref() == Some("parent") {
        // The independently verified parent goal owns review;
        // investigating and declining a change must not trigger forced writes.
        // The child still
        // records operations, deterministic evidence and cancellation; it must
        // not spend a second reviewer call on a synthesized derived prompt.
        None
    } else {
        source_message_id
            .and_then(|id| state.store.message(id))
            .and_then(|message| {
                let acceptance = &message["metadata"]["acceptance"];
                Some((acceptance["request"].as_str()?.to_owned(), acceptance["checks"].clone()))
            })
            .or_else(|| crate::engine::job_control::initial_acceptance(state,&session,acceptance_source.as_deref().unwrap_or(&request.input)))
            .or_else(|| acceptance_source.as_deref().and_then(|text| prepare_owner_action(state, text)))
    };
    if let Some(command)=session["nativeSdkVerifyCommand"].as_str().or_else(||session["nativeJobVerifyCommand"].as_str()).filter(|s|!s.trim().is_empty() && !request.direct_chat){
        let (text,mut checks)=acceptance.take().unwrap_or_else(||(request.input.clone(),json!([])));
        if let Some(items)=checks.as_array_mut(){items.push(json!({"type":"command_succeeded","command":command}));}
        acceptance=Some((text,checks));
    }
    // An ambiguous literal replacement cannot be frozen into one expected
    // hash. Drop only that unverifiable check so the model still gets a turn;
    // the task then remains observable but is not reported as verified.
    let mut demote_all = false;
    if let Some((_, checks)) = &mut acceptance {
        let before = checks.as_array().map(Vec::len).unwrap_or(0);
        if let Some(items) = checks.as_array_mut() {
            items.retain(|check| {
                !(check["type"] == "file_replace_exact"
                    && check["expectedSha256"].as_str().is_none())
            });
        }
        let after = checks.as_array().map(Vec::len).unwrap_or(0);
        if after < before {
            state.store.audit(
                "acceptance.demoted",
                json!({
                    "executionId": execution_id,
                    "reason": "ambiguous exact replacement: expected hash unavailable",
                    "removedChecks": before - after,
                }),
            );
            demote_all = after == 0;
        }
    }
    if demote_all {
        acceptance = None;
    }
    if let Some((text, checks)) = &acceptance {
        let mut execution = state.store.get("execution", execution_id)
            .ok_or("실행 기록을 찾을 수 없습니다.")?;
        execution["acceptanceText"] = json!(text);
        execution["acceptanceChecks"] = checks.clone();
        execution["acceptanceSourceMessageId"] = json!(reply_to);
        state.store.put("execution", &execution).ok_or("검증 조건을 저장할 수 없습니다.")?;
    }
    let acceptance_retries = 0usize;
    let procedure_repairs = 0usize;
    let recovery_policy = crate::engine::policy::active(state, "recovery");
    let program_policy = if request.direct_chat { None } else {
        crate::engine::policy::active(state, "execution")
    };
    // The file guard is opt-in: an operator or a verified policy must
    // activate mutation.preflight before it applies. No automatic promotion.
    let selected_preflight = crate::engine::policy::portable_file_guard(state)
        .or_else(|| crate::engine::policy::active(state, "mutation.preflight"));
    if let Some(policy) = selected_preflight {
        let mut execution = state.store.get("execution", execution_id)
            .ok_or("실행 기록을 찾을 수 없습니다.")?;
        execution["preflightPolicy"] = policy;
        state.store.put("execution", &execution).ok_or("검증 정책을 저장할 수 없습니다.")?;
    }
    // A contract that is already a complete, independently verifiable
    // transformation does not need a model decision. This is an execution
    // optimization, not learning evidence.
    if let Some((text, checks)) = &acceptance {
        if let Some(replacements) = frozen_replacements(checks) {
            if let Some(saved) = run_frozen_replacements(
                state,
                request,
                tx,
                session_id,
                execution_id,
                task_id,
                text,
                replacements,
                metrics,
            )
            .await?
            {
                return Ok(Some(saved));
            }
        }
    }
    let (max_acceptance_retries, max_procedure_repairs) = recovery_policy
        .as_ref()
        .map(|policy| {
            crate::engine::policy::recovery_limits(&policy["parameters"])
        })
        .unwrap_or((2, 2));
    let op_seq = 0u64;

    let metadata = json!({"runtime":"dominion-core","authorKind":"agent","roomId":session.get("roomId"),"replyToMessageId":reply_to,"executionId":execution_id,"taskId":task_id,"reasoningLanguageRequested":request.reasoning_language,"reasoningLanguageEffective":request.effective_reasoning_language(),"reasoningTranslated":false});

    let provider = select_provider(state, request, &session)?;
    let allow_provider_fallback = true;
    let intelligence=crate::addons::runtime_hooks::strategy_port(
        state,session_id,request.agent_id.as_deref(),execution_id,
    );
    let decision_limits=intelligence.execution_limits(MAX_ROUNDS,MAX_TOOL_CALLS);
    let mut max_rounds=if request.direct_chat { 1 } else { decision_limits.max_rounds };
    if let Some(limit)=session["nativeSdkMaxRounds"].as_u64(){
        let used=state.store.sdk_used_model_calls(session_id,execution_id)?;
        let remaining=limit.clamp(1,80).saturating_sub(used);
        if remaining==0{return Err("SDK의 누적 모델 호출 한도에 도달했습니다.".into());}
        max_rounds=if request.direct_chat { 1 } else { remaining as usize };
    }
    let mut max_tools=if request.direct_chat { 0 } else { decision_limits.max_tools };
    let elapsed_limit=MAX_ELAPSED;
    let simple_creation_target = if !session["roomId"].is_string() {
        simple_file_creation(state,&request.input,&acceptance)
    } else {None};
    let mut tool_definitions = if request.tools_enabled && request.tools_execution_enabled {
        extensions::tools::native(state,session_id,request.agent_id.as_deref())
    } else {
        Vec::new()
    };
    let is_room = session["roomId"].is_string();
    let social_private = !is_room && super::context_engine::is_social_turn(&request.input);
    let plain_private = request.direct_chat || (!is_room && record_input && acceptance.is_none()
        && simple_creation_target.is_none() && session["delegated"]!=true
        && request.attachment_ids.is_empty()
        && !tool_catalog::private_tool_request(&request.input));
    if social_private || plain_private {
        // A greeting cannot benefit from hundreds of registered MCP schemas
        // or a tool discovery gateway, even when tool access is enabled.
        tool_definitions.clear();
    }
    let casual_room = is_room && acceptance.is_none()
        && !tool_catalog::room_tool_request(&request.input);
    // Do not enumerate remote MCP servers or scan old tool receipts for
    // ordinary group conversation. A resumed action still uses the full path.
    if request.tools_enabled && request.tools_execution_enabled && !casual_room && !social_private && !plain_private {
        tool_definitions.extend(extensions::tools::external(
            state,session_id,request.agent_id.as_deref()).await);
    }
    if is_room && request.tools_enabled && request.tools_execution_enabled {
        if casual_room {
            tool_definitions.clear();
        } else {
            let recent = recent_room_tools(state, session_id);
            tool_definitions =
                select_room_tools(tool_definitions, &request.input, &recent, acceptance.is_some());
        }
    }
    let discovery_decision=intelligence.discovery_decision(is_room);
    let discovery=discovery_decision.selection;
    if !is_room {
        tool_definitions = select_tool_definitions(tool_definitions, &request.input, &discovery);
    }
    // A skill index without a usable lookup would silently remove existing
    // instructions from tool-enabled agents. Advertise exactly one small
    // read-only schema when this agent can actually view an enabled skill.
    if request.tools_enabled && request.tools_execution_enabled && !social_private && !plain_private
        && (!is_room || !tool_definitions.is_empty()) {
        let has_skill=extensions::knowledge::has_visible_skills(
            state,session_id,request.agent_id.as_deref());
        if has_skill && !tool_definitions.iter().any(|def|tool_name(def)=="skill_view") {
            if let Some(definition) = extensions::tools::native(
                state,session_id,request.agent_id.as_deref()).into_iter()
                .find(|def|tool_name(def)=="skill_view") {
                tool_definitions.push(definition);
            }
        }
    }
    if simple_creation_target.is_some() {
        // A concrete file creation has no use for web, memory, delegation or
        // Keep native file access and shell fallback available.
        let workspace_root = state.config.workspace.canonicalize()
            .unwrap_or_else(|_|state.config.workspace.clone());
        let inside_workspace = simple_creation_target.as_ref().is_some_and(|path|
            path.starts_with(&workspace_root));
        tool_definitions = extensions::tools::native(
            state,session_id,request.agent_id.as_deref()).into_iter().filter(|definition| {
            let name = tool_name(definition);
            matches!(name, "shell_exec" | "terminal_exec") || if inside_workspace {
                matches!(name, "workspace_list" | "workspace_read" | "workspace_write")
            } else {
                matches!(name, "file_list" | "file_read" | "file_write")
            }
        }).collect();
        if !request.tools_enabled || !request.tools_execution_enabled { tool_definitions.clear(); }
        intelligence.discovery_overridden("single-file task catalog");
        if let Some(mut execution) = state.store.get("execution", execution_id) {
            execution["contextPolicy"] = json!("single-file-creation");
            state.store.put("execution", &execution);
        }
    }
    // Freeze the *provider-visible* catalog before compiling context, so
    // room budgets, stored accounting, continuity hashes and actual model
    // requests all use the same schemas. Previously pruning happened AFTER
    // context compilation and falsely pushed tool-heavy rooms over budget.
    if session["delegationWorkspace"].is_object(){tool_definitions.retain(|tool|!tool_name(tool).starts_with("mcp__")&&!matches!(tool_name(tool),"github_api"|"github_repositories"|"github_file_read"|"github_file_update"|"github_file_replace"|"browser_control"));}
    if session["readOnly"]==true {tool_definitions.retain(|tool|tools::observational(tool_name(tool)));}
    tool_definitions.retain(|tool|tools::profile_tool_visible(&session,tool_name(tool)));
    if request.tools_enabled && request.tools_execution_enabled
        && simple_creation_target.is_none() && !tool_definitions.is_empty() {
        tool_definitions = tool_catalog::progressive_tools(
            tool_definitions, &request.input, acceptance.is_some()
        );
        // Progressive disclosure adds discovery adapters; apply the original
        // session restrictions to those newly inserted definitions as well.
        if session["readOnly"]==true {tool_definitions.retain(|tool|tools::observational(tool_name(tool)));}
        tool_definitions.retain(|tool|tools::profile_tool_visible(&session,tool_name(tool)));
    }
    // A tool-heavy room may need the full native tool/procedure prompt.
    // Its *conversation* must still use the shared, provenance-scoped room
    // selector: falling through to general chat history resends every peer's
    // earlier turns and can expose later owner messages.
    let mut scoped_room_history: Option<(Vec<Value>, Value)> = None;
    let compiled = if is_room {
        let source = source_message_id.or(reply_to.as_deref()).ok_or("응답할 원문이 없습니다.")?;
        let compiled = super::room_context::compile(
            state, request, session_id, source, &tool_definitions, &strategy_candidates,
        )?;
        // A tool task needs room for its applicable procedures and evidence.
        // When fixed identity + selected native schemas exhaust the proposed
        // conversational budget, keep the established work-context policy.
        let work_needs_context = !tool_definitions.is_empty()
            && compiled.accounting["requiredOverBudget"] == true;
        if work_needs_context {
            if let Some(mut execution) = state.store.get("execution", execution_id) {
                execution["contextPolicy"] = json!("native-work-context");
                execution["contextPolicyReason"] = json!("fixed work context exceeds conversational target; preserve native procedures while keeping source-scoped room history");
                state.store.put("execution", &execution);
            }
            scoped_room_history=Some((compiled.history, compiled.accounting));
            None
        } else {
            Some(compiled)
        }
    } else {
        None
    };
    let (prompt_context, history_messages) = if let Some(compiled) = compiled {
        let delivered_memories = compiled.system.items.iter()
            .filter(|item| item["kind"] == "memory")
            .filter_map(|item| item["id"].as_str().and_then(|id| state.store.get("memory", id)))
            .collect::<Vec<_>>();
        extensions::knowledge::record_context_use(
            state,session_id,request.agent_id.as_deref(),&delivered_memories);
        if let Some(mut execution) = state.store.get("execution", execution_id) {
            execution["inputContext"] = compiled.accounting;
            state.store.put("execution", &execution);
        }
        (compiled.system, compiled.history)
    } else if !is_room {
        // The private compiler owns history selection, evidence snapshot and
        // the full input-budget estimate. Do not rebuild that selection here.
        let compiled=super::context_engine::compile_private(
            state,request,session_id,task_input(&session,&request.input),
            Some(&strategy_candidates),Some(execution_id),
            provider.context_tokens,provider.max_output_tokens,&tool_definitions,
        )?;
        if let Some(mut execution)=state.store.get("execution",execution_id) {
            execution["inputContext"]=compiled.accounting;
            state.store.put("execution",&execution);
        }
        (compiled.system,compiled.history)
    } else {
        let prompt_context = super::context::system_prompt_with_manifest_at(
            state,
            request,
            session_id,
            task_input(&session,&request.input),
            Some(&strategy_candidates),
            Some(execution_id),
        );
        let history_messages = if let Some((selected,_))=scoped_room_history.as_ref() {
            // Source-bounded owner transcript, latest relevant peers, prior
            // self response and referenced tool results are already rendered.
            selected.clone()
        } else {
            history(
                state,
                session_id,
                request.agent_id.as_deref(),
                provider.context_tokens.map(|tokens| {
                    ((tokens as usize).saturating_mul(3)).clamp(8_000, 400_000)
                }),
            )?
        };
        (prompt_context, history_messages)
    };
    // Tool-heavy rooms may use the general native-work prompt with a
    // source-scoped room history. The owner-state evidence still needs a
    // dependency proof or its immutable cached prefix could survive a later
    // correction without being invalidated.
    let mut provenance_items = prompt_context.items.clone();
    if is_room {
        for row in &history_messages {
            let Some(id) = row["_dominion_frame"].as_str()
                .and_then(|key| key.strip_prefix("owner-state:")) else { continue };
            if !provenance_items.iter().any(|item|
                item["kind"] == "owner_state" && item["id"] == id) {
                provenance_items.push(json!({"kind":"owner_state","id":id}));
            }
        }
    }
    let mut messages = vec![json!({"role":"system","content":prompt_context.prompt,"_dominion_source":reply_to,
        "_dominion_context_reset":social_private,
        "_dominion_dependencies":super::continuity::dependencies(state,&provenance_items)})];
    if is_room {
        if let Some(mut execution) = state.store.get("execution", execution_id) {
            if execution["inputContext"].is_null() {
                let persona = request.agent_id.as_deref().and_then(|id| state.store.get("agent", id));
                let persona_tokens = super::room_context::estimate(persona.as_ref().and_then(|agent|agent["systemPrompt"].as_str()).unwrap_or(""));
                let mut prepared = messages.clone();
                prepared.extend(history_messages.clone());
                let selection=scoped_room_history.as_ref().map(|(_,accounting)|accounting);
                execution["inputContext"] = json!({
                    "policy":"native-work-context", "estimated":true, "contextModelCalls":0,
                    "conversationSelection":if selection.is_some() {"room-selective-v1"}else{"general-history"},
                    "personaTokens":persona_tokens, "memoryTokens":null,
                    "sharedStateTokens":selection.map(|a|a["sharedStateTokens"].clone()),
                    "ownerStateTokens":selection.map(|a|a["ownerStateTokens"].clone()),
                    "selectedMessageIds":selection.map(|a|a["selectedMessageIds"].clone()),
                    "availableMessages":selection.map(|a|a["availableMessages"].clone()),
                    "requiredOverBudget":selection.map(|a|a["requiredOverBudget"].clone()),
                    "historyTokens":super::room_context::estimate(&json!(history_messages).to_string()),
                    "systemTokens":super::room_context::estimate(&prompt_context.prompt).saturating_sub(persona_tokens),
                    "toolSchemaTokens":super::room_context::estimate(&json!(tool_definitions).to_string()),
                    "totalInputTokens":super::room_context::input_estimate(&prepared,&tool_definitions),
                    "note":"native tool/procedure prompt retains existing behavior; room history is selected by the owner-message cursor"
                });
                state.store.put("execution", &execution);
            }
        }
    }
    let context_overflow = false;
    let mut context_items = provenance_items;
    // The profile is trusted child-session metadata, not a model-generated
    // role claim. Admission is enforced separately by the tool gateway.
    if let Some(profile) = session["delegationProfile"].as_str() {
        let role_instruction = match profile {
            "inspection" => Some("Inspection worker: inspect code and evidence only. Report exact file paths and observations; do not claim changes."),
            "implementation" => Some("Implementation worker: change only the isolated task worktree. Verify the result and describe the patch; the parent decides whether to integrate."),
            "verification" => Some("Verification worker: inspect the isolated worktree and run only approved Cargo tests. Report failed checks and precise evidence without modifying source files."),
            _ => None,
        };
        if let Some(instruction) = role_instruction {
            messages.push(json!({"role":"system","content":instruction}));
            context_items.push(json!({
                "kind":"delegation_profile","profile":profile,
                "isolated":session["delegationWorkspace"].is_object(),
            }));
        }
    }
    let retrieved_experience_ids = context_items
        .iter()
        .filter(|item| item["kind"].as_str() == Some("experience"))
        .filter_map(|item| item["id"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    if let Some(mut execution) = state.store.get("execution", &execution_id) {
        execution["retrievedExperienceIds"] = json!(retrieved_experience_ids);
        state.store.put("execution", &execution);
    }
    context_items.push(json!({
        "kind": "history",
        "count": history_messages.len(),
        "messageIds": history_messages
            .iter()
            .filter_map(|message| message["id"].as_str())
            .collect::<Vec<_>>(),
        "digest": crate::engine::memory::stable_id(&[
            &serde_json::to_string(
                &history_messages
                    .iter()
                    .map(|message| json!({
                        "id": message["id"],
                        "role": message["role"],
                        "content": message["content"],
                        "agentId": message["agent_id"],
                    }))
                    .collect::<Vec<_>>()
            )
            .unwrap_or_default(),
        ]),
    }));
    // Compiler-owned materialization keeps one private evidence snapshot
    // before the durable transcript; group fallback retains room semantics.
    super::context_engine::append_context(
        &mut messages,&prompt_context,history_messages,!is_room,
    );
    metrics.lock().unwrap().context_elapsed_ms = turn_started.elapsed().as_millis() as u64;

    // Action requests get an explicit plan before tools. A versioned policy in
    // the planning slot can override the built-in heuristic; no active policy
    // means the existing behaviour is unchanged.
    let planning_policy=intelligence.planning_policy();
    if let Some(policy) = &planning_policy {
        context_items.push(json!({
            "kind": "planning_policy",
            "id": policy["id"],
            "revision": policy["revision"],
            "parameters": policy["parameters"],
        }));
    }
    if let Some(policy) = &recovery_policy {
        context_items.push(json!({
            "kind": "recovery_policy",
            "id": policy["id"],
            "revision": policy["revision"],
            "parameters": policy["parameters"],
        }));
    }
    if let Some(policy) = &program_policy {
        context_items.push(json!({
            "kind": "execution_policy",
            "id": policy["id"],
            "revision": policy["revision"],
            "programRevision": policy["parameters"]["program"]["revision"],
        }));
    }
    // An optional algorithm supplies decomposition. Native verification still
    // owns every acceptance check even when strategies are disabled.
    let frozen_checks=acceptance.as_ref().map(|(_,checks)|checks).unwrap_or(&Value::Null);
    let typed_decomposition=intelligence.decomposition_decision(
        task_input(&session,&request.input),frozen_checks);
    let decomposition=typed_decomposition.payload;
    let subtask_count = decomposition["subtasks"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["subtasks"] = decomposition["subtasks"].clone();
        execution["decomposition"] = decomposition.clone();
        state.store.put("execution", &execution);
    }
    context_items.push(json!({
        "kind": "task_decomposition",
        "kernel": decomposition["kernel"],
        "subtasks": subtask_count,
        "digest": crate::engine::memory::stable_id(&[
            task_input(&session,&request.input),
            decomposition["kernel"].as_str().unwrap_or(""),
        ]),
    }));
    let planning_decision=intelligence.planning_decision(
        planning_policy.as_ref(),&request.input,&acceptance,subtask_count,
    );
    let plan_requested=planning_decision.requires_plan;
    context_items.push(json!({
        "kind":"strategy_planning_decision",
        "decision":planning_decision
    }));

    if subtask_count > 1 && acceptance.is_some() && request.tools_enabled && request.tools_execution_enabled {
        let subtasks = decomposition["subtasks"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, subtask)| {
                format!(
                    "{}. {}",
                    index + 1,
                    subtask["text"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        messages.insert(1, json!({
            "role":"system",
            "content":format!("{}\n\nEngine-owned frozen acceptance obligations are not completed work. Resolve each applicable check with actual tool receipts. Only explicit dependency links impose order; never claim success from the checklist itself.",crate::engine::chat::harness_prompts::render(state,"agent.decomposition",&[("subtasks",&subtasks)]))
        }));
    }

    // Keep a native tool schema catalog. The provider adapter constructs
    // native calls or the text-tool manual for whichever local/API model
    // actually handles each round, including after failover.

    let discovered_tool_names = tool_definitions
        .iter()
        .filter_map(|definition| {
            definition["function"]["name"]
                .as_str()
                .or_else(|| definition["name"].as_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["discoveredTools"] = json!(discovered_tool_names);
        execution["budgetChecked"] = json!(true);
        state.store.put("execution", &execution);
    }
    // Read-only and isolated-session policy was applied before progressive
    // selection. Reapplying it here was redundant and hid a missing boundary.
    let native_tool_names = tool_definitions
        .iter()
        .filter_map(|definition| {
            definition["function"]["name"]
                .as_str()
                .or_else(|| definition["name"].as_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    context_items.push(json!({
        "kind": "tools",
        "providerToolMode": provider.tool_mode,
        "availableDefinitions": native_tool_names,
        "nativeDefinitions": native_tool_names.clone(),
        "textManual": provider.tool_mode == "compat"
            && request.tools_enabled
            && request.tools_execution_enabled,
        "digest": crate::engine::memory::stable_id(&[
            &serde_json::to_string(&native_tool_names).unwrap_or_default(),
            &provider.tool_mode,
        ]),
    }));
    context_items.push(json!({"kind":"planning_policy","requested":plan_requested,
        "policyId":planning_policy.as_ref().map(|policy|&policy["id"])}));
    let final_system = messages
        .first()
        .and_then(|message| message["content"].as_str())
        .unwrap_or("")
        .to_string();
    context_items.push(json!({
        "kind": "system_prompt_final",
        "digest": crate::engine::memory::stable_id(&[&final_system]),
        "chars": final_system.chars().count(),
    }));
    let typed_procedure=intelligence.procedure_decision(
        &strategy_candidates,&request.input);
    context_items.push(json!({
        "kind": "harness_plan",
        "schemaVersion":typed_procedure.schema_version,
        "strategies": typed_procedure.strategies,
        "note": "harness output is recorded as execution_operation evidence after this freeze",
    }));
    let model_config = json!({
        "providerId": provider.id,
        "providerName": provider.name,
        "model": provider.model,
        "toolMode": provider.tool_mode,
        "reasoningProtocol": provider.reasoning_protocol,
        "thinking": request.thinking,
        "reasoningEffort": request.reasoning_effort,
        "reasoningLanguage": request.reasoning_language,
        "toolsEnabled": request.tools_enabled,
        "toolsExecutionEnabled": request.tools_execution_enabled,
        "maxRounds": max_rounds,
        "maxToolCalls": max_tools,
        "maxTokens": MAX_TOKENS,
        "maxElapsedMs": MAX_ELAPSED.as_millis() as u64,
    });
    record_selection_snapshot(
        state,
        execution_id,
        task_id,
        session_id,
        request,
        &strategy_candidates,
        context_items,
        prompt_context.memory_watermark,
        model_config,
    )?;

    // Room turns are short conversational beats. A hung free model must not
    // stall the whole room for five minutes; fail over within a minute.
    let request_timeout = if session["roomId"].is_string() {
        std::env::var("DOMINION_ROOM_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value >= 10)
            .unwrap_or(60)
    } else {
        300
    };
    let client = state.provider_http_client(request_timeout)?;

    let final_content = String::new();
    let final_reasoning = String::new();
    let last_reasoning = String::new();
    let continuation_projection = None;
    let total_tokens: u64 = 0;
    let prompt_tokens_total: u64 = 0;
    let completion_tokens_total: u64 = 0;
    let cached_prompt_tokens_total: u64 = 0;
    let cache_reporting_complete = true;
    let usage_estimated = false;
    let started_at = std::time::Instant::now();
    metrics.lock().unwrap().context_elapsed_ms = turn_started.elapsed().as_millis() as u64;
    let finished = false;
    let force_final = false;
    let call_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let failed_calls: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let environment_version = 0u64;
    let adaptation_attempts = 0usize;
    let adaptation_failures = 0usize;
    let acceptance_passed = false;
    let budget = super::budget::BudgetLedger::new(max_rounds, max_tools, elapsed_limit);

    let _kernel_checkpoint = KernelCheckpoint { state, execution_id, budget: budget.clone() };
    let kernel_state = dominion_core::hosted::Snapshot::new(execution_id, &bounded_contract_intent(&request.input), max_rounds)?;
    let kernel_journal = ChatKernelJournal { state, execution_id, session_id, generation };
    dominion_core::hosted::Journal::save(&kernel_journal, &kernel_state)?;
    let program_finished=program_policy.is_none();
    let mut adapter=product::MainTurn{
        metrics,
        owner:if request.direct_chat { None } else {
            crate::engine::job_control::Owner::from_session(state,&session)
        },
        owner_revision:session["ownerAppliedRevision"].as_u64().unwrap_or(0),
        state,
        request:request.clone(),
        tx,
        session_id,
        execution_id,
        task_id,
        generation,
        source_message_id,
        session,
        strategy_candidates,
        reply_to,
        acceptance,
        max_acceptance_retries,
        max_procedure_repairs,
        acceptance_retries,
        procedure_repairs,
        program_policy,
        planning_policy,
        plan_requested,
        op_seq,
        metadata,
        provider,
        allow_provider_fallback,
        max_rounds,
        max_tools,
        elapsed_limit,
        simple_creation_target,
        tool_definitions,
        is_room,
        context_overflow,
        messages,
        client,
        turn_started,
        final_content,
        final_reasoning,
        last_reasoning,
        continuation_projection,
        total_tokens,
        prompt_tokens_total,
        completion_tokens_total,
        cached_prompt_tokens_total,
        cache_reporting_complete,
        usage_estimated,
        started_at,
        finished,
        force_final,
        call_counts,
        failed_calls,
        environment_version,
        adaptation_attempts,
        adaptation_failures,
        acceptance_passed,
        budget,
        program_finished,
        model_round: product::ModelRound::default(),
        tool_round: product::ToolRound::default(),
    };
    let context=dominion_agent_sdk::Context{snapshot:kernel_state,budget:adapter.budget.clone(),round:0,journal:&kernel_journal};
    dominion_agent_sdk::run(&mut adapter, context).await?;
    adapter.save().await

}

#[allow(clippy::too_many_arguments)]
async fn run_harness_nodes(
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
/// Explicit product-program branches are native child frames. Independent
/// branches retain their concurrency and shared parent budget; no two workers
/// mutate the same native checkpoint or invent additional budget.
struct ChatToolJournal<'a>{state:&'a AppState,execution_id:&'a str,frame_id:&'a str,session_id:&'a str,generation:u64}
impl dominion_core::hosted::Journal for ChatToolJournal<'_>{
    fn save(&self,snapshot:&dominion_core::hosted::Snapshot)->Result<(),String>{
        self.state.store.put("native_frame",&json!({"id":self.frame_id,"parentExecutionId":self.execution_id,"sessionId":self.session_id,"runtime":"dominion-core","kernelState":snapshot})).ok_or("네이티브 실행 프레임을 저장하지 못했습니다.")?;Ok(())
    }
    fn cancelled(&self,_:&str)->bool{self.state.is_cancelled(self.session_id)||self.state.cancellation_generation(self.session_id)!=self.generation}
}

struct ChatKernelJournal<'a> {
    state: &'a AppState,
    execution_id: &'a str,
    session_id: &'a str,
    generation: u64,
}
impl dominion_core::hosted::Journal for ChatKernelJournal<'_> {
    fn save(&self, snapshot: &dominion_core::hosted::Snapshot) -> Result<(), String> {
        let mut execution = self.state.store.get("execution", self.execution_id)
            .ok_or("Rust 코어 실행 기록이 없습니다.")?;
        execution["runtime"] = json!("dominion-core");
        execution["modelCalls"]=json!(execution["modelCalls"].as_u64().unwrap_or(0).max(snapshot.model_calls as u64));
        execution["kernelState"] = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
        self.state.store.put("execution", &execution)
            .ok_or("Rust 코어 체크포인트를 저장할 수 없습니다.")?;
        Ok(())
    }
    fn cancelled(&self, _: &str) -> bool {
        self.state.is_cancelled(self.session_id)
            || self.state.cancellation_generation(self.session_id) != self.generation
    }
}

struct ChatKernelDispatcher<'a> {
    state: &'a AppState,
    tx: &'a UnboundedSender<Value>,
    session: &'a Value,
    request: &'a TurnRequest,
    session_id: &'a str,
    execution_id: &'a str,
    metrics: &'a std::sync::Mutex<Metrics>,
    prepared: &'a [(usize, u64, Value)],
    calls: &'a [ToolCall],
    round: usize,
    compat: bool,
    definitions: &'a [Value],
}
impl dominion_core::hosted::Dispatcher for ChatKernelDispatcher<'_> {
    type Output = (Value, bool, String);
    fn definitions(&self)->Vec<Value>{self.definitions.to_vec()}
    fn execute<'a>(&'a self, action: &'a dominion_core::hosted::Action)
        -> dominion_core::hosted::BoxFuture<'a, Result<dominion_core::hosted::DispatchResult<Self::Output>, String>> {
        Box::pin(async move {
            let (index, seq, _) = self.prepared.iter().find(|(index, _, _)|
                action.id == format!("{}:{}", self.round, self.calls[*index].id))
                .ok_or("Rust 코어 도구 호출의 원문을 찾을 수 없습니다.")?;
            let output = dispatch_tool_call(
                self.state, self.tx, self.session, self.request, self.session_id,
                self.execution_id, &self.calls[*index], action.arguments.clone(),
                self.metrics, *seq, self.round, *index, self.compat, "model", None, None, None,
            ).await?;
            Ok(dominion_core::hosted::DispatchResult {
                success: output.1, observation: output.0.clone(), output,
            })
        })
    }
}

struct KernelCheckpoint<'a> {
    state: &'a AppState,
    execution_id: &'a str,
    budget: dominion_core::budget::BudgetLedger,
}
impl Drop for KernelCheckpoint<'_> {
    fn drop(&mut self) {
        if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
            execution["runtime"] = json!("dominion-core");
            execution["budget"] = self.budget.snapshot();
            self.state.store.put("execution", &execution);
        }
    }
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
