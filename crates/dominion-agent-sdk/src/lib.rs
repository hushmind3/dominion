//! In-process agent SDK. The application and external hosts execute the same
//! native loop; this crate adds no provider, database, or retry engine.
//! Hosts supply model/tool adapters, permission checks and a durable journal.
pub use dominion_algorithms as algorithms;
pub use dominion_core as runtime;
pub use dominion_algorithms::engine::Engine;
pub use dominion_core::budget::BudgetLedger;
pub use dominion_core::domain::RunStatus;
pub use dominion_core::hosted::{Action, BoxFuture, DispatchResult, Dispatcher, Journal, Receipt, Snapshot, dispatch_batch};
pub use dominion_core::runtime_port::{AgentLoop, Context, Flow, Host, ModelDecision, model_call};

/// Execute a host's turn using the same algorithm as the DOMINION application.
/// The supplied context must refer to a running, settled native checkpoint.
/// A returned error is not permission to replay uncertain effects.
pub async fn run<H: Host>(host: &mut H, context: Context<'_>) -> Result<(), String> {
    AgentLoop::run(&Engine, host, context).await
}

mod embedded;
pub use embedded::{run_embedded, CheckpointStore, EmbeddedLimits, ModelPort, ModelTurn, TurnDecision, Verification, Verifier};
