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
- `web/src/components/ExecutionMonitor.tsx` shows the actual `nativeState` and allows owner steering; the TS SDK is a client only.
- The wrapper checks command `exitCode` and structured errors before writing a successful tool receipt.
- The previous goal-specific runner was not recreated, and no Goals API is present.

Validation status: new Rust unit tests exist in the imported kernel and production adapter; compilation and live provider tasks are still outstanding on the available machine. Root Cargo lock and local Rust SDK dependency graph were updated. Do not infer frontier parity from committed source alone.
