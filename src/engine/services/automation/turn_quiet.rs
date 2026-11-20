//! Home, jobs and autonomous workloads use this adapter into the pure core.
//! Memory/context/model/tool implementations stay in their existing services.
use crate::{engine::{chat,core},state::AppState};
use serde_json::Value;
use tokio::sync::mpsc;

struct ChatPort<'a> { state: &'a AppState }
impl core::ExecutionPort for ChatPort<'_> {
    type Request = chat::TurnRequest;

    async fn execute(&self, request: &chat::TurnRequest,
        events: mpsc::UnboundedSender<Value>) -> Result<Option<Value>, String> {
        chat::run_turn_background(self.state, request, &events).await
    }
}

/// Reuses the same concrete chat runtime; the core owns result and evidence
/// settlement while the service owns all model/context/transport configuration.
pub async fn run_quiet_outcome(
    state: &AppState, request: &chat::TurnRequest,
) -> (Result<Option<Value>, String>, core::TurnStats) {
    core::run_with_events(&ChatPort {state}, request).await
}

pub async fn run_quiet_with_stats(
    state: &AppState, request: &chat::TurnRequest,
) -> Result<(Option<Value>, core::TurnStats), String> {
    let (result, stats) = run_quiet_outcome(state, request).await;
    result.map(|message| (message, stats))
}

pub async fn run_quiet(
    state: &AppState, request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    run_quiet_with_stats(state, request).await.map(|(message, _)| message)
}
