# DOMINION — active Rust architecture (2026-11-19)

This file documents the **current main-branch execution path**. Historical
design notes and branch experiments are not the deployed product contract.

## 1. One kernel, one effect boundary

The authoritative execution kernel is
[`dominion-core::Engine`](crates/dominion-core/src/engine.rs).
The [Rust runtime adapter](src/engine/chat/runtime.rs) and
[provider orchestrator](src/engine/chat/product.rs) connect incoming turns,
model calls and tool effects to the same kernel.

A task consists of bounded model iterations, admitted tool proposals,
durable approvals, committed effect receipts and independent verification.
Unresolved mutating side effects cannot be silently retried. The
[journal](crates/dominion-core/src/journal.rs) and
[tool gateway](src/engine/chat/runtime/tool_dispatch.rs) preserve evidence
for cancellation, reconciliation and recovery.

## 2. Provider routing and incremental context

[Provider adapters](src/engine/chat/provider.rs) handle local and remote
model APIs. The [conversation continuity module](src/engine/chat/continuity.rs)
maintains per-participant history frames and routes provider cache hints.
[Room context](src/engine/chat/room_context.rs) budgets selected evidence
separately from the immutable identity prefix.
[Room state](src/engine/room_state.rs) retains owner-confirmed facts and
stable shared-state digests. [Group orchestration](src/engine/rooms.rs)
owns speaker selection, cancellation and group delivery.

A stable cache key is a **request** to the provider, not proof of a cache
hit. Provider-reported cache reads, uncached input and model output are
recorded separately when available. User-visible history remains in SQLite.

## 3. Progressive tools and MCP

[Capability retrieval](src/engine/capability.rs) ranks real registered
tools and exposes bounded search results rather than emitting every schema.
Every subsequently resolved tool call passes the actual schema, current
session permission, effect journal, approval and cancellation checks.
MCP tools do not bypass the Rust execution kernel.

Public HTTP content is stored as session-scoped, content-addressed evidence
instead of being copied wholesale into each prompt. The `http_fetch` tool
returns an initial readable excerpt and a document ID; `http_document_read`
retrieves later windows or exact phrase matches without a new network fetch.
The original source is kept for audit, not replaced by a generated summary.
Execution records also retain a pre-provider input composition estimate by
system, conversation, tool output, retrieval and tool-schema origin. Provider
usage and cache hits remain separately measured authoritative counters.

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
