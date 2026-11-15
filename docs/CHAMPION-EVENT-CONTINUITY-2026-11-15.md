# Champion: durable event continuity across Rust, TypeScript and UI (2026-11-15)

## Problem and source comparison

The Champion Rust event path previously read only 500 SQLite rows, then subscribed to live updates. A disconnected client with more than 500 retained records could silently miss the rest. Parallel emitters could allocate event sequence N before another emitter persisted/broadcast N+1, causing out-of-order observations.

The implementation compared the actual same-repository `dominion-sub4` event/HTTP source (2026-11-15) with Champion's original `src/events.rs` and `src/api/system.rs`. The previous Champion SDK already offered a cursor-based, read-only event watcher, so the integration extends it instead of introducing a second SDK/harness.

As an external architecture reference, DeepSeek Harness's 2026 `agent-loop` and event-backed Session distinguish durable history from a live driver: https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/README.md. Dominion keeps its own SQLite, Rust engine, owner controls and TS transport.

## Runtime route (original entrypoints preserved)

- `src/events.rs::Bus::emit`: one shared mutex orders sequence allocation, SQLite persistence and live publication across cloned emitters.
- `src/events.rs::Bus::replay_available`: pagination in batches of 500, capped at 4096 per catch-up; SQLite presently retains around 2000–2256 latest rows. This is bounded retention, not infinite audit history.
- `src/api/system.rs::events` (`GET /api/events`): subscribes **before** loading retained history, replays all available pages, drops duplicates by increasing sequence, recovers on receiver lag, and emits `event: replay_gap` with `requiresStateRefresh:true` if the requested history was pruned.
- `sdk/typescript/src/client.ts::watchEvents`: handles the gap control message without requiring a nonexistent sequence ID or mutating the last observed durable cursor. Invalid gap metadata is rejected. Only the read-only GET reconnects; a non-idempotent model/tool POST is never retried.
- `web/src/App.tsx`: the existing EventSource listens for `replay_gap`, triggers the existing `dominion-refresh` path and refreshes authoritative approval and running-job counters. No synthetic job/agent success is created.

## Regression coverage

- `src/events.rs`: 900+ retained events across multiple SQL pages; 8 concurrent emitters × 30 calls with observed broadcast and durable-log order; retention-pruning discontinuity.
- `sdk/typescript/tests/sdk.test.mjs`: correctly receives `replay_gap` without a sequence then a real event; malformed gap metadata is rejected; existing cursor/reconnect tests remain intact.
- `web/src/App.tsx`: uses the existing page refresh event rather than adding an independent state store.

Recommended validation in a checked-out Rust/Node environment:

```sh
cargo test --release events::
npm test --prefix sdk/typescript
node --test scripts/source-hygiene.test.mjs
node scripts/source-hygiene.mjs
npm run build --prefix web
```

**Evidence boundary:** GitHub source commits and test definitions are not the same as observed test results. This chat environment cannot clone the GitHub repository over the container network and has not executed local Cargo/Node tests, live model calls or CI. A storage device failure between SQLite append and process recovery is not simulated; durability relies on the existing SQLite configuration. The work does not change the autonomous Rust decision loop, RSI evaluation, paid video providers or model weights.
