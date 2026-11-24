//! Embeddable SDK using the exact same native agent loop as the Dominion app.
//! A host owns its model, tool, approval, verification and durable journal adapters.
//! The SDK does not create another scheduler, database or implicit authority.
pub use dominion_core as runtime;
pub use dominion_algorithms as algorithms;
pub use dominion_core::runtime_port::{AgentLoop, Context, Flow, Host, ModelDecision, model_call};
pub use dominion_core::hosted::{Action, BoxFuture, Dispatcher, DispatchResult, Journal, Receipt, Snapshot, Status, dispatch_batch};
pub use dominion_core::budget::BudgetLedger;
pub use dominion_core::run_event::{RunEventRef, RunEventEnvelope, RunEventPage, RunLifecycle};
pub use dominion_algorithms::engine::Engine;

/// The same admission, budget, iteration and journal path used by product chat,
/// rooms and SDK runs. No server, model vendor or global configuration required.
pub async fn run<H: Host>(host: &mut H, context: Context<'_>) -> Result<(), String> {
    AgentLoop::run(&Engine, host, context).await
}

#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::{Client, ClientError, StartRun, AgentRun, RunActions, RunStatus, RunSummary, SourceRef, InferenceRoute, InferenceSupport};
