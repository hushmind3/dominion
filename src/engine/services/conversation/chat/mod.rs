//! Native Rust chat engine: shared request contract and stable public entry points.
mod approval;
mod cancellation;
use dominion_agent_kernel as budget;
mod tool_schedule;
use dominion_context_policy as trace_budget;
#[path = "../../conversation/chat_context_window.rs"]
mod context_window;
#[path = "../../conversation/chat_prompt_sources.rs"]
pub(crate) mod prompt_sources;
#[path = "../../providers/chat_codex_ws.rs"]
mod codex_ws;
#[path = "../../conversation/chat_context.rs"]
mod context;
#[path = "../../conversation/chat_continuity.rs"]
mod continuity;
#[path = "../../conversation/chat_room_context.rs"]
mod room_context;

#[path = "../../decision/chat_evidence.rs"]
mod evidence;
pub(crate) use evidence::prepare_owner_action;
mod protocol;
#[path = "../../providers/chat_provider.rs"]
mod provider;
mod runtime;
#[path = "../../tools/chat_tool_projection.rs"]
mod tool_projection;
#[path = "../../tools/chat_capability.rs"]
mod capability;
mod outcome;
#[cfg(test)]
mod tests;
#[path = "../../providers/chat_transport.rs"]
mod transport;

pub use protocol::parse_text_tool_calls;
pub use runtime::{run_room_turn, run_turn, run_turn_background, run_turn_resume};

/// Cancel approvals bound to a session that is being archived/reset.
pub(crate) fn close_session_approvals(state: &crate::state::AppState, session_id: &str) {
    approval::close_pending_approvals(state, session_id);
}

/// Static prompt preview for the room settings UI.
pub(crate) fn system_prompt_preview(
    state: &crate::state::AppState,
    room_id: &str,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let tools = state
        .store
        .get("room", room_id)
        .and_then(|room| room["toolsEnabled"].as_bool())
        .unwrap_or(true);
    room_context::preview(state, room_id, agent_id, tools)
}


/// The consumer settings view uses Sub2's own compiler and memory scope.
/// This is read-only: no model turn, native run or tool execution is started.
pub(crate) fn private_system_prompt_preview(
    state:&crate::state::AppState,session_id:Option<&str>,agent_id:Option<&str>,
    input:&str,tools_enabled:bool,korean_reasoning:bool,
)->Result<serde_json::Value,String>{
    use serde_json::json;
    if let Some(id)=agent_id{
        if state.store.get("agent",id).is_none(){return Err("에이전트를 찾을 수 없습니다.".into());}
    }
    if let Some(id)=session_id{
        let session=state.store.get("session",id).ok_or("대화를 찾을 수 없습니다.")?;
        if session["roomId"].is_string(){return Err("그룹 세션은 별도 미리보기를 사용하세요.".into());}
    }
    let chosen=session_id.unwrap_or("__private_prompt_preview__");
    let request=TurnRequest::from_value(&json!({
        "input":input,"sessionId":chosen,"agentId":agent_id,
        "conversationMode":"chat","toolsEnabled":tools_enabled,
        "toolsExecutionEnabled":tools_enabled,
        "reasoningLanguage":if korean_reasoning{"ko"}else{"en"},
    }))?;
    let compiled=context::system_prompt_with_manifest(state,&request,chosen,input,None);
    let (_,scope)=context::configured_private_prompt(state,agent_id);
    let own=if let Some(id)=agent_id{
        state.store.get("agent",id)
    }else{
        state.store.get("assistant_settings","global")
    }.and_then(|record|record["systemPromptOverride"].as_str().map(str::to_owned));
    let agent_name=agent_id.and_then(|id|state.store.get("agent",id))
        .and_then(|agent|agent["name"].as_str().map(str::to_owned));
    let owner_enabled=state.store.get("assistant_settings","global")
        .is_none_or(|s|s["enabled"]!=false);
    let system_chars=compiled.prompt.chars().count();
    let dynamic_context=format!("{}{}",compiled.source_instructions,compiled.dynamic_context);
    let context_chars=dynamic_context.chars().count();
    Ok(json!({
        "systemPrompt":compiled.prompt,"dynamicContext":dynamic_context,
        "systemChars":system_chars,"systemTokensEstimated":system_chars.div_ceil(4),
        "contextChars":context_chars,"contextTokensEstimated":context_chars.div_ceil(4),
        "agentId":agent_id,"agentName":agent_name,"ownerEnabled":owner_enabled,
        "sampleInput":input,"sessionId":session_id,
        "ownOverrideActive":own.is_some(),"ownOverrideText":own,
        "effectiveOverrideScope":scope,
    }))
}

/// Clean up old durable approvals at startup and in the maintenance loop.
pub(crate) fn expire_stale_approvals(state: &crate::state::AppState) -> usize {
    approval::expire_stale_approvals(state)
}
use serde_json::Value;
use std::time::Duration;
pub use transport::run;

const MAX_ROUNDS: usize = 8;

const MAX_TOOL_CALLS: usize = 40;

/// Hard token budget per turn. Usage is read from the provider stream.
const MAX_TOKENS: u64 = 250_000;

const MAX_ELAPSED: Duration = Duration::from_secs(600);

const HISTORY_LIMIT: usize = 40;

// One logical turn: a user prompt, optional agent/provider/model overrides.
// Workflows, rooms and the chat UI all go through this.

