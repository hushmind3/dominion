//! Optional host integrations; no adapter lives in the agent execution kernel.
//! The core owns planning, approvals, cost admission and durable tool receipts.
pub mod accounts;
pub mod channels;
pub mod mcp;
pub mod browser;
pub mod skills;
pub mod registry;
pub mod network;
pub mod local_tools;
pub mod agent_bridge;
use crate::state::AppState;
use std::sync::Arc;

/// Shared MCP transport handles, independent of the core agent state machine.
#[derive(Clone, Default)]
pub struct IntegrationState {
    pub(crate) mcp_sessions: Arc<mcp::SessionPool>,
}

/// Connectors are started by runtime composition, never the agent job worker.
pub fn start(state: AppState) {
    channels::adapter::start(state.clone());
    channels::discord::start(state.clone());
    channels::telegram::start(state);
}
