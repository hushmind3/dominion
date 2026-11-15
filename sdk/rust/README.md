# Dominion Native Rust SDK

**The Rust SDK is the primary agent harness.** The canonical `dominion_sdk::harness` exposes the actual native model-independent kernel, with bounded model/tool loops, parallel read-only actions, owner steering, evidence verification and durable pre-effect journals. The HTTP client in lib.rs is optional.

Use it from another Rust application with this path dependency:

    dominion-sdk = { path = "sdk/rust" }

Implement the provider-independent `dominion_sdk::harness::{Model, Tools, Evidence, Journal}` traits in the host, construct `Kernel { model, tools, evidence, journal }`, and call `kernel.run(RunRequest { ... }).await`. The Rust kernel owns the plan/act/verify loop and tool-dispatch scheduling; the host provides credentials, actual tools, durable checkpoints and approval gates. Every checkpoint must persist before external effects.

The DOMINION production adapter, src/engine/native_agent.rs, is an actual Driver implementation: model planning through the native router, execution through the native Rust agent tool/approval/runtime, deterministic acceptance through the native verifier, and checkpoints through the SQLite-backed job queue. GUI tasks and TypeScript SDK both target that path via /api/jobs. The separately isolated RSI generation/independent acceptance path remains in place.

The client methods in sdk/rust/src/lib.rs optionally connect to an existing Rust engine at 127.0.0.1:4317 for jobs, sessions, approvals, rooms, video, RSI, chat SSE and replayable events. They are secondary to the native harness.

No vendor SDK owns the loop. No goals.rs and no /api/goals API. The Rust kernel includes deterministic scheduling tests for read concurrency, dependency barriers and authorization, plus scripted integration tests of failure-driven replanning and write stopping. Run cargo test --manifest-path sdk/rust/Cargo.toml and cargo test --manifest-path Cargo.toml in a Rust development environment. Live model and server tests remain outstanding. See docs/SDK-ARCHITECTURE-2026-11-15.md.

Native entrypoint: `use dominion_sdk::harness::{Kernel, RunRequest, Model, Tools, Evidence, Journal};`. This is a real execution kernel rather than a wrapper around an agent provider. `Dominion` HTTP methods are secondary, for remote/local product integration.


## Rust-owned dependency scheduler (2026-11-15)

The `dominion-agent-sdk` crate now implements a dependency-aware dispatcher in `crates/dominion-agent-sdk/src/scheduler.rs`. Tool calls must reference prior durable receipts or earlier actions in model order. Independent read operations may execute in bounded parallel groups; every mutating operation creates a barrier. Invalid dependency graphs are rejected before effects, and any failed batch is durably recorded and sent to a new model planning step rather than blindly continuing the old plan. The production MCP adapter treats structured errors and non-success HTTP statuses as failures instead of successful tool receipts.

The canonical Rust entrypoint remains `Kernel::run`, re-exported as `dominion_sdk::harness`. The TypeScript package is only a typed HTTP/SSE client. This set of changes is source-implemented and accompanied by scripted tests; Rust SDK and native host compilation passed targeted tests in GitHub run 43684016242; live-provider validation has not yet run. Full CI and SWE-bench should follow product integration, stale-asset cleanup and user-authorized budget checks.


## Durable, safe continuation

`Kernel::resume(request, checkpoint)` continues an interrupted job from the exact previously committed `Snapshot`. Identity and owner-task text must match. The kernel rejects any checkpoint with a pending model request or outstanding tool effect, preserves prior receipts and token counters, and starts at the next unfinished planning step. A terminal snapshot can be returned without another model request. No prior command is reissued automatically.

The live adapter `src/engine/native_agent.rs` now reads the SQLite-backed `nativeState` checkpoint and calls `Kernel::resume` only when `nativeInFlight` is clear. Restored job runs reuse the original session, execution identifier and chat message identifiers; the existing job recovery worker leaves uncertain tool/model outcomes in `needs_review` rather than requeueing. Scripted tests cover successful continuation, failed dependency recovery, missing identity, and unknown-outcome rejection. These tests executed and passed in the focused GitHub Rust SDK run 43684016242.


### Native coding verification

The public job API accepts a `command_succeeded` criterion and transforms it into a Rust-owned exact shell execution receipt check. The verifier only treats a passing command *after the latest workspace edit* as satisfying the owner's acceptance condition. The same requirement flows from the main-based GUI through `/api/jobs`; no TypeScript execution loop. Workflow child jobs also use `Kernel::run` with stable IDs and resumable checkpoints. Test commands still require actual owner approval.


### Automatic acceptance/repair loop

Native `Kernel::run` and `Kernel::resume` own verification-command scheduling as well as model and tool turns. Set `RunRequest.checks` to a `ToolSucceeded` criterion with `tool: Some("shell_exec")` and `contains: Some("cargo test")`; the kernel executes the exact missing test under the host's normal permission and journal controls when the model proposes final completion. A failed check is fed into repair, not blindly repeated, and later side effects invalidate earlier passes. For HTTP callers, use `acceptanceChecks: [{ "type": "command_succeeded", "command": "cargo test" }]`. This is executable Rust source with authored tests. Rust SDK and host builds passed in the focused GitHub run 43684016242; live-provider operation remains unverified.


## Executed verification — 2026-11-15

**[GitHub Actions run 43684016242](https://github.com/hushmind3/dominion-rust/actions/runs/43684016242)** on commit `58c37744763506268a369674e3f828ae56b2a699` **concluded SUCCESS**:

- `cargo test --manifest-path crates/dominion-agent-sdk/Cargo.toml -- --nocapture`: **35 passed, 0 failed** (12 unit + 6 kernel integration + 15 replanning + 2 real-file/shell tests)
- The two real-file tests run actual filesystem writes, `sh -c` commands, a real nonzero exit code, repair, re-verification and disk checkpoint restore. A scripted model supplies deterministic decisions; this is **not** a claim that a live frontier model solved a fresh task.
- `cargo test --manifest-path Cargo.toml --bin dominion-rust native_ -- --nocapture`: **22 passed, 0 failed** (filtered Rust host tests; 724 others not run)
- `cargo test --manifest-path Cargo.toml --bin dominion-rust recover_new_workflow_reuses_durable_rust_sdk_child -- --nocapture`: **1 passed, 0 failed** (745 other host tests not run)
- `npm ci --prefix web`, `npm run typecheck --prefix web`, `npm run build --prefix web`: **all succeeded**. Vite reported a successful production build.

**Total executed Rust cases in this narrow gate: 58 passed / 0 failed.** Full product CI, live-model coding success, live-browser UI, paid provider budget, and SWE-bench remain independently unverified. The regular full CI workflow was not invoked by this gate; verification is a dedicated `sub3-rust-sdk-check.yml` workflow and is scoped to core/host/UI validation. This narrows one proven execution capability, not a declaration that all of Dominion or its coding intelligence is frontier-equivalent.



### Context preservation across long agent runs

The SDK's Rust-native, read-only `dominion_receipt_search` and `dominion_receipt_read` tools expose bounded lookup of this run's durable receipts to the planner without reissuing external operations. Recent tool output, past failures and operation IDs are projected with explicit size budgets. Their full authoritative JSON remains in `Snapshot.receipts` across `Kernel::resume`. The native `/api/jobs` and GUI execution path inherit this automatically. See `docs/SUB3-DURABLE-CONTEXT-2026-11-15.md`.
