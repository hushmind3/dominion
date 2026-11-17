# Sub2 · Provider-wire tool trace budget (2026-11-17)

## Reason and reference comparison
- Sub2/main/sub3 `src/engine/chat/context.rs` selects past messages by `content` length, leaving `assistant.tool_calls[].function.arguments` out of that history estimate.
- In Sub2 `src/engine/chat/runtime/turn_body.rs`, the next model request previously cloned all in-memory `messages`, including potentially multi-megabyte write arguments and tool results.
- OpenHands SDK persists conversation events for resume, separating durable state from model-visible context (source: https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py).
- Qwen Code uses prompt compaction controls while preserving tool-call ordering and approval barriers (sources: https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-09-27-hosted-workspace-tool-turn.md ; https://github.com/QwenLM/qwen-code/blob/main/docs/design/chat-compression-cache-sharing.md).

## Implementation
- `src/engine/chat/trace_budget.rs`: classify only complete, uniquely paired assistant + tool-result frames; retain normal short groups byte-for-byte.
- For oversized groups, replace only provider-wire replay with a bounded, clearly labeled inert assistant note, naming real tools, IDs, paths and excerpted observed results. The original message remains in SQLite and the in-memory working transcript.
- Prioritize newest groups within a deterministic byte allocation. Always preserve owner/system messages and ordinary prose, including user-requested file contents.
- An incomplete or duplicated tool frame is never rewritten as successful evidence; existing error/recovery paths retain ownership.
- `src/engine/chat/runtime/turn_body.rs` applies this once before the existing provider token estimate and paid call, for local and hosted models alike.

## Verification / limits
- Focused Rust tests assert byte-for-byte short-frame preservation, huge file-write argument removal, newest evidence priority, no corruption of incomplete frames and storage immutability.
- All limits are bytes, not tokenizer-accurate token counts; no claim of benchmarked speed or cost reduction.
- Live provider latency, real completion rate, total tokens, and platform builds are outside this patch's verified scope.
