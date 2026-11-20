# Sub3 — portable core / product host / storage boundary
Date: 2026-11-20. Branch: `dominion-sub3`.

## Core: model decisions and authorized execution

`crates/dominion-agent-runtime/` owns portable execution budgets, shared action/receipt contracts, dependency-aware read/effect scheduling, and schema admission. It has **no dependency on the Algorithms or SDK crates**. `crates/dominion-agent-algorithms/` owns replaceable planning prompts, task-intent classification, tool discovery/ranking, model-visible context projection and failure diagnoses; it depends only on Runtime and `serde_json`. `crates/dominion-agent-sdk/` composes both while retaining the Model → Tool → Result loop, durable Journal contract, replay controls and acceptance verification. None of these three crates depends on the product's SQLite or `AppState`.

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
| Runtime | `crates/dominion-agent-runtime/` | Budget admission, action/receipt contracts, scheduling, JSON Schema guards |
| Intelligence | `crates/dominion-agent-algorithms/` | Swap planning prompts, intent, tool discovery, context views and failure guidance |
| SDK facade | `crates/dominion-agent-sdk/` | Compose model/tool/journal adapters, maintain the bounded turn loop and evidence checks |
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

## Replaceable algorithms over invariant execution

The SDK exposes `Kernel::run_with_algorithm(request, &policy)` and `Kernel::resume_with_algorithm(request, snapshot, &policy)`. `AgentAlgorithm` controls planner instructions, stricter plan validation, tool-shortlist selection, receipt-context projection, failure diagnostics and task-intent signals. Existing `run()` / `resume()` call `StandardAlgorithm` and preserve public wire contracts.

Every returned model-visible tool description must match a **real owner-approved tool name, JSON Schema, description and effect/read-only classification** and fit the bounded visible-catalog limit. The SDK refuses fabricated or changed declarations before issuing a model call. Additional intent policies can demand more evidence, but cannot weaken the standard code-edit/effect evidence requirements. Full authority remains at the host's approval and pinned tool contracts.

See [per-run algorithm injection details](ALGORITHM-RUNTIME-2026-11-20.md).

## Isolation level

This is an actual source ownership separation, with **an independently compilable Rust agent-kernel crate** and host-owned persistent storage. It does **not** assert the product host can run without database services or that host features are independently hot-unloadable plugins. The host `AppState` and job adapters still use `Store`; this is intentionally outside the SDK and must be further narrowed by host-specific persistence traits before multiple backends are supported.

The kernel's `Journal` trait already receives snapshots/receipts. The concrete durable checkpoint implementation is now extracted from the agent runner into `src/host/journal.rs` and uses the host `JobContext`/SQLite store; the kernel imports no storage code. Mutating operations retain existing approval, cancellation epoch, pinned MCP contracts and effect receipts, so this change does not remove execution safeguards.

## Regression verification

`scripts/algorithm-boundary.test.mjs` additionally enforces the one-way Runtime → Algorithms → SDK dependency and relocated algorithms; Rust regression `algorithm_policy_injection` tests default compatibility, stricter policy selection and rejection of forged tool declarations. `scripts/core-boundary.test.mjs` requires an engine-free source tree, storage under `src/storage/`, product execution adapters under `src/host/`, and no SQLite or `AppState` references in SDK sources. `scripts/integration-boundary.test.mjs` tracks the external tool boundary. The sub3 product and Rust SDK workflows are updated to use actual test module paths and watch the new host/storage directories.

A passing source-boundary test does not replace Rust compilation or real-world model/provider integration verification.
