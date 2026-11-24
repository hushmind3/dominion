//! Public, provider-independent agent runtime API.
//! The application and external hosts share the exact same types and
//! implementations. No database, server, UI or concrete provider is linked.
//! Model/tool ports are trusted host adapters; the host remains responsible
//! for permissions, approvals, persistence, external effect reconciliation
//! and independent verification of work.
pub use dominion_turn_core as runtime;
pub use dominion_agent_loop as workflows;
pub use dominion_agent_kernel as decisions;
pub use dominion_context_policy as context;
pub use dominion_turn_core::agent_loop::{
    AgentLoop, Harness, ModelPort, ModelResponse, PortRunResult, PreparedResponse,
    RoundObservation, ToolFeedback, ToolPort, ToolRequest,
};
pub use dominion_turn_core::driver::{Driver, Step};
pub use dominion_turn_core::{ExecutionPort, TurnStats, run_with_events};
pub use dominion_agent_loop::{run, select_named, ActiveLoop};
use serde_json::Value;

/// Explicit limits and permissions for one host-owned invocation.
/// The driver enforces a model-round limit and its existing tool-proposal
/// accounting contract. A proposal budget is NOT an external spending cap.
#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    pub max_model_rounds: usize,
    pub tool_budget: usize,
    pub tools_enabled: bool,
    pub has_plan: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { max_model_rounds: 8, tool_budget: 0, tools_enabled: false, has_plan: false }
    }
}

/// Run using a built-in algorithm or an external AgentLoop implementation.
/// This is a convenience entry point into the existing Harness, not a new
/// runtime. Transcript mutations remain visible to the caller on failure.
/// A successful return represents a model answer, not verified task success.
pub async fn execute<A: AgentLoop, M: ModelPort, T: ToolPort>(
    algorithm: &mut A,
    model: &mut M,
    tools: &mut T,
    history: &mut Vec<Value>,
    catalog: &[Value],
    options: RunOptions,
) -> Result<PortRunResult, String> {
    let mut driver = Driver::new(options.max_model_rounds, options.tool_budget);
    let visible = if options.tools_enabled { catalog } else { &[] };
    run(algorithm, &mut driver, model, tools, history, visible,
        options.tools_enabled, options.has_plan).await
}
