//! Typed, per-round state. Durable execution evidence remains in the core journal;
 //! these values are intentionally transient and never serialized as ad-hoc DB JSON.
use super::*;

pub(in crate::engine::chat::runtime) struct ModelRound {
    pub(super) round_tools: Vec<Value>,
    pub(super) wire_messages: Vec<Value>,
    pub(super) model_started: std::time::Instant,
    pub(super) model_offset_ms: u64,
    pub(super) completion: Option<crate::engine::chat::Completion>,
}
impl Default for ModelRound {
    fn default() -> Self {
        Self { round_tools: Vec::new(), wire_messages: Vec::new(),
            model_started: std::time::Instant::now(), model_offset_ms: 0, completion: None }
    }
}

pub(in crate::engine::chat::runtime) struct ToolRound {
    pub(super) round_calls: Vec<ToolCall>,
    pub(super) parallel_reads: bool,
    pub(super) dispatched: std::collections::HashMap<usize, Result<(Value, bool, String), String>>,
    pub(super) ordering_used: bool,
    pub(super) ordering_ok: bool,
    pub(super) prepared_arguments: Value,
    pub(super) prepared_key: String,
    pub(super) completed_operations: Vec<Value>,
}
impl Default for ToolRound {
    fn default() -> Self {
        Self { round_calls: Vec::new(), parallel_reads: false,
            dispatched: std::collections::HashMap::new(),
            ordering_used: false, ordering_ok: true,
            prepared_arguments: Value::Null, prepared_key: String::new(),
            completed_operations: Vec::new() }
    }
}
