# Sub2 — contrastive, receipt-grounded ProgramIR repair (2026-11-18)

Branch: `dominion-sub2`. This describes source changes, not a verified benchmark gain.

## One connected execution path

```text
Existing program executor (run_program / execute_nodes)
  → real tool failure (execution_operation + exact callId)
  → failed node remains pending (completed effects unchanged)
  → indexed run-specific native HTIR provenance slice (≤64 receipts)
  → ONE repair model response: ≤3 independent candidate ProgramIR revisions
  → apply_revision_proposal validates each candidate against the checkpoint
  → reject unrelated node rewrites, nonexistent evidence, unsafe probes
  → 1–3 deduplicated observational preflights through dispatch_tool_call
  → compare machine-observed True / False / Unknown for each candidate
  → compose non-conflicting failed-node edits + evidence/verification additions
  → Pareto filter (verified evidence / effectful nodes / change scope)
  → choose and persist revised ProgramIR + probe call IDs / candidate audit
  → execute revised program through existing effect gateway
  → read-back postflight(s) via native gateway (exact predicate evaluation)
  → completion ONLY on observed True; False / Unknown → needs_review
```

This is not a fixed tactic list or a synthetic "strategy trial." Tool results determine candidate admission and completion. Candidate assessments are attached to `revisedProgram.repairTournament` and the real tool observations to `execution_operation`.

## Important correctness repair

Previously `execute_program_tool` called `mark_program_completed` even when a tool returned a failed result with `onFailure=revise`. The next revision was rejected by `apply_revision_proposal` because the failed node had been incorrectly committed as completed.

Now such a node remains pending; the failure's exact tool `callId` is durably recorded and becomes the anchor for the evidence graph. A later successful revised execution clears that failed-node marker, without clearing prior external effect receipts. Confirmed completed nodes still cannot be replaced.

## What is combined from 2026 public implementations

- **Harness² (Google Research, October 2026)**: propose multiple concrete alternatives, actually run small controlled probes, compose compatible gains. In Dominion, probes are read-only on the current real task, not multiple speculative full-workspace mutation rollouts. Source: https://github.com/google-research/harness2/blob/main/harness2/core/recursion.py
- **DeepSeek Harness (2026)**: nested tool work goes through a single admitted native gateway, bounded budgets and durable dispatch logs. Dominion does not copy its TypeScript driver; it reuses `dispatch_tool_call`, operation IDs, approval, cancellation and effect sealing. Source: https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/tool-executor-ptc/src/ptc.ts
- **PRISM / Airbnb AHO (2026)**: keep non-dominated candidates instead of choosing the model's largest self-reported score. Dominion uses a bounded Pareto frontier from actual preflight verdicts and the structural cost of candidate changes. No claim of full PRISM holdout training. Source: https://github.com/airbnb/agent-harness-optimizer/blob/main/agent_harness_optimizer/optimizers/prism/loop.py
- **HarnessFix (June 2026)**: use exact native tool receipts, explicit version/input/resource/procedure links, and provenance-scoped repair, not a generic summary of the last error. Causal citations must connect to the failure; temporal proximity is insufficient. Source: https://github.com/HarnessFix/HarnessFix/blob/main/failure_analysis/htir.py

## Changed code

| Responsibility | Source |
| --- | --- |
| Contrastive candidate validation, composition, deduplicated observation sharing, Pareto admission, postflight predicate | `src/engine/adaptation/repair_tournament.rs` |
| Native failure dependency graph and causal evidence checks | `src/engine/adaptation/trace_ir.rs` |
| Task-specific hypothesis probes, retained from the earlier work | `src/engine/adaptation/probes.rs` |
| Actual preflight and postflight tool execution, failed node completion repair, checkpoint and revision history | `src/engine/chat/runtime/program.rs` |
| Bounded indexed access to real run receipts | `src/db.rs::recent_execution_operations` |
| Module registration | `src/engine/adaptation/mod.rs` |

## Properties and constraints

- At most **one** model request to generate up to **three** ProgramIR alternatives per program repair attempt; existing max repair attempts and shared model-round ledger remain in force.
- An explicitly selected **append-only** existing repair policy cannot be bypassed by the contrastive replacement path. Legacy append proposals remain available.
- A replaced failed call_tool retains `onFailure=revise` if the model omitted it, so another real failure follows the existing bounded repair policy instead of being silently marked completed.
- At most **three distinct preflight tool calls** per tournament; identical inputs are executed once and the *same real observation* is evaluated against each candidate's own prediction.
- A candidate needs a preflight with a machine-readable predicate and a non-side-effecting native tool. False or Unknown cannot qualify. Candidate replacements are scoped to the actual failed node; a non-failed candidate may only add observation or verification nodes for compatible composition.
- The selected repair also carries machine-checkable read-only **postflights**. A bare "exists" result is insufficient: observed value or ordered comparison must be True before the program may report completion.
- When postflight fails / is denied / is inconclusive / runs out of budget, the program records `needs_review`, the actual call IDs and verdicts, **not** a fabricated completed repair. It does not silently replay completed external writes.
- `turn_body` treats `needs_review` as a settled program execution, **not** a completed task; it does not rerun the same program every model round. `determine_task_outcome` refuses `Succeeded` when the persisted program checkpoint requires review, even if the model emitted a final answer.
- Legacy single-proposal `apply_revision_proposal` remains supported, preserving previously stored programs and clients.
- No new long-running worker, private DB, SDK surface or UI; uses the existing shared engine, checkpoint and operation history.
- Model-provided evidence IDs are checked against checkpoint observations or an actual non-temporal failure-provenance path. Invalid refs and attempts to modify completed operations are rejected.

