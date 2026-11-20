# Sub3 — portable core / product host / storage boundary
Date: 2026-11-20. Branch: `dominion-sub3`.

## Core: model decisions and authorized execution

`crates/dominion-agent-sdk/` owns the portable agent runtime, bounded iterations, decision planning, dispatch order, result interpretation and recovery checkpoints. It requires only `serde`, `serde_json` and `futures-util` at runtime; it **does not compile SQLite, passwords, web API, MCP transports or a product AppState**. The traits `Model`, `Tools` and `Journal` define how the embedding application supplies these functions.

```text
User natural language -> host request adapter -> independent SDK kernel
                                                   |
                                   Model / Tools / Journal (interfaces)
                                                   |
                           Host execution adapters + integrations
                                                   |
             Storage (SQLite, migrations, vault) and optional features
```

## Ownership and code layout

| Layer | Implementation | Responsibility |
| --- | --- | --- |
| Core | `crates/dominion-agent-sdk/` | Receive request, decide, call authorized tool, interpret output, bounded continue |
| Host | `src/host/execution/` + `src/host/state.rs` | Product-level auth, durable SQLite-backed journal, approvals, job recovery, effect records |
| Storage | `src/storage/sqlite.rs`, `src/storage/migrations.rs`, `src/storage/vault.rs` | Durable records, schema upgrades and encrypted credentials |
| Conversation | `src/conversation/chat/` | UI-facing streaming and room/context projection |
| Features | `src/features/` | Memory, learning, algorithms, collaboration, provider management, product services |
| Integrations | `src/integrations/` | MCP, messaging, browser, account OAuth, local shell/file tools, web and Skills |
| UI/API | `web/` and `src/api/` | Preserve existing routes and user workflows |

The entire former `src/engine/` directory (10 implementation modules plus `mod.rs`) was moved to `src/host/execution/`; no file in `src/engine/` remains. SQLite, migrations, vault and shared AppState were moved from root `src/` into storage/host. External feature modules were separated in earlier commits and the chat service has already been moved to `src/conversation/`.

The product's existing Rust calls to `crate::engine::...`, `crate::db::...`, `crate::state::...`, `crate::vault::...` and `crate::migrations::...` are temporarily forwarded by **compatibility adapters in `src/compat/engine.rs` and root aliases in `src/main.rs`**, not by moving implementations back into the core. Network REST contracts, JSON records, database filenames, migration order and UI menus are unchanged.

## Isolated legacy imports

The feature catalog, conversation facade and local tool-driver reexports were removed from `src/host/execution/mod.rs` and relocated to `src/compat/engine.rs`. They are **source-only compatibility aliases** for existing `crate::engine::...` imports; they do not belong to the portable kernel or the host execution module. New code imports `host`, `features`, `storage`, `conversation` and `integrations` directly.

## Isolation level

This is an actual source ownership separation, with **an independently compilable Rust agent-kernel crate** and host-owned persistent storage. It does **not** assert the product host can run without database services or that host features are independently hot-unloadable plugins. The host `AppState` and job adapters still use `Store`; this is intentionally outside the SDK and must be further narrowed by host-specific persistence traits before multiple backends are supported.

The kernel's `Journal` trait already receives snapshots/receipts. The concrete durable checkpoint implementation is now extracted from the agent runner into `src/host/journal.rs` and uses the host `JobContext`/SQLite store; the kernel imports no storage code. Mutating operations retain existing approval, cancellation epoch, pinned MCP contracts and effect receipts, so this change does not remove execution safeguards.

## Regression verification

`scripts/core-boundary.test.mjs` requires an engine-free source tree, storage under `src/storage/`, product execution adapters under `src/host/`, and no SQLite or `AppState` references in SDK sources. `scripts/integration-boundary.test.mjs` tracks the external tool boundary. The sub3 product and Rust SDK workflows are updated to use actual test module paths and watch the new host/storage directories.

A passing source-boundary test does not replace Rust compilation or real-world model/provider integration verification.
