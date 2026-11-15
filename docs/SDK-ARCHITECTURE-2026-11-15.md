# DOMINION 2026-11-15 Frontier Comparison — source-backed working audit

## Comparison standard

Reference is the finished frontier-class autonomous agent product, not merely whether a function or API endpoint exists. Compare end-to-end source editing and task completion, memory and session continuation, tool permissions and sandbox, failure recovery, parallel agents, model/provider independence, context/cache/cost, video, group rooms, verified non-weight RSI, SDK/API parity and a non-developer UI. DOMINION's rubric weights: autonomous work 25, engine/RSI 15, turn-key video 15, cost 10, rooms 8, extension 8, providers 6, UX 5, maintenance 5, operations 3.

## Sources inspected directly (2026-11-15)

| System | Actual execution-path source | Relevant mechanism |
| --- | --- | --- |
| [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/agent.ts) | packages/core/agent-loop/src/agent.ts | Session-derived request projection, step/turn state, queued inbox, cancellation |
| [Codex](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/orchestrator.rs) | codex-rs/core/src/tools/orchestrator.rs | Sandbox/approval authority, retry and escalation |
| [OpenClaw](https://github.com/openclaw/openclaw/blob/main/packages/agent-core/src/agent-loop.ts) | packages/agent-core/src/agent-loop.ts | Reusable agent core, tool batch, steering, tool-loop recovery |
| [Hermes](https://github.com/NousResearch/hermes-agent/blob/main/agent/conversation_loop.py) | agent/conversation_loop.py | Model/tool/request phases, prompt caching, context compression, bounded retries |
| [OpenCode](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/prompt.ts) | packages/core/src/session/prompt.ts | Durable session inbox, explicit owner input, skill/plugin preparation |
| [Qwen Code](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/core/client.ts) | packages/core/src/core/client.ts | Compaction, tool exploration, provider request/agent context separation |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/context/pipeline/orchestrator.ts) | packages/core/src/context/pipeline/orchestrator.ts | Context transformations with provenance, protected nodes |
| [Pi](https://github.com/earendil-works/pi/blob/main/packages/agent/src/agent-loop.ts) | packages/agent/src/agent-loop.ts | Minimal vendor-independent agent loop and same-session continuation |
| [Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) | Official external behavior and SDK documentation, not private runtime | Reference for end-user coding autonomy; private implementation is not assumed |

GitHub API repository metadata checked on 2026-11-15: OpenClaw approximately 394k stars, DeepSeek Harness 281k, Hermes 263k, OpenCode 227k, Codex 135k, Pi 122k, Gemini CLI 109k, Qwen Code 30k. Stars are discovery signals and do NOT prove successful task completion.

## Native Rust execution after changes

    React tasks ─────┐
    TypeScript SDK ──┼─ /api/jobs ─ Rust SQLite worker ── native_agent::run
    Optional Rust HTTP client ────────────────────────────┘        │
    Embedded Rust application ─ sdk/rust::harness::{run, native kernel adapter} ─┘
                                                        │
                         Rust provider/tool/MCP/skill/memory runtime
                         frozen verifier, checkpoints, recovery, event bus

- crates/dominion-agent-sdk/src/lib.rs: independent task policy, planning, execution, verification/repair, bounded dynamic replanning, checkpoint and no-duplicate-effect rule.
- src/engine/native_agent.rs: real production native kernel adapter mapping model calls, Rust tool evidence, session continuation, frozen acceptance and SQLite checkpoints to SDK core.
- src/engine/jobs.rs: ordinary prompt jobs dispatch through this Rust core; RSI worktree generator keeps its independently evaluated path.
- src/api/jobs.rs: accepts owner task instructions and bounded policies, rejects bogus verifier criteria.
- web/src/components/ExecutionMonitor.tsx: non-developer task submission, progress phases, verification gaps, cancellation, simple automatic/thorough choice.
- sdk/typescript: optional typed client; shares /api/* and same Rust persistent execution state, never owns the harness.

No goals.rs and no /api/goals path were added. The removed Goals-specific collection alias and recovery flags were cleaned.

## Verified, unverified and frontier gaps

Implemented as source changes: independent reusable Rust SDK state machine, real Rust worker adapter, frozen deterministic acceptance, evidence-based repairs, crash-side-effect checkpoint barrier, a task progress GUI, existing memory/tool/provider connections.

Not verified in this runtime: Rust compilation, actual model-driven multi-file task pass rates, crash recovery under live external side effects, model cost gains or competitive parity. No SWE-bench score.

Important frontier gaps to address: strong OS sandbox for all execution routes, first-class parallel planning DAG and conflict-safe merge, owner steering at task boundaries, comprehensive context and provider cache/cost experiments, fully automated low-cost cinematic video with partial regeneration, end-to-end group conversation and RSI A→B→A verified improvement, and all-screen SDK/UI parity.

## Correct validation order

1. Compile and run cargo test on sdk/rust and root Rust engine with an installed toolchain.
2. Run TypeScript SDK tests, frontend typecheck and actual runtime API tests.
3. Exercise real user tasks through UI and both SDKs, including write/test/fix loops, cancel, restart and uncertain external effects.
4. Remove dead/duplicate paths, generated artifacts and obsolete documentation references. Do not start CI while those remain.
5. Once integrated frontier capabilities work, perform same-model/tool/seed/budget SWE-bench Verified and cost/recovery comparisons.

This working environment lacked cargo/rustc and a configured live model server. Source commits do not constitute compile or task-completion evidence. CI has not been started.

## 2026-11-15 stronger direct native kernel integration

The canonical Rust SDK is now `dominion_sdk::harness`, re-exporting a provider-independent executable kernel from `crates/dominion-agent-sdk/src/lib.rs`. The older high-level Driver state machine and its legacy bridge were deleted, not kept as duplicate runners.

- `src/engine/native_agent.rs` supplies production model, tool, evidence and SQLite journal implementations.
- `src/engine/jobs.rs::run_prompt` delegates **normal** tasks directly to `native_agent::run`; special RSI experiments keep their isolated existing source-edit/holdout implementation.
- Read-only tool operations are bounded and may execute in parallel; side-effecting tools are ordered and require the real Rust approval policy.
- `POST /api/jobs/:id/directive` supplies a user correction; the kernel checks for it before model and tool batches and rejects stale work.
- `web/src/components/ExecutionMonitor.tsx` uses main's original execution-monitor interface. `web/src/components/NativeAgentPanel.tsx` keeps main's autonomous-work presentation while reading the same sub3 Rust `job` journal and `/api/approvals`; it does not start a second agent loop. Mid-task steering remains available via `POST /api/jobs/:id/directive` and the TypeScript SDK.
- The wrapper checks command `exitCode` and structured errors before writing a successful tool receipt.
- The previous goal-specific runner was not recreated, and no Goals API is present.

Validation status: new Rust unit tests exist in the imported kernel and production adapter; compilation and live provider tasks are still outstanding on the available machine. Root Cargo lock and local Rust SDK dependency graph were updated. Do not infer frontier parity from committed source alone.


## Frontend decision (2026-11-15)

The `dominion-sub3` branch adopts **main's frontend**: the main versions of `web/src/pages/Home.tsx` and `web/src/components/ExecutionMonitor.tsx`, plus its `NativeAgentPanel` layout. The panel's transport is adapted to sub3's production `/api/jobs` and `/api/approvals` endpoints rather than copying main's separate `/api/sdk/runs` engine. The optional verification command becomes an explicit native agent execution instruction (subject to tool permissions and actual observed exit code), not a claimed post-run command hook. Failed-run retry and in-doubt reconciliation each create a **new** journaled job; neither blindly replays prior external side effects.

Static integration: main frontend page/panel paths → `/api/jobs` → `src/api/jobs.rs` → `src/engine/jobs.rs::run_prompt` → `src/engine/native_agent.rs` → `dominion-sdk` kernel. Approval path: panel → `/api/approvals` → `src/api/approvals.rs` → native Rust approval waiter. The native Rust host compiled and the main-based frontend TypeScript check and Vite production build passed in GitHub run 43684016242. A live browser/API task remains to be exercised.


## Rust-owned scheduler and clean checkpoint continuation (2026-11-15)

- New independent `crates/dominion-agent-sdk/src/scheduler.rs` uses a deterministic ordered dependency planner. Read-only calls can run concurrently only when independent; mutating operations are serialized. Future dependencies and duplicate IDs are rejected before dispatch. Earlier successful receipts may be dependencies; failed dependencies do not unlock subsequent side effects.
- `Kernel::run` invokes the scheduler, persists write-ahead intent, rechecks user steering and cancellation before every batch, and dynamically replans after tool failures or invalid action plans. Incomplete batches are never reported as successful work.
- `Kernel::resume` matches exact run identity and original user task, rejects unknown in-flight model/tool outcomes and skips all already-committed steps and receipts. The job queue continues only clean interrupted checkpoints, and the product adapter preserves session IDs, execution IDs, and existing messages.
- `src/engine/native_agent.rs` uses a single common structured error validator for MCP and native tools; this prevents false positive receipts from HTTP-style error envelopes.
- New deterministic tests: `scheduler.rs` (5) and `tests/kernel_replanning.rs` (5), in addition to prior kernel tests; these test files are source artifacts, **not a claim of executed test passes**.
- The production UI still uses `main`'s layout. `NativeAgentPanel` is adapted to the sub3 native `/api/jobs` and `/api/approvals` interfaces. TypeScript is a client, not a separate harness.

**Scope and remaining verification:** Run Rust unit/integration tests on an installed Rust toolchain and then an actual local UI + provider sequence. Concurrent agent delegation, video delivery, group conversation runtime, cross-domain RSI improvements, semantic context compression and verified commercial-grade pricing remain whole-product evaluation items. Do not claim they were made frontier-equivalent by this scheduler change. CI stays deferred until project-wide cleanup and integration checks.

## Core execution unification — 2026-11-15

The default general-purpose job path remains `POST /api/jobs` → `src/engine/jobs.rs::run_prompt` → `src/engine/native_agent.rs` → independent `dominion-agent-sdk::Kernel`. No external vendor SDK owns planning, dispatch or verification.

**Real command completion gate:** `POST /api/jobs` accepts `acceptanceChecks: [{ "type": "command_succeeded", "command": "cargo test" }]`. The Rust API normalizes this into a native exact-command tool-receipt criterion. `Kernel::verify` checks the command string, successful host-confirmed execution, and that the receipt occurred **after the latest recorded workspace edit**. A claim of completion alone cannot satisfy it. The `main` autonomous-work panel uses this criterion, including explicit retries, instead of merely appending a verification request to the prompt. This does not bypass shell approval.

**Workflow engine migration:** `src/engine/workflows.rs` now executes each workflow step through the same native Rust SDK job queue. Stable child IDs and `enqueue_once` support retry-safe continuation; `start_now` avoids single-worker deadlocks. A prior child result is reused instead of re-running modifications. Unknown legacy in-flight chat-loop steps remain in `needs_review`. Cancelling the parent session propagates to active native children via the journal.

**Tests authored:** scripted native-kernel tests for stale command verification, mismatched/failed commands and replanning, workflow child checkpoint recovery, legacy recovery and deterministic child ID. Existing tests remain. Rust SDK and native host targeted tests have now run and passed in GitHub run 43684016242; no claims of runtime equivalence with frontier systems. CI remains deliberately deferred until the whole product is cleaned up and ready.


### Mandatory coding verification and repair ownership

The independent `dominion-agent-sdk::Kernel` now runs the owner's required exact command checks itself on the model's proposed final answer. No model-generated claim can substitute for a host tool receipt. Failed tests are returned as bounded observations for source repair, passing receipts become stale after new effects, and no-repeat checks avoid spending tokens or running commands repeatedly without progress. See `docs/RUST-CODING-COMPLETION-2026-11-15.md` for end-to-end API, state machine and scripted regression cases.


## Executed verification — 2026-11-15

**[GitHub Actions run 43684016242](https://github.com/hushmind3/dominion-rust/actions/runs/43684016242)** on commit `58c37744763506268a369674e3f828ae56b2a699` **concluded SUCCESS**:

- `cargo test --manifest-path crates/dominion-agent-sdk/Cargo.toml -- --nocapture`: **35 passed, 0 failed** (12 unit + 6 kernel integration + 15 replanning + 2 real-file/shell tests)
- The two real-file tests run actual filesystem writes, `sh -c` commands, a real nonzero exit code, repair, re-verification and disk checkpoint restore. A scripted model supplies deterministic decisions; this is **not** a claim that a live frontier model solved a fresh task.
- `cargo test --manifest-path Cargo.toml --bin dominion-rust native_ -- --nocapture`: **22 passed, 0 failed** (filtered Rust host tests; 724 others not run)
- `cargo test --manifest-path Cargo.toml --bin dominion-rust recover_new_workflow_reuses_durable_rust_sdk_child -- --nocapture`: **1 passed, 0 failed** (745 other host tests not run)
- `npm ci --prefix web`, `npm run typecheck --prefix web`, `npm run build --prefix web`: **all succeeded**. Vite reported a successful production build.

**Total executed Rust cases in this narrow gate: 58 passed / 0 failed.** Full product CI, live-model coding success, live-browser UI, paid provider budget, and SWE-bench remain independently unverified. The regular full CI workflow was not invoked by this gate; verification is a dedicated `sub3-rust-sdk-check.yml` workflow and is scoped to core/host/UI validation. This narrows one proven execution capability, not a declaration that all of Dominion or its coding intelligence is frontier-equivalent.

