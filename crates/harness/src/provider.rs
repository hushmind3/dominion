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

/// A synchronous completion backend.
///
/// Async providers adapt by blocking on their runtime in an executor thread;
/// keeping this trait synchronous makes the loop deterministic and testable.
pub trait Provider {
    fn complete(&mut self, messages: &[Message]) -> Result<ModelReply, String>;
}
