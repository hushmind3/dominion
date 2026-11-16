# DOMINION Rust Native Agent SDK

The independent **`dominion-agent-sdk`** crate owns budgeted model/tool execution, plan/decision snapshots, tool authorization contracts, bounded JSON Schema validation, durable receipts, independent acceptance checks and fail-closed crash/recovery behavior. It is not a built-in media renderer or an autonomous self-improving RSI engine.

For large allowed MCP registries, `dominion_tool_search` and fixed `dominion_tool_call` bridge offer progressive discovery without changing provider function schemas. The host validates final arguments, tool allowlists and pinned MCP manifests before effectful execution. Owner intervention never becomes a fabricated success receipt.

`replay::trace(&snapshot)` reconstructs recorded decisions and effects **offline**; it neither calls a model nor repeats tools or automatically promotes code. The product host `src/engine/native_replay.rs` cross-checks independent operation journals, reported cache usage and configured KRW tariffs.

This SDK does not implement generation-level campaigns, autonomous core mutation, video/audio production, or unattended optimizer policies. User-requested coding/refactoring/testing tasks run through the normal Rust harness and acceptance criteria.

Run `cargo test -p dominion-agent-sdk`. See [lean product architecture](../../../docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md).
