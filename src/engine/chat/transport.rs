//! Transport event adapter. It owns no provider selection or tool execution.
use super::{run_turn, TurnRequest};
use crate::state::AppState;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

/// Per-turn stop tombstone prevents a cancellation that reached the server
/// before the chat POST from being undone by normal session resume. The stop
/// endpoint associates the UUID with the immutable session, so a request ID
/// from another session cannot cancel this turn.
pub(super) fn request_cancelled(
    state: &AppState, session_id: &str, request_id: Option<&str>
) -> bool {
    request_id
        .and_then(|id| state.store.get("chat_cancel_request", id))
        .is_some_and(|record| {
            record["sessionId"].as_str()==Some(session_id)
                && record["cancelled"]==true
        })
}

pub async fn run(state: AppState, input: Value, tx: UnboundedSender<Value>) {
    let session_hint = input
        .get("sessionId")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let result = match TurnRequest::from_value(&input) {
        Ok(request) => run_turn(&state, &request, &tx).await,
        Err(message) => Err(message),
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
