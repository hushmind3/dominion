//! Transport event adapter. It owns no provider selection or tool execution.
use super::{run_turn, run_turn_plain, TurnRequest};
use crate::state::AppState;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

enum ChatMode {
    Plain,
    Agent,
}

/// Validate and bind an owner request before creating a session or SSE task.
/// The runtime still rechecks its lease, cancellation and route at execution.
pub struct ChatCommand {
    request: TurnRequest,
    mode: ChatMode,
}
impl ChatCommand {
    pub fn admit(state: &AppState, input: &Value) -> Result<Self, String> {
        let mut request = TurnRequest::from_value(input)?;
        let mode = match input.get("mode").and_then(Value::as_str) {
            Some("chat") => ChatMode::Plain,
            Some("agent") | None if input["mode"].is_null() || input["mode"] == "agent" => {
                ChatMode::Agent
            }
            _ => return Err("지원하지 않는 대화 모드입니다.".into()),
        };
        let session = match request.session_id.as_deref() {
            Some(id) => state
                .store
                .get("session", id)
                .ok_or("세션을 찾을 수 없습니다.")?,
            None => json!({}),
        };
        if session["roomId"].is_string()
            || !session["channelContext"].is_null()
            || matches!(session["kind"].as_str(), Some("room" | "channel"))
        {
            return Err("그룹·외부 채널 대화는 해당 대화 화면에서 보내세요.".into());
        }
        if matches!(mode, ChatMode::Plain) {
            if request.task_id.is_some()
                || request.parent_task_id.is_some()
                || request.verification_owner.is_some()
            {
                return Err("일반 채팅은 사용자 대화에서만 시작할 수 있습니다.".into());
            }
            request.agent_id = None;
            request.tools_enabled = false;
            request.tools_execution_enabled = false;
            request.auto_learning = false;
            request.system_override = None;
        } else if request.agent_id.is_none() {
            request.agent_id = session["agentId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_string);
        }
        if let Some(id) = request.agent_id.as_deref() {
            if !state
                .store
                .get("agent", id)
                .is_some_and(|agent| agent["enabled"] != false)
            {
                return Err("대화할 AI가 없거나 사용 중지되었습니다. AI 설정을 확인하세요.".into());
            }
        }
        let route = super::provider::select_provider(state, &request, &session)?;
        request.provider_id = Some(route.id);
        request.model = Some(route.model);
        Ok(Self { request, mode })
    }
}

pub async fn run(state: AppState, input: Value, tx: UnboundedSender<Value>) {
    match ChatCommand::admit(&state, &input) {
        Ok(command) => run_command(state, command, tx).await,
        Err(message) => {
            let _ = tx.send(json!({"type":"error","message":message}));
            let _ = tx.send(json!({"type":"done","sessionId":input["sessionId"]}));
        }
    }
}

pub async fn run_command(state: AppState, command: ChatCommand, tx: UnboundedSender<Value>) {
    let ChatCommand { request, mode } = command;
    let session_hint = request.session_id.clone().unwrap_or_default();

    let result = match mode {
        ChatMode::Plain => run_turn_plain(&state, &request, &tx).await,
        ChatMode::Agent => run_turn(&state, &request, &tx).await,
    };

    match result {
        Ok(Some(message)) => {
            let _ = tx.send(json!({
                "type": "assistant_saved",
                "messageId": message.get("id"),
                "sessionId": message.get("session_id"),
            }));
            let _ = tx.send(json!({
                "type": "done",
                "sessionId": message.get("session_id").cloned().unwrap_or(json!(session_hint)),
            }));
        }
        Ok(None) => {
            let _ = tx.send(json!({ "type": "done", "sessionId": session_hint }));
        }
        Err(message) => {
            let _ = tx.send(json!({ "type": "error", "message": message }));
            let _ = tx.send(json!({ "type": "done", "sessionId": session_hint }));
        }
    }
}

// Executes a full turn (model + tools + persistence) without owning the SSE
// transport, so background jobs can reuse exactly the same engine.
