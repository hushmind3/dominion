# DOMINION Rust Native Agent SDK

The independent **`dominion-agent-sdk`** crate owns budgeted model/tool execution, plan/decision snapshots, tool authorization contracts, bounded JSON Schema validation, durable receipts, independent acceptance checks and fail-closed crash/recovery behavior. It is not a built-in media renderer or an autonomous self-improving RSI engine.

## Recovery authority and bounded history

The native host durably pins a SHA-256 binding over the authorized tool names, JSON schemas, read-only classification, acceptance checks and evidence requirement before model or tool execution. A changed binding on an existing run results in `needs_review` instead of adopting a broader tool contract. Legacy checkpoints remain loadable and receive a binding at their next safe continuation. The Rust kernel rejects duplicate names and attempts to shadow its reserved history/discovery tools before making a model call.

`dominion_receipt_search` pages immutable history, while `dominion_receipt_read` supports `field: "result" | "arguments"`, `startChar` and `maxChars`. Oversized arguments are shown as a bounded preview by default and can be retrieved incrementally without repeating the original tool. This limits model-visible text without deleting durable evidence.

For large allowed MCP registries, `dominion_tool_search` and fixed `dominion_tool_call` bridge offer progressive discovery without changing provider function schemas. The host validates final arguments, tool allowlists and pinned MCP manifests before effectful execution. Owner intervention never becomes a fabricated success receipt.

`replay::trace(&snapshot)` reconstructs recorded decisions and effects **offline**; it neither calls a model nor repeats tools or automatically promotes code. The product host `src/engine/native_replay.rs` cross-checks independent operation journals, reported cache usage and configured KRW tariffs.

This SDK does not implement generation-level campaigns, autonomous core mutation, video/audio production, or unattended optimizer policies. User-requested coding/refactoring/testing tasks run through the normal Rust harness and acceptance criteria.

Run `cargo test -p dominion-agent-sdk`. See [lean product architecture](../../../docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md).
