# Sub2: 18:17 measured benchmark, artifact/outcome reconciliation

**Date:** 2026-11-18 (Asia/Seoul)  
**Branch under examination:** `dominion-sub2` at `555f73b0`  
**Evidence limit:** The figures below were provided in the user's report; the referenced ZIP was on the user's local `~/Downloads` and was not attached to this development environment. No claim of inspecting its underlying `jobs.jsonl`, `summary.json` or raw provider events is made.

## New user-reported measurements

| Candidate | Coding verified outputs | Coding tokens | 9-person room, 3-turn total tokens | Room status |
|---|---:|---:|---:|---|
| main fb46709d | 3/3 | 53,604 | 81,979 | 30 messages |
| sub2 555f73b0 | 3/3 | 71,286 | 81,311 | 30 messages |
| sub3 6490bcd4 | 3/3 | 57,807 | 75,306 | 30 messages |
| sub4 2343821d | 2/3 | not reported | 27,014 | HTTP 400 in turn 3 |
| money ada3b133 | 3/3 | 72,724 | 81,757 | 30 messages |

Sub2's previous room total was 75,151. This run was 81,311, +6,160 (~8.2%). Sub3 moved from 80,972 to 75,306. This two-run cross-branch reversal should **not** be treated as proof that either room selector is intrinsically better without a larger paired repeated trial set.

The sub2 coding token cost exceeds sub3's by 13,479 (~23.3%) even though both produced 3/3 correct files. This is an actionable *investigation metric*, not causal evidence of a specific verifier or retry flaw; per-task model calls, completion tokens, provider usage and executed tools must be compared first.

**Critical distinction:** Sub2's t2 artifact passed the user's file oracle, but the job ended `needs_review`. File correctness, independently verified owner contract, and job lifecycle status are three different observations. The local source alone does not establish which specific acceptance check or tool uncertainty led to the reported `needs_review`.

## Inspected Rust path

`src/engine/jobs.rs::run_prompt` → `src/engine/autonomy.rs::execute` → `engine::run_quiet_outcome` → `verification::check_acceptance` → `agent_kernel::Cycle::assess` → `run_prompt` checkpoint → `run_job` finalization.

The autonomous worker was checkpointing `autonomousState.status=running` immediately before returning `completed` / `needs_review`, leaving conflicting lifecycle state in the persisted job. The outer `job.status` eventually became terminal, but the nested state never received a terminal checkpoint.

Changed **only on sub2**:
- terminal autonomy checkpoint records `completed` or `needs_review`, corresponding phase, exact attempt, original frozen verification criteria, unresolved gaps, last model error, and final per-attempt receipts *before* handing the result to `run_prompt`;
- separates `replayUnsafeEffects` from `unknownToolOutcomes`: a non-idempotent but known successful remote write is not the same as a tool with an unknown final effect;
- leaves immutable job `input`, existing claim/epoch guards, and conservative uncertainty disposition untouched;
- cancelled or superseded jobs cannot persist a later successful terminal checkpoint.

The existing `run_prompt` result still contains the authoritative `status`, `unmetChecks`, `lastError` and `receipts` for the same task. This change improves triage and internal status coherence; it does **not** turn a weakly evidenced t2 success into a fake `completed`.

## Next paired benchmark inputs required for causal diagnosis

For each sub2 coding task, compare **job `status`, `result.status`, `result.unmetChecks`, `autonomousState.unmetChecks`, `result.receipts[].error`, `failureClass`, `execution.taskOutcome`**, and each iteration's provider `modelCalls`, `promptTokens`, `completionTokens`, `cachedPromptTokens` and tool evidence. On t2, inspect which acceptance check was frozen before edits, whether native SHA-256 write receipts matched the final file, whether a post-edit test was requested/run, and whether any non-replayable or genuinely uncertain effects were observed.

Sub2's room token deltas should be evaluated with at least a few same-model runs and exact message cardinality, with actual cached and uncached token fields separated from aggregate input tokens.

No user-visible real-model improvement is claimed until those next measurements. Full release CI is outside the scope of this targeted development change.
