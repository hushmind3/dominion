//! Native Rust chat engine: shared request contract and stable public entry points.
mod approval;
mod budget;
mod codex_ws;
#[path = "../../addons/knowledge/chat_context.rs"]
mod context;
mod context_engine;
mod continuity;
#[path = "../../addons/integrations/chat_room_context.rs"]
mod room_context;
mod web_research;

pub(crate) fn estimate_text_tokens(text:&str)->u64 {room_context::estimate(text) as u64}
mod evidence;
pub(crate) use evidence::prepare_owner_action;
mod protocol;
mod provider;
mod runtime;
#[cfg(test)]
mod tests;
mod transport;

pub use protocol::parse_text_tool_calls;
pub use runtime::{run_room_turn, run_turn, run_turn_background, run_turn_resume};
pub(crate) use transport::run_with_http_epoch;

/// The coordinator owns the fixture and assertions. Only aliases to the real
/// runtime functions cross this boundary; candidate-owned test helpers cannot
/// manufacture the oracle's observations.
#[cfg(test)]
pub(crate) mod context_oracle {
    pub(crate) use super::room_context::compile;
    pub(crate) use super::provider::select_provider;
    pub(crate) use super::continuity::{dependencies,prepare,project,commit,commit_projection};
}

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

/// Read-only preview of the same private-chat prompt compiler used by
/// ordinary turns. No fake inference, history writes, or remote model calls.
/// Retrieved context is a separate message, not part of the system role.
pub(crate) fn private_system_prompt_preview(
    state: &crate::state::AppState,
    session_id: Option<&str>,
    agent_id: Option<&str>,
    input: &str,
    tools_enabled: bool,
    korean_reasoning: bool,
) -> Result<Value, String> {
    if let Some(id) = session_id {
        let session = state.store.get("session", id)
            .ok_or("선택한 대화를 찾을 수 없습니다.")?;
        if session["roomId"].is_string() {
            return Err("그룹 대화의 시스템 프롬프트는 그룹 화면에서 확인하세요.".into());
        }
    }
    let agent = match agent_id {
        Some(id) => Some(state.store.get("agent", id)
            .ok_or("선택한 에이전트를 찾을 수 없습니다.")?),
        None => None,
    };
    let request = TurnRequest::from_value(&serde_json::json!({
        "input":input,
        "sessionId":session_id,
        "agentId":agent_id,
        "toolsEnabled":tools_enabled,
        "toolsExecutionEnabled":tools_enabled,
        "reasoningLanguage":if korean_reasoning{"ko"}else{"original"}
    }))?;
    let session = session_id.unwrap_or("prompt-preview:private");
    let compiled = context::system_prompt_with_manifest_at(
        state,&request,session,&request.input,None,None
    );
    let owner = state.store.get("assistant_settings","global")
        .unwrap_or_else(||serde_json::json!({"id":"global","instructions":"","enabled":true}));
    let own_override=if let Some(agent)=agent.as_ref(){
        agent["systemPromptOverride"].as_str()
    }else{owner["systemPromptOverride"].as_str()};
    let effective_scope=if agent.as_ref().and_then(|a|a["systemPromptOverride"].as_str()).is_some(){
        Some("agent")
    }else if owner["systemPromptOverride"].is_string(){Some("global")}else{None};
    let prompt_tokens = room_context::estimate(&compiled.prompt);
    let context_tokens = room_context::estimate(&compiled.dynamic_context);
    Ok(serde_json::json!({
        "systemPrompt":compiled.prompt,
        "dynamicContext":compiled.dynamic_context,
        "systemChars":compiled.prompt.chars().count(),
        "systemTokensEstimated":prompt_tokens,
        "contextChars":compiled.dynamic_context.chars().count(),
        "contextTokensEstimated":context_tokens,
        "agentId":agent_id,
        "agentName":agent.as_ref().and_then(|a|a["name"].as_str()),
        "agentInstructions":agent.as_ref().and_then(|a|a["systemPrompt"].as_str()).unwrap_or(""),
        "ownerInstructions":owner["instructions"],
        "ownerEnabled":owner["enabled"] != false,
        "ownOverrideActive":own_override.is_some(),
        "ownOverrideText":own_override,
        "effectiveOverrideScope":effective_scope,
        "sessionId":session_id,
        "sampleInput":request.input,
        "toolsEnabled":tools_enabled,
        "reasoningLanguage":if korean_reasoning{"ko"}else{"original"},
        "scope":"private-chat",
        "note":"현재 코드를 통한 일반 채팅 프롬프트 조립 미리보기입니다. 직접 편집한 전체 프롬프트는 일반 채팅에서 기본 시스템 지침을 대체합니다. 도구 스키마·실제 이전 대화·캐시 상태는 별도입니다."
    }))
}

