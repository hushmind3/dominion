//! The smallest entrypoint: natural-language turn -> native execution -> result.
//! Providers, tools, memory, rooms, jobs and other product subsystems live
//! outside this module. Historical API paths are reexported by engine::mod.
use super::chat;
use crate::state::AppState;
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Default, Clone)]
pub struct TurnStats {
    pub tool_success: Vec<String>,
    pub tool_errors: Vec<String>,
    pub evidence: Vec<Value>,
}

/// Keep both success receipts and failed effects for the SAME decision
/// kernel. Dropping tool_error meant a prior external effect marked unknown
/// could disappear from autonomous replay admission.
pub(super) fn collect_tool_event(stats: &mut TurnStats, event: &Value) {
    let Some(name) = event.get("name").and_then(Value::as_str)
        .filter(|name| !name.is_empty()) else {return;};
    match event.get("type").and_then(Value::as_str) {
        Some("tool_end") => {
            stats.tool_success.push(name.to_string());
            stats.evidence.push(serde_json::json!({
                "tool": name, "callId": event.get("id"),
                "arguments": event.get("arguments"), "result": event.get("result"),
            }));
        }
        Some("tool_error") => {
            stats.tool_errors.push(name.to_string());
            let result = event.get("result").filter(|result| !result.is_null())
                .cloned().unwrap_or_else(||serde_json::json!({
                    "error":event.get("error").and_then(Value::as_str).unwrap_or("tool failed"),
                }));
            let uncertain = result["outcomeUnknown"]==true || result["effectUnknown"]==true
                || event["outcomeUnknown"]==true || event["effectUnknown"]==true;
            let mut receipt=serde_json::json!({
                "tool":name,"status":"failed","callId":event.get("id"),
                "arguments":event.get("arguments"),"result":result,
            });
            if uncertain {receipt["outcomeUnknown"]=serde_json::json!(true);}
            if event["dispatched"]==false {receipt["dispatched"]=serde_json::json!(false);}
            stats.evidence.push(receipt);
        }
        _ => {}
    }
}

/// Runs a turn and collects tool evidence for the goal harness.
pub async fn run_quiet_outcome(
    state: &AppState,
    request: &chat::TurnRequest,
) -> (Result<Option<Value>, String>, TurnStats) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let collector = tokio::spawn(async move {
        let mut stats = TurnStats::default();
        while let Some(event) = receiver.recv().await {
            collect_tool_event(&mut stats,&event);
        }
        stats
    });
    let result = chat::run_turn_background(state, request, &sender).await;
    drop(sender);
    let stats = collector.await.unwrap_or_default();
    (result, stats)
}

/// Background resume that never records a second owner message.
pub async fn run_quiet_resume(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let drain = tokio::spawn(async move {
        while receiver.recv().await.is_some() {}
    });
    let result = chat::run_turn_resume(state, request, &sender).await;
    drop(sender);
    let _ = drain.await;
    result
}

/// Compatibility wrapper; harnesses that need evidence after a model/runtime
/// error use run_quiet_outcome instead of discarding completed tool work.
pub async fn run_quiet_with_stats(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<(Option<Value>, TurnStats), String> {
    let (result, stats) = run_quiet_outcome(state, request).await;
    result.map(|message| (message, stats))
}

/// Runs a turn with nobody listening on the event stream (background jobs).
/// The channel stays open so tool execution is not treated as cancelled.
pub async fn run_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    run_quiet_with_stats(state, request)
        .await
        .map(|(message, _)| message)
}

/// A room worker owns generation and persistence independently of open tabs.
/// Forward transient progress to connected UIs; the stored message is truth.
pub async fn run_room_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
    source_message_id: &str,
) -> Result<Option<Value>, String> {
    let session_id = request
        .session_id
        .clone()
        .ok_or("그룹 세션이 필요합니다.")?;
    let session = state
        .store
        .get("session", &session_id)
        .ok_or("그룹 세션을 찾을 수 없습니다.")?;
    let room_id = session["roomId"]
        .as_str()
        .ok_or("그룹 대화 연결이 없습니다.")?
        .to_string();
    let agent_id = request.agent_id.clone();
    let turn_id = uuid::Uuid::new_v4().to_string();
    state.bus.emit(serde_json::json!({"type":"room_event","roomId":room_id,"sessionId":session_id,"agentId":agent_id,"turnId":turn_id,"event":{"type":"turn_start","sourceMessageId":source_message_id}}));
    let bus = state.bus.clone();
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let stream_room = room_id.clone();
    let stream_session = session_id.clone();
    let stream_agent = agent_id.clone();
    let stream_turn = turn_id.clone();
    let collector = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            bus.emit(serde_json::json!({"type":"room_event","roomId":stream_room,"sessionId":stream_session,"agentId":stream_agent,"turnId":stream_turn,"event":event}));
        }
    });
    let result = chat::run_room_turn(state, request, &sender, source_message_id).await;
    drop(sender);
    let _ = collector.await;
    let event = match &result {
        Ok(Some(message)) => {
            serde_json::json!({"type":"assistant_saved","messageId":message["id"]})
        }
        Ok(None) => serde_json::json!({"type":"done"}),
        Err(error) => serde_json::json!({"type":"error","message":error,"terminal":true}),
    };
    state.bus.emit(serde_json::json!({"type":"room_event","roomId":room_id,"sessionId":session_id,"agentId":agent_id,"turnId":turn_id,"event":event}));
    result
}

