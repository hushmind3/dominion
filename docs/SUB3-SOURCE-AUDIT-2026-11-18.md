# DOMINION sub3 — source-tree and documentation audit (2026-11-18)

**Scope:** `hushmind3/dominion-rust`, branch `dominion-sub3`. **No changes to `main`.** This is a static repository/API/source-path review, not a claim that all 196 Rust files were line-reviewed or runtime-tested. The GitHub recursive tree returned **479 file/directory entries**, untruncated: **196 `.rs`**, **47 frontend `.ts/.tsx` under `web/src`**, **62 `.md`**, and five `.yml`. Core code at the actual entry/dispatch/storage/UI/API boundaries was read directly; dependencies and workspace/build manifests were compared. Prior measured artifacts were dated separately.

## Runtime entry and shared state

| Boundary | Grounded entry/source | Audit conclusion |
| --- | --- | --- |
| Server process | `src/main.rs` → `src/bootstrap.rs` → `src/api/mod.rs` | Loopback HTTP server; recover jobs and start shared worker/maintenance. Loads and persists SQLite before accepting routes. |
| Native agent jobs | `src/api/jobs.rs` / `src/api/sdk_runs.rs` → `src/engine/jobs.rs::run_prompt` → `src/engine/native_agent.rs::run` | Model, tools and journal are injected into `dominion_sdk::harness::Kernel` from `crates/dominion-agent-sdk`. Approval/verification/cancellation use host-provided real operations. |
| Historical harness API | `src/api/sdk.rs` → `src/api/sdk_runs.rs` | New `/api/harness/runs` invokes the same Rust worker with `harnessCompat` capability constraints. Old `sdk_run` records are read-only and cannot be resumed/reconciled as new effects. `crates/harness` no longer exists. |
| Interactive chat and rooms | `src/api/chat.rs` → `src/engine/chat/runtime/turn.rs` plus `src/engine/chat/room_context.rs` | Separate streaming turn lifecycle and context pipeline. **Do not claim a single top-level kernel for every chat and job.** |
| Durable state | `src/db.rs`, `src/engine/jobs.rs`, `src/engine/native_replay.rs` | Central SQLite store and job/effect receipts; scope and recovery depend on checked status, tool contracts, and persisted state. Existing user records must not be destructively migrated. |
| Memory, context, cache | `src/engine/chat/context.rs`, `continuity.rs`, `codex_ws.rs`, `crates/dominion-agent-sdk/src/context.rs` | Context projection and Codex WS delta transport exist; full provider cache reuse and actual billing savings need fresh usage measurements. |
| Skills/tools | `src/engine/skill_discovery.rs`, `src/engine/chat/runtime/capability.rs`, `crates/dominion-agent-sdk/src/tool_discovery.rs` | Authorized tool catalog plus progressive discovery. A model-discovered name is not a permission grant. Chat task-scoped Skill selection was just unified with native logic in earlier commits; test status is pending. |
| Optional Money | `src/engine/money*.rs`, `src/db/money*.rs`, `src/api/money*.rs`, `web/src/pages/Money.tsx` | Domain data lives in the same SQLite/database job interface; collects source evidence and hypotheses, with explicit owner review/implementation. This is not proof of money earned. |
| Product UI | `web/src/App.tsx`, `web/src/components/WorkspaceShell.tsx`, `web/src/components/NativeAgentPanel.tsx` | React/Vite UI has chat, rooms, Money, providers, memory and work functions; these UI screens need API-level end-to-end verification after updates. |
| SDK clients | `sdk/rust`, `sdk/typescript`, `packages/sdk-typescript` | One Rust kernel, but multiple client facades. Legacy `/api/harness/runs` client is compatibility surface, not an independent runtime. |
| Release/CI | `.github/workflows/ci.yml`, `release.yml`, `sub3-*.yml` | Development checks are scoped Rust tests; full release gate runs manually for macOS/Windows, with Linux used for release publishing only. |

## Verified documentation inconsistencies resolved

