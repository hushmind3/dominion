# DOMINION — active Rust architecture (2026-11-20)

This file documents the **current main-branch execution path**. Historical
design notes and branch experiments are not the deployed product contract.

## 1. One kernel, one effect boundary

The **single mandatory agent harness loop** is
[`dominion_algorithms::engine::Engine::drive`](crates/dominion-algorithms/src/engine.rs).
It implements [`dominion_core::runtime_port::AgentLoop`](crates/dominion-harness/src/runtime_port.rs),
where the core owns stable state, budgets, cancellations and journal contracts
— not a competing execution loop. Optional strategies are selected by
[`TurnStrategyPort`](crates/dominion-contracts/src/strategy.rs), implemented in
[`src/addons/strategies`](src/addons/strategies/turn_policy.rs).
The [Rust chat adapter](src/engine/chat/runtime.rs) and
[product host](src/addons/integrations/chat_product.rs) connect incoming
turns and model/tool callbacks to that **one** driver.

A task consists of bounded model iterations, admitted tool proposals,
durable approvals, committed effect receipts and independent verification.
Unresolved mutating side effects cannot be silently retried. The
[journal](crates/dominion-contracts/src/journal.rs) and
[tool gateway](src/engine/chat/runtime/tool_dispatch.rs) preserve evidence
for cancellation, reconciliation and recovery.

## 2. Provider routing and incremental context

[Provider adapters](src/engine/chat/provider.rs) handle local and remote
model APIs. The [conversation continuity module](src/engine/chat/continuity.rs)
maintains per-participant history frames and routes provider cache hints.
[Room context](src/addons/integrations/chat_room_context.rs) budgets selected evidence
separately from the immutable identity prefix.
[Room state](src/addons/integrations/room_state.rs) retains owner-confirmed facts and
stable shared-state digests. [Group orchestration](src/addons/integrations/rooms.rs)
owns speaker selection, cancellation and group delivery.

A stable cache key is a **request** to the provider, not proof of a cache
hit. Provider-reported cache reads, uncached input and model output are
recorded separately when available. User-visible history remains in SQLite. The independent
[room participant cursor](src/db/room_cursor.rs) advances only after a
committed speaker reply; the cursor's exact selected-message IDs do not
claim that every earlier room message was sent to the model.
The [verified work board](src/addons/integrations/room_board.rs) shares
completed tool receipts rather than raw peer tool payloads.

## 3. Progressive tools and MCP

[Capability retrieval](src/engine/capability.rs) ranks real registered
tools and exposes bounded search results rather than emitting every schema.
Every subsequently resolved tool call passes the actual schema, current
session permission, effect journal, approval and cancellation checks.
MCP tools do not bypass the Rust execution kernel.

## 4. Persistent state and SDK

The [API](src/api/mod.rs) exposes session, room, tool, model and SDK routes.
The [SQLite store](src/db.rs) owns records, messages, effect receipts and
indexed execution lookup. The [TypeScript SDK](packages/sdk-typescript/src/index.ts)
is an HTTP client for the same native kernel; it is not a parallel executor.
The [React frontend](web/src/App.tsx) uses the same Rust API in browser and
the [Tauri 2 desktop shell](src-tauri/tauri.conf.json).

## 5. Quality and release gates

- `node scripts/check-runtime-architecture.mjs`
- `node scripts/check-release-contract.mjs`
- `cargo test --locked --workspace`
- `npm run typecheck --prefix web` and `npm run build --prefix web`
- `.github/workflows/ci.yml` for incremental regression tests
- `.github/workflows/release.yml` for Windows/macOS tests, native installers,
  checksums and GitHub release publication after all platforms pass

See [README](README.md), [한국어 README](README.ko.md) and
[Harness contract](docs/HARNESS.md).

## 6. Versioned run/event correlation (2026-11-20)

The native [`RunEventRef`](crates/dominion-contracts/src/run_event.rs)
records `schemaVersion`, `executionId` and, when known, `sessionId`,
`taskId`, `roomId`, `agentId`, `sourceMessageId` and lifecycle.
The [event journal](src/events.rs) attaches this optional `runRef` to
execution, message, approval and SDK records when an authoritative execution
ID is present. Existing event `type`, `kind`, `seq` and record payloads
remain unchanged; unrelated room IDs and job IDs are **never** misidentified
as execution IDs. The global `seq` remains the reconnect/replay cursor.