/// Clean up old durable approvals at startup and in the maintenance loop.
pub(crate) fn expire_stale_approvals(state: &crate::state::AppState) -> usize {
    approval::expire_stale_approvals(state)
}
use serde_json::Value;
use std::time::Duration;

const MAX_ROUNDS: usize = 8;

const MAX_TOOL_CALLS: usize = 40;

/// Hard token budget per turn. Usage is read from the provider stream.
pub(crate) const MAX_TOKENS: u64 = 250_000;

const MAX_ELAPSED: Duration = Duration::from_secs(600);

const HISTORY_LIMIT: usize = 40;

// One logical turn: a user prompt, optional agent/provider/model overrides.
// Workflows, rooms and the chat UI all go through this.

#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub session_id: Option<String>,
    pub input: String,
    pub attachment_ids: Vec<String>,
    pub agent_id: Option<String>,
    /// Explicit model-only conversation: no agent binding, tool execution or learning.
    pub direct_chat: bool,
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
    pub fn effective_reasoning_language(&self) -> Option<&str> {
        match self.reasoning_language.as_deref() {
            Some("original") => None,
            None | Some("auto") => self.input.chars()
                .any(|ch| matches!(ch, '\u{AC00}'..='\u{D7A3}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}'))
                .then_some("ko"),
            language => language,
        }
    }

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
        let direct_chat = value.get("directChat").and_then(Value::as_bool).unwrap_or(false);
        let string = |key: &str| {
            value
                .get(key)
                .and_then(|item| item.as_str())
                .filter(|item| !item.is_empty())
                .map(str::to_string)
        };
        Ok(Self {
            session_id: string("sessionId"),
            input: text,
            attachment_ids,
            agent_id: if direct_chat { None } else { string("agentId") },
            direct_chat,
            provider_id: string("providerId"),
            model: string("model"),
            tools_enabled: !direct_chat && value
                .get("toolsEnabled")
                .and_then(|item| item.as_bool())
                .unwrap_or(true),
            tools_execution_enabled: !direct_chat && value
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
            auto_learning: !direct_chat && value
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
    finish_reason: Option<String>,
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
    /// Logical non-streaming helper calls; retries are separately receipted.
    auxiliary_calls: u32,
    tool_calls: u32,
    iterations: u32,
    tokens: u64,
    first_token_ms: Option<u64>,
    model_elapsed_ms: u64,
    tool_elapsed_ms: u64,
    context_elapsed_ms: u64,
}


#[cfg(test)]
mod private_prompt_preview_tests {
    use super::*;
    use crate::{config::Config,db::Store,state::AppState,vault::Vault};
    use serde_json::json;

    fn fixture() -> AppState {
        let dir=std::env::temp_dir().join(format!("dominion-prompt-preview-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir};
        AppState::new(
            config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path())
        )
    }

    #[test]
    fn private_preview_uses_real_owner_and_agent_instruction_compiler_without_writing_history(){
        let state=fixture();
        state.store.put("session",&json!({"id":"private:preview"})).unwrap();
        state.store.put("agent",&json!({
            "id":"worker","name":"검토자","systemPrompt":"TEST_AGENT_PROMPT_256"
        })).unwrap();
        state.store.put("assistant_settings",&json!({
            "id":"global","enabled":true,"instructions":"TEST_OWNER_RULE_384"
        })).unwrap();
        let preview=private_system_prompt_preview(
            &state,Some("private:preview"),Some("worker"),
            "코드 확인",false,true
        ).unwrap();
        let content=preview["systemPrompt"].as_str().unwrap();
        assert!(content.contains("DOMINION 지속 에이전트"));
        assert!(content.contains("TEST_AGENT_PROMPT_256"));
        assert!(content.contains("TEST_OWNER_RULE_384"));
        assert!(content.contains("도구가 비활성화"));
        assert_eq!(preview["scope"],"private-chat");
        assert!(preview["systemTokensEstimated"].as_u64().is_some());
        assert!(state.store.messages("private:preview").is_empty(),
            "preview must not add a user or assistant message");
        assert!(state.store.all("execution").is_empty(),
            "preview must not run or bill a model");
    }

    #[test]
    fn full_prompt_editor_overrides_all_builtin_private_text_including_empty() {
        let state=fixture();
        state.store.put("agent",&json!({
            "id":"worker","name":"worker","systemPrompt":"OLD_PERSONA",
            "systemPromptOverride":"단 1줄만 출력하세요."
        })).unwrap();
        state.store.put("assistant_settings",&json!({
            "id":"global","enabled":true,"instructions":"OLD_OWNER_RULE",
            "systemPromptOverride":"SHARED_CUSTOM"
        })).unwrap();
        let edited=private_system_prompt_preview(
            &state,None,Some("worker"),"코드 작성",true,true
        ).unwrap();
        assert_eq!(edited["systemPrompt"],"단 1줄만 출력하세요.");
        assert_eq!(edited["ownOverrideActive"],true);
        assert_eq!(edited["effectiveOverrideScope"],"agent");
        let fallback=private_system_prompt_preview(&state,None,None,"코드 작성",true,true).unwrap();
        assert_eq!(fallback["systemPrompt"],"SHARED_CUSTOM");
        let mut worker=state.store.get("agent","worker").unwrap();
        worker["systemPromptOverride"]=json!("");
        state.store.put("agent",&worker).unwrap();
        let empty=private_system_prompt_preview(&state,None,Some("worker"),"코드 작성",true,true).unwrap();
        assert_eq!(empty["systemPrompt"],"");
        assert_eq!(empty["systemTokensEstimated"],0);
        worker.as_object_mut().unwrap().remove("systemPromptOverride");
        state.store.put("agent",&worker).unwrap();
        let inherited=private_system_prompt_preview(&state,None,Some("worker"),"코드 작성",true,true).unwrap();
        assert_eq!(inherited["systemPrompt"],"SHARED_CUSTOM");
    }

    #[test]
    fn private_preview_respects_owner_opt_out_and_refuses_room_scope() {
        let state=fixture();
        state.store.put("agent",&json!({"id":"worker","name":"worker"})).unwrap();
        state.store.put("assistant_settings",&json!({
            "id":"global","enabled":false,"instructions":"HIDDEN_DISABLED_OWNER_PROMPT"
        })).unwrap();
        let preview=private_system_prompt_preview(
            &state,None,Some("worker"),"설명",false,false
        ).unwrap();
        assert!(!preview["systemPrompt"].as_str().unwrap()
            .contains("HIDDEN_DISABLED_OWNER_PROMPT"));
        state.store.put("session",&json!({"id":"group-room","roomId":"room-1"})).unwrap();
        let error=private_system_prompt_preview(
            &state,Some("group-room"),Some("worker"),"설명",true,false
        ).unwrap_err();
        assert!(error.contains("그룹"));
    }
}

#[cfg(test)]
mod direct_chat_request_tests {
    use super::TurnRequest;
    use serde_json::json;

    #[test]
    fn direct_chat_never_inherits_agent_tools_or_auto_learning() {
        let request = TurnRequest::from_value(&json!({
            "input":"안녕하세요","agentId":"existing-agent","directChat":true,
            "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":true,
        })).unwrap();
        assert!(request.direct_chat);
        assert_eq!(request.agent_id, None);
        assert!(!request.tools_enabled);
        assert!(!request.tools_execution_enabled);
        assert!(!request.auto_learning);
    }

    #[test]
    fn existing_agent_requests_remain_unchanged() {
        let request = TurnRequest::from_value(&json!({
            "input":"작업해","agentId":"existing-agent","toolsEnabled":true,
        })).unwrap();
        assert!(!request.direct_chat);
        assert_eq!(request.agent_id.as_deref(), Some("existing-agent"));
        assert!(request.tools_enabled);
    }
}
