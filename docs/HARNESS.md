# Dominion 하네스 — 2026-11-18 서브3 활성 구조

> This document replaces the retired, independent `dominion-harness` library/CLI instructions.
> Only `dominion-sub3` was changed. Historical `sdk_run` database records and checkpoint JSON files remain on disk.

## Native autonomous work

The durable Rust autonomous task runner uses:

- `src/api/jobs.rs` / `src/api/sdk_runs.rs`: the product job queue and stable SDK API.
- `src/engine/native_agent.rs`: provider, memory, owner-approved tool/MCP and journal host adapters.
- `crates/dominion-agent-sdk/src/lib.rs`: the model/tool/verification/checkpoint execution kernel.
- `crates/dominion-agent-sdk/src/tool_discovery.rs`: bounded native tool selection and verified search/call bridge.
- `src/engine/skill_discovery.rs`: scoped progressive Skill lookup/reading, not a second agent runtime.
- `sdk/rust` and `sdk/typescript`: protocol clients, not independent model execution loops.

The `/api/sdk/runs` and **new** `/api/harness/runs` requests use the **same** Rust job/kernel path. The legacy route has a `harnessCompat` history marker and a restricted workspace/shell tool contract; it does not authorize unrelated MCP, browser or external account tools.

## Retired standalone harness (deleted)

The independent `crates/harness/` workspace package, `dominion_harness::AgentSdk::run`, and the `dominion-harness` CLI are no longer built or executable on this branch. Their former implementation can be examined in Git history only. The workspace `Cargo.toml` and `Cargo.lock` no longer depend on it.

The HTTP `src/api/sdk.rs` retains endpoint **compatibility** for new native jobs, plus an archive-only view of old `sdk_run` records and bounded historical `sdk-runs/{uuid}.json` checkpoints. It does **not** resume/re-approve/reconcile old standalone-runner sessions or replay uncertain past side effects. Such requests explicitly return a conflict; users can start new native jobs while the original evidence remains archived.

## Still active, not disposable residue

`src/engine/chat/runtime/turn.rs` remains the existing interactive chat/room execution path. Deleting it without moving streaming protocols, active chat sessions, rooms and tool approvals would disable a real product feature. Full chat/native convergence requires measured migration and is **not** claimed here. Independent third-party ACP runtimes are also **not** bundled yet.

## Verification and quality gate

- `cargo test -p dominion-agent-sdk --lib`
- `cargo test --bin dominion-rust api::sdk::tests`
- `cargo test --bin dominion-rust engine::skill_discovery::tests`
- `.github/workflows/sub3-rust-sdk-check.yml` / `sub3-product-acceptance.yml`: scoped macOS Rust test runs and checks that the deleted duplicate workspace is not restored.

A successful test run applies only to its exact checked commit. Tests do not, by themselves, establish that chat/native execution paths are structurally unified.
