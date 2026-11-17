# Sub2: failed-tool evidence, replay boundaries and no-progress retries (2026-11-17)

## Problem found in the real call path

The native route is `jobs::run_prompt -> autonomy::execute -> run_quiet_outcome -> chat::dispatch_tool_call -> dominion_agent_kernel::Cycle::assess`. The model/tool runtime emitted `tool_error` events, but `run_quiet_outcome` previously recorded their names in `tool_errors` **without forwarding any failed receipts into `TurnStats.evidence`**.

A blocked remote effect uses `result.effectUnknown=true`. Losing that observation meant the standalone kernel could not distinguish an unresolved external write from an ordinary failed turn. The kernel also interpreted known research tools such as `web_search` as potential external writes, preventing a safe retry after a search. Finally, the planner treated every fresh tool event, including a repeated identical read, as *new progress*.

## Source-level changes

1. `src/engine/mod.rs` collects both `tool_end` and `tool_error` receipts. Failed results preserve tool identity, arguments, error status and uncertainty in `TurnStats.evidence`; a failed tool cannot be a successful acceptance receipt.
2. `src/engine/chat/runtime/tool_calls.rs` supplies the original arguments on tool errors. Pre-dispatch rejections carry `dispatched=false`, including user denial and soft errors. An earlier unsettled effect retains `effectUnknown=true` and still requires review even though the duplicate invocation was not dispatched.
3. `crates/dominion-agent-kernel/src/lib.rs` accepts both `outcomeUnknown` and `effectUnknown` contracts and differentiates explicit known observational tools from mutating or unknown external tools. GET-only GitHub access is read-only for replay decisions; POST and unspecified MCP tools remain conservative.
4. `src/engine/autonomy.rs` computes successful *novel* tool evidence from stable call inputs and results, ignoring per-call IDs and timestamps. Duplicate reads and failed events no longer override the kernel's stagnant-progress stop rule; genuinely changed read output and modified workspace receipts still count.

The original authorization, approval, token ceiling, persistence schema, React UI, external SDK routes, stored jobs and historical SQLite data are unchanged. No migration or new daemon was introduced. Only `dominion-sub2` was modified.

## Regression cases added in source

- Uncertain failed remote effect reaches the no-replay kernel, resulting in `Review`.
- Denied/pre-dispatch remote call is not incorrectly treated as a completed external write.
- Search, fetch, memory, Google read and GitHub GET can precede a safe repair; GitHub POST and unknown MCP continue to gate replay.
- Same `git status` output with a new call ID and latency is *not* new progress; changed output is.
- Failed reads and no-op file writes do not purchase additional retries.

## Evaluation contract

**Source-confirmed:** five connected source changes and focused Rust regression cases were committed to `dominion-sub2`. **Not run in this chat:** Rust unit/integration tests, HEAD CI, real account/browser flows, real-model task-completion benchmarks and CPU/DB/token before/after measurements. No speedup or score increase is asserted without executing the same workloads.

Next measurement should run the kernel tests, `outcome_tests`, and autonomous prompt jobs with: (a) failed Google/GitHub read followed by a repair, (b) an uncertain remote mutation, and (c) repeated identical read output across 3 attempts. Verify final job statuses, tool receipts, cost and absence of replayed effects before judge scoring.

## Frontier comparison consulted

- [Codex Rust app-server lifecycle, approvals and replay](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [Qwen Code session multi-agent 2026-10-05 design](https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-05-session-multi-agent.md)

These are design references, not evidence that DOMINION matches their real-world completion rates.
