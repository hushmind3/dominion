//! Stable service compatibility surface for the independent context policy.
//! Model-wire compaction is pure and never mutates durable tool receipts.
use serde_json::Value;

pub(super) fn bounded_tool_trace(messages:&[Value],budget_bytes:usize)->Vec<Value>{
    dominion_context_policy::bounded_tool_trace(messages,budget_bytes)
}
pub(super) fn compact_historical(messages:&[Value],max_receipts:usize)->Vec<Value>{
    dominion_context_policy::compact_historical(messages,max_receipts)
}