#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub session_id: Option<String>,
    /// Optional browser turn correlation token, never a model instruction.
    pub request_id: Option<String>,
    pub input: String,
    pub attachment_ids: Vec<String>,
    pub agent_id: Option<String>,
    /// Explicit plain conversation: no implicit session agent or tool authority.
    /// Omitted for backward compatibility with historical agent/SDK callers.
    pub direct_chat: bool,
    /// Explicit opt-in for read-only public web search in a direct Chat turn.
    /// Never extends native/agent/MCP authority beyond web_search/http_fetch.
    pub web_search: bool,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub tools_enabled: bool,
    /// Explicit overall tool execution permission. `tools_enabled` only
    /// controls whether the model may call tools directly.
    pub tools_execution_enabled: bool,
    pub thinking: Option<bool>,
    pub reasoning_effort: Option<String>,
    pub reasoning_language: Option<String>,
    pub auto_learning: bool,
    pub system_override: Option<String>,
    /// Root task episode this turn attempts. Derived prompts (goal iterations,
    /// room speakers) must attach to an existing root instead of creating a
    /// new owner instruction.
    pub task_id: Option<String>,
    pub parent_task_id: Option<String>,
    /// Frozen contract revision the caller believes it is executing.
    pub task_revision: Option<String>,
    /// `parent` means the parent lifecycle owns final verification (a goal
    /// reviewer). The child still runs deterministic checks and records
    /// evidence, but does not call a second semantic reviewer.
    pub verification_owner: Option<String>,
}

impl TurnRequest {
    pub fn from_value(value: &Value) -> Result<Self, String> {
        crate::engine::reasoning::validate(value)?;
        let text = value
            .get("input")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let attachment_ids = crate::engine::attachments::ids(&value["attachmentIds"])?;
        if text.is_empty() && attachment_ids.is_empty() {
            return Err("메시지가 비어 있습니다.".into());
        }
        let string = |key: &str| {
            value
                .get(key)
                .and_then(|item| item.as_str())
                .filter(|item| !item.is_empty())
                .map(str::to_string)
        };
        Ok(Self {
            session_id: string("sessionId"),
            request_id: match string("requestId").or_else(||string("clientTurnId")) {
                Some(value) if uuid::Uuid::parse_str(&value).is_ok() => Some(value),
                Some(_) => return Err("채팅 요청 ID 형식이 올바르지 않습니다.".into()),
                None => None,
            },
            input: text,
            attachment_ids,
            agent_id: string("agentId"),
            direct_chat: match value.get("conversationMode").and_then(Value::as_str) {
                Some("chat") => true,
                Some("agent") => false,
                None => value.get("directChat").and_then(Value::as_bool).unwrap_or(false),
                Some(_) => return Err("채팅 실행 모드가 올바르지 않습니다.".into()),
            },
            web_search: value.get("webSearch").and_then(Value::as_bool).unwrap_or(false),
            provider_id: string("providerId"),
            model: string("model"),
            tools_enabled: value
                .get("toolsEnabled")
                .and_then(|item| item.as_bool())
                .unwrap_or(true),
            tools_execution_enabled: value
                .get("toolsExecutionEnabled")
                .and_then(|item| item.as_bool())
                .unwrap_or(true),
            thinking: value.get("thinking").and_then(|item| item.as_bool()),
            reasoning_effort: match string("reasoningEffort") {
                Some(effort) if crate::engine::reasoning::valid_effort(&effort) =>
                {
                    Some(effort)
                }
                Some(_) => {
                    return Err(
                        "추론 강도 값이 올바르지 않습니다.".into(),
                    )
                }
                None => None,
            },
            reasoning_language: string("reasoningLanguage"),
            auto_learning: value
                .get("autoLearning")
                .and_then(|item| item.as_bool())
                .unwrap_or(false),
            system_override: string("systemOverride"),
            task_id: string("taskId"),
            parent_task_id: string("parentTaskId"),
            task_revision: string("taskRevision"),
            verification_owner: string("verificationOwner"),
        })
    }
}

#[derive(Clone, Debug, Default)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct Completion {
    continuity: Option<Value>,
    // Internal working history; never serialized into client message metadata.
    projection: Option<continuity::Projection>,
    content: String,
    reasoning: String,
    tool_calls: Vec<ToolCall>,
    usage: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    // None means the provider did not report cache usage, not a cache miss.
    cached_prompt_tokens: Option<u64>,
    usage_exact: bool,
    first_delta_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderRef {
    id: String,
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    tool_mode: String,
    reasoning_protocol: String,
    local: bool,
    context_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    requests_per_minute: Option<u64>,
    kind: String,
    account_id: Option<String>,
}

// SSE entry point used by `POST /api/chat`.

#[derive(Default)]
struct Metrics {
    model_calls: u32,
    tool_calls: u32,
    iterations: u32,
    tokens: u64,
    first_token_ms: Option<u64>,
    model_elapsed_ms: u64,
    tool_elapsed_ms: u64,
    context_elapsed_ms: u64,
}

#[cfg(test)]
mod consumer_chat_contract_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn consumer_direct_chat_and_client_turn_id_do_not_inherit_agent_permissions(){
        let request_id=uuid::Uuid::new_v4().to_string();
        let request=TurnRequest::from_value(&json!({
            "input":"안녕하세요","directChat":true,
            "clientTurnId":request_id,"toolsEnabled":false,
            "toolsExecutionEnabled":false
        })).unwrap();
        assert!(request.direct_chat);
        assert_eq!(request.request_id.as_deref(),Some(request_id.as_str()));
        assert!(!request.tools_enabled);
        assert!(!request.tools_execution_enabled);
        assert!(!TurnRequest::from_value(&json!({
            "input":"hi","clientTurnId":"not-a-uuid"
        })).is_ok());
    }
}
