//! Background continuation is an automation service, never intrinsic core.
use crate::{engine::chat,state::AppState};
use serde_json::Value;
use tokio::sync::mpsc;
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