1. **Root English README was not an onboarding page:** over 21 KB of dense historical patch details with no quick-start. Replaced with working commands, product/architecture overview, API/SDK references, hard limits and a current-state evidence boundary. Detailed change history remains in `docs/`.
2. **Korean README promoted an expensive release-profile test as the ordinary Rust check:** replaced with focused development-profile commands, a runnable local startup path and a distinct release-stage section.
3. **`sdk/rust/README.md` claimed the separate RSI runner and video/RSI client endpoints were still active.** They are not active in the verified `src/api/mod.rs` route surface or `sdk/rust/src/lib.rs`. Replaced with the real `dominion_sdk::harness` re-export and `Dominion` HTTP client.
4. **`packages/sdk-typescript/README.md` named the deleted `dominion_harness::sdk::AgentSdk` and implied a separate runner.** Replaced with current `/api/harness/runs` compatibility semantics and its restrictive authority.
5. **Removed-feature description overstated GUI cleanup.** `web/src/pages/Memory.tsx` still imports and renders `GrowthDashboard` from `web/src/pages/memory/Growth.tsx`. The new READMEs distinguish preserved manual growth/learning/record viewing from retired *unattended RSI generations*.
6. **Cache success was easy to over-read.** Historical `benchmark/ROOM-100-TURN-LUNA-2026-11-09.json` reports 2,630,782 prompt tokens, 477,568 cached tokens, 18.15% hit rate, and 621 history-budget rebases. That benchmark is **not current sub3 HEAD**. Documentation now states that provider-reported cache usage, host projected reuse, and WebSocket wire deltas are different metrics.

## Remaining code risks and ownership (not silently fixed by documentation)

- **Two top-level execution lifecycles.** `src/engine/chat/runtime/` and `crates/dominion-agent-sdk::Kernel` share tools/projections in places but do not yet have a single state machine. Replacing one without proving parity risks lost room turn behavior, SSE, cancellation and approvals. Priority: contract and effect-level parity fixtures before migration.
- **Large state-bearing files.** Notably `src/db.rs`, `src/engine/chat/provider.rs`, `src/engine/chat/runtime/turn.rs` and `src/engine/improve.rs`. A large file is not inherently a defect, but this makes changes to token/context, storage and runtime error propagation cross-cutting. Priority: inspect dependency ownership and use isolated regression checks rather than broad refactors.
- **Parallel legacy/client surfaces.** `sdk/typescript` is a main typed HTTP client; `packages/sdk-typescript` continues the old compatibility route. Avoid accidental mismatch of status/reconciliation envelopes. Do not remove the latter until users are migrated.
- **Context reuse measurement gap.** New `continuity::project` host-side reuse estimates cannot be substituted for provider cache reads or billed-token deltas. Compare identical task/model/settings, context preserved, completion, cache usage, wall time and actual total cost.
- **Retained manual learning code.** `src/engine/learning.rs`, `distill.rs` and `improve.rs` still contain explicitly requested memory/procedure and historical evidence operations. `Memory.tsx` still surfaces growth metrics. Treating all of those as deletable RSI residue would break live features.
- **Background work is not zero.** `src/bootstrap.rs` starts the shared job worker, maintenance backups, configured channel connectors and a delayed periodic provider-catalog refresh. Avoid implying that removing RSI/media also disabled all idle work; measure CPU/DB/network separately.
- **Unverified recent source edits.** The recently added chat Skill relevance test and continuity token estimate test have been committed with `[skip ci]`; no local Cargo toolchain was available in this editing environment. These commits **must not inherit** the green status of earlier, unrelated GitHub test runs.

## Focused verification after source cleanup

```sh
# Current Rust kernel and the recently changed chat/skill paths
cargo test -p dominion-agent-sdk --lib
cargo test --bin dominion-rust engine::chat::context::tests
cargo test --bin dominion-rust engine::chat::continuity::tests
cargo test --bin dominion-rust engine::skill_discovery::tests
cargo test --bin dominion-rust api::sdk::tests
# Documentation/retired-source shape; Node 24 required by the frontend
node scripts/check-lean-product.mjs
```

Full frontend, platform, packaging and release-profile tests remain in their **release-stage** workflows. No CI was triggered by this audit or README refresh. Static inspection and existence of authored tests are not actual test execution. The independent evaluator should verify the exact branch commit hash, then compare *same-task* quality, token usage, cache read/write, recall and real effect recovery.

## 2026 public design comparisons (not benchmark claims)

- [Claude Code: Prompt caching is everything (2026-04)](https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything) — stable prompt prefix and deferred tool definitions.
- [Claude Platform: Reducing cost and improving performance (2026-09)](https://claude.com/blog/reducing-cost-and-improving-performance-with-claude-platform) — deferred tool loading and prompt reuse tradeoffs.
- [GitHub Copilot/VS Code: token efficiency (2026-06)](https://code.visualstudio.com/blogs/2026/06/17/improving-token-efficiency-in-github-copilot) — core tool set plus on-demand tools.
- [OpenHands SDK architecture](https://github.com/OpenHands/docs/blob/main/sdk/arch/overview.mdx) — explicit agent/tool/server boundaries and persisted state.

These external systems provide design comparison points, not measured performance gains attributable to DOMINION.

**Change boundary:** documentation edits only in this audit pass. No UI paths, runtime behavior, database schemas, approval rules, OAuth settings or `main` source were changed.
