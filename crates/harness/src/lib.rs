//! Dominion's provider-independent, Rust-first autonomous agent SDK.
//!
//! The SDK kernel lives in [`sdk::AgentSdk`]; the HTTP product, graphical
//! application, and native CLI use the same executor. There is no second
//! legacy agent loop in this crate.

use serde::{Deserialize, Serialize};

pub mod context;
pub mod openai;
pub mod policy;
pub mod provider;
pub mod sdk;
pub mod tool;
pub mod tools;

pub use context::ContextBudget;
pub use policy::{AllowAll, Decision, DenyWrites, Policy, ReadOnlyPolicy};
pub use provider::{ModelReply, ModelUsage, Provider};
pub use sdk::{AgentSdk, CommandVerifier, FileJournal, RunStatus, Session, Verification, Verifier};
pub use tool::{Permission, Tool, ToolCall, ToolSpec};

/// Model-neutral conversation entry, retaining exact tool-call correlation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Message {
    fn plain(role: Role, content: impl Into<String>) -> Self {
        Self { role, content: content.into(), tool_calls: Vec::new(), tool_call_id: None }
    }
    pub fn system(content: impl Into<String>) -> Self { Self::plain(Role::System, content) }
    pub fn user(content: impl Into<String>) -> Self { Self::plain(Role::User, content) }
    pub fn assistant(content: impl Into<String>) -> Self { Self::plain(Role::Assistant, content) }
    pub fn tool(content: impl Into<String>) -> Self { Self::plain(Role::Tool, content) }
    pub fn assistant_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self { role: Role::Assistant, content: content.into(), tool_calls, tool_call_id: None }
    }
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self { role: Role::Tool, content: content.into(), tool_calls: Vec::new(), tool_call_id: Some(call_id.into()) }
    }
}

/// Enforced execution bounds. Product and CLI may choose their own values.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_rounds: usize,
    pub max_tool_calls: usize,
    pub max_context_chars: usize,
    pub max_tool_output_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rounds: 24,
            max_tool_calls: 80,
            max_context_chars: 120_000,
            max_tool_output_chars: 8_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_keeps_instruction_and_correlates_tool_output() {
        let user = Message::user("정확히 요청한 작업");
        assert_eq!(user.content, "정확히 요청한 작업");
        let assistant = Message::assistant_calls("", vec![
            ToolCall { id:"c1".into(), name:"fs_read".into(), arguments:json!({"path":"x"}) }
        ]);
        let output = Message::tool_result("c1", "contents");
        assert_eq!(assistant.tool_calls[0].id, output.tool_call_id.unwrap());
        let copied = serde_json::to_value(assistant).unwrap();
        assert_eq!(copied["tool_calls"][0]["arguments"]["path"], "x");
    }
}
