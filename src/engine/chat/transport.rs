//! Transport event adapter. It owns no provider selection or tool execution.
use super::{run_turn, runtime::run_turn_http, TurnRequest};
use crate::state::AppState;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

pub async fn run(state: AppState, input: Value, tx: UnboundedSender<Value>) {
    run_with_http_epoch(state,input,tx,None).await
}

/// A request admitted by /api/chat carries the cancellation generation read
/// synchronously by the HTTP handler, before the engine is spawned.
pub(crate) async fn run_with_http_epoch(
    state: AppState, input: Value, tx: UnboundedSender<Value>,
    accepted_cancel_generation: Option<u64>,
) {
    let session_hint = input
        .get("sessionId")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let result = match TurnRequest::from_value(&input) {
        Ok(request) => if accepted_cancel_generation.is_some() {
            run_turn_http(&state, &request, &tx, accepted_cancel_generation).await
        } else {
            run_turn(&state, &request, &tx).await
        },
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