## Targeted source-level regression tests added

- A failed `onFailure=revise` operation remains pending and can actually be replaced; a successful completed effect cannot.
- Two nonconflicting proposals may produce a validated composed revision, while incompatible changes to the same node cannot be combined.
- Observations that contradict a candidate exclude it; missing/denied results never count as passing.
- Identical read-only tool calls are performed once and shared across competing hypotheses.
- Unrelated program-node rewrites, invented causal evidence, unsafe shell probes and weak existence-only postflights are rejected.
- After actual mutation, a passing postflight requires an observed value match; a failed or Unknown result causes a non-completion.

## Native controlled A/B trial, not a new benchmark framework

The pre-existing isolated AB/BA runner in `src/bin/dominion-benchmark.rs`
now supports `BENCH_REPAIR_AB=1`. Both arms receive the **same frozen
ProgramIR** and fixture, forcing one real read of a missing target file.
The only benchmark-only difference is the repair planner contract:

- baseline: one legacy ProgramIR replacement proposal;
- treatment: up to 3 evidence-grounded candidates with native preflights and
  required post-execution readback.

This selector is available **only** inside `DOMINION_BENCHMARK=1` disposable
engines. Production always keeps the contrastive runtime. Both arms use the
same model, provider, requested task, original program, tools, controller
budget and file oracle; AB/BA trial order is persisted and alternated.
A trial whose execution trace never reached ProgramIR repair is marked
invalid, not credited as a recovery win, even if the chat model happened to
write the expected output.

Run in an isolated checkout with a local OpenAI-compatible model server and
debug/release binaries already built:

```sh
BENCH_REPAIR_AB=1 BENCH_MODE=real BENCH_PAIRS=12 \
 BENCH_FIXTURE=update_config BENCH_MODEL=qwen3:4b \
 BENCH_PROVIDER_BASE=http://127.0.0.1:11434/v1 \
 DOMINION_BIN=./target/release/dominion-rust \
 ./target/release/dominion-benchmark
```

The runner creates a disposable temporary workspace and engine. Results
include individual native execution and operation receipts, the selected
repair arm, evidence of actual revision, preflight/postflight call IDs,
AB/BA assignments, deterministic file oracle results and observed tokens.
For every trial the runner queries the existing native
`GET /api/sessions/:id/usage` ledger. Unlike the execution's streamed-only
token field, this includes non-streaming repair-planner calls. The summary
reports total observed tokens and tokens per **independently verified
completion** only when *every* assigned trial has actual session usage
coverage. Missing usage produces null efficiency, not an invented zero.
The source metadata records that repair-planner temperature is 0.2 and normal
chat sampling may be provider-defined (not falsely frozen to 0).
Monetary cost remains unavailable without real provider invoices; local
compute/electricity cannot be silently converted into API spend.

The paired effect and observed tokens are experimental outputs, not a claim
of a benefit until real model trials have actually been executed.

This A/B tests the *incremental value of Dominion's contrastive repair* versus
its earlier single-repair path. It is **NOT** a head-to-head comparison against
stock Codex/Claude Code. A stock comparison requires separately recorded
stock-harness runs on the exact same task/model/tool/budget constraints and
independent output verification. The existing `scripts/sub2-compare.mjs`
can compare genuine verified exports under matching metadata, but no
stock-harness winner is presumed.

## Verification status

Commit source and unit-test bodies exist on `dominion-sub2`. This session does not have `rustc` or `cargo`. GitHub Actions development-test jobs were queued or previously cancelled during rapid changes, with no confirmed passing test result observed for the final revision. Do not describe this as compiled, benchmark-winning, frontier-equivalent, or production-verified yet. CI completion is not part of this owner's current request.

Measured complete-task success rate, postflight recovery rate, token cost, CPU/DB utilization and time comparisons under controlled identical tasks are **not measured yet**. Actual counterfactual mutations in separate isolated worktrees and offline held-out harness optimization are also **not implemented**; only read-only candidate preflights execute before selecting a live revision.
