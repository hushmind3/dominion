//! External connectors and delivery adapters, never a parallel agent kernel.
//! They may provide tools or channel transport, but the native dispatcher
//! retains permission checks, effects, cancellation and execution receipts.
pub mod browser;
pub mod bridge;
pub mod channel_adapter;
pub mod discord;
pub mod github_work;
pub mod google_work;
pub mod mcp;
pub mod telegram;

// A single port into optional MCP capabilities. The native runtime still
// owns model turns, approvals, budget, cancellation and operation receipts.
use crate::state::AppState;
use serde_json::Value;
pub async fn tool_definitions(state:&AppState)->Vec<Value>{ mcp::definitions(state).await }
pub fn requires_tool_approval(state:&AppState,name:&str)->bool{
    mcp::requires_approval(state,name)
}
pub async fn execute_tool(state:&AppState,name:&str,args:&Value)->Result<Value,String>{
    if !name.starts_with("mcp__") {
        return Err(format!("외부 도구 이름이 아닙니다: {name}"));
    }
    mcp::call(state,name,args).await
}
