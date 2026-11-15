use super::{Message, ToolCall};

/// One model response. A provider adapter converts its wire format into this
/// shape; the harness never sees provider-specific JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelReply {
    /// Prose plus optional tool calls. Empty prose is allowed when the model
    /// only wants to act.
    Assistant { content: String, tool_calls: Vec<ToolCall> },
    /// A terminal answer. The harness ends the run.
    Final { content: String },
}

/// Actual provider-reported usage. Zero means the provider did not report
/// that field; no model-name heuristic is used to invent token counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
}

impl ModelUsage {
    pub fn add(&mut self, other: Self) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self.completion_tokens.saturating_add(other.completion_tokens);
        self.cached_prompt_tokens = self.cached_prompt_tokens.saturating_add(other.cached_prompt_tokens);
    }
}

/// A synchronous completion backend.
///
/// Async providers adapt by blocking on their runtime in an executor thread;
/// keeping this trait synchronous makes the loop deterministic and testable.
pub trait Provider {
    fn complete(&mut self, messages: &[Message]) -> Result<ModelReply, String>;
    /// Consume the last round's provider-reported token usage, if available.
    fn take_usage(&mut self) -> Option<ModelUsage> { None }
}
