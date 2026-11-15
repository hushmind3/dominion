# Native streaming execution / partial-call settlement (sub3 · 2026-11-15)

## Production entrypoints

The owner-facing native auto-work GUI and API use the **same independent Rust agent kernel**, not a second scripted demo:

```text
web/src/components/NativeAgentPanel.tsx
  → POST /api/jobs → jobs::run_prompt
  → native_agent::run → dominion_agent_sdk::Kernel::run/resume
  → ConfiguredModel::complete_with_tools
  → native_model::complete_with_run (OpenAI-compatible / Anthropic / Codex)
  → native_model::receive_stream + native_stream::{SseDecoder,Assembler}
  → ModelTurn { typed Decision.actions, verified terminal assistant content }
  → scheduler::arrange → NativeTools::execute → approval/permission/receipts
  → durable snapshot, independent acceptance checks, result
```

## Exact provider streaming contracts

- **OpenAI-compatible Chat Completions:** `stream:true`; tool calls assemble by **index**, preserving ID, name and arbitrary chunk-split JSON arguments, followed by `finish_reason=tool_calls` and terminal `[DONE]`. A missing terminal marker, invalid argument JSON, `length` finish, mismatched tool outcome or incomplete call cannot be executed.
- **Anthropic Messages:** `stream:true`; `content_block_start`, `content_block_delta` with `input_json_delta.partial_json`, `content_block_stop`, `message_delta` and `message_stop` are separate states. Partial tool arguments are assembled into valid objects only after block settlement. Thinking/redacted-thinking blocks do not leak as public final text or become tools. Any unclosed content/tool block is rejected.
- **Codex Responses:** `stream:true`; `response.output_text.delta` streams visible text, `response.output_item.added` marks a pending tool, and **only** `response.completed` with a completed response authorizes parsing the full typed tool call. `response.failed`, incomplete responses and abrupt EOF remain failures. The old whole-response byte accumulation was removed.

`src/engine/native_stream.rs` is a bounded, byte-level SSE decoder. It handles arbitrary HTTP split points, UTF-8 scalars spanning chunks, LF/CRLF, multiline SSE data and provider status events. The decoder caps aggregate input at 32 MiB, per-event payload at 2 MiB, raw line at 256 KiB, partial tool arguments at 1 MiB, and tool indexes/blocks at 80. No partial tool argument ever directly reaches the host executor.

## Observable job progress and cancellation

While the native model produces its turn:

- `native_model::StreamProgress` emits `job_event` events (`model_delta`, `model_tool_pending`) and persists a bounded `job.nativeStream` with `stage`, `protocol`, `events`, `visibleTail`, and **pending tool names/IDs only**. Raw/partial tool arguments are excluded.
- The main-based `NativeAgentPanel` shows generated text and distinguishes **pending, unexecuted** tool calls from real tool receipts. HTTP jobs still poll the same record and receive bus events.
- The Rust kernel already write-ahead persists `Snapshot.model_in_flight` before starting generation. Unexpected EOF, incomplete SSE and failed responses are not silently replayed after a restart; they remain `needs_review`.
- A native stream polls the job's cancellation flag every **250 ms even when the provider is stalled**. Explicit owner cancellation is a terminal `cancelled` state; a narrow cancellation-only CAS writes its terminal `nativeState` without circumventing the normal rejection of post-cancel effects. No model-pending tool is ever dispatched.

Compatible self-hosted servers that ignore streaming can still return a **complete JSON** response; the adapter parses it normally, rather than pretending it emitted SSE. CLI/text-only models continue using the existing Rust-owned JSON planner without any vendor SDK.

## 2026 design sources directly compared

- OpenAI Responses streaming events: https://developers.openai.com/api/reference/resources/responses/streaming-events
- OpenAI Chat Completions streaming chunks/tool_calls: https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events
- Anthropic Messages streaming/content block deltas: https://platform.claude.com/docs/en/build-with-claude/streaming
- Codex source: `codex-rs/core/src/tools/router.rs` and `tools/orchestrator.rs` (https://github.com/openai/codex)
- DeepSeek Harness durable assistant stream attempt: `packages/core/agent-loop/src/assistant-stream.ts` (https://github.com/deepseek-ai/deepseek-harness)
- Dominion's existing ChatGPT native WebSocket handler for comparison: `src/engine/chat/codex_ws.rs`; this improvement targets the autonomous job worker's native streaming HTTP path.

## Executable regression evidence / limits

Source tests exercise split-byte SSE, Korean/UTF-8 payloads, interleaved indexed tool calls, Anthropics' partial input JSON and thinking, missing completion, Codex incomplete/error events, paired model/host tool receipts, real loopback HTTP SSE (5–7 byte chunks), durable progress persistence and owner cancellation during a deliberately stalled provider. The narrow branch verification runs Rust SDK tests, filtered native-host tests, workflow checkpoint recovery and main-based GUI typecheck/build; full CI/SWE-bench is deliberately separate.

No claim is made that scripted provider fixtures prove a frontier-model coding success rate. Production third-party provider live credentials, Codex socket-chain reuse inside autonomous jobs, broader benchmark scoring and cross-provider cache cost accounting remain separate follow-on work.
