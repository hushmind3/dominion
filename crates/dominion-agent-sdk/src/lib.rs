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
pub mod extensions;
pub mod plugin_protocol;

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

/// Optional request-time compaction for external hosts. Only the cloned
/// provider view is reduced; the original history remains available for
/// independent verification and persistence by the caller. This adapter
/// invokes the same context policy and Harness as the product.
struct CompactModel<'a,M> {
    inner: &'a mut M,
    tool_trace_bytes: usize,
}
impl<M: ModelPort> ModelPort for CompactModel<'_,M> {
    async fn complete(&mut self, history: &[Value], visible: &[Value])
        -> Result<ModelResponse,String> {
        let wire = context::bounded_tool_trace(history,self.tool_trace_bytes);
        self.inner.complete(&wire,visible).await
    }
    fn cancelled(&self)->bool { self.inner.cancelled() }
}

/// Execute with a bounded historical tool trace on every model request.
/// The byte limit applies to completed tool frames, not the full context
/// window or model tokens. Incomplete frames and ordinary dialogue are
/// preserved. The host must persist and make original evidence retrievable.
pub async fn execute_compacted<A: AgentLoop,M: ModelPort,T: ToolPort>(
    algorithm: &mut A,
    model: &mut M,
    tools: &mut T,
    history: &mut Vec<Value>,
    catalog: &[Value],
    options: RunOptions,
    tool_trace_bytes: usize,
) -> Result<PortRunResult,String> {
    let mut adapter = CompactModel {inner:model,tool_trace_bytes};
    execute(algorithm,&mut adapter,tools,history,catalog,options).await
}
