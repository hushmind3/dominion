# Sub3: reproducible coding-task and cost evidence (2026-11-17)

## Executed, not merely specified

Focused CI on sub3 HEAD 3da2b4b52ee4fbb31dccaf09ab77762ea13cc641:
https://github.com/hushmind3/dominion-rust/actions/runs/44013575350

| Fixture | Real execution | Verified | Calls | Scripted tokens | Tool receipts | Failing/passing tests | Replayed effects |
| --- | --- | --- | ---: | ---: | ---: | --- | ---: |
| Two Rust files, injected crash, restore, failing rustc, repair | Filesystem and rustc | yes | 5 | 800 | 5 | 1/1 | 0 |
| One file, failing shell check, repair, passing rerun | Filesystem and shell | yes | 4 | 480 | 4 | 1/1 | 0 |
| **Combined** | **2 scenarios** | **2/2** | **9** | **1,280** | **9** | **2/2** | **0** |

The model is a deterministic script in both fixtures. These results prove a limited Rust SDK harness property: real file/shell effects, verified results, failure feedback, and crash-safe continuation. They DO NOT establish real-model coding completion, paid provider performance, or competitive model rankings. Token figures are scripted ModelReply usage values; actual billed KRW remains unknown, not zero. Wall time is separately recorded per run, without claiming universal throughput.

## Reproduction

With Rust, Node.js and a POSIX shell:

~~~bash
node --test scripts/coding-scorecard.test.mjs
cargo test -p dominion-agent-sdk --test multifile_fault_recovery -- --nocapture | tee /tmp/dominion-two-file.log
cargo test -p dominion-agent-sdk --test real_fs_repair actual_shell_failure_triggers_real_file_repair_then_green_test -- --nocapture | tee /tmp/dominion-one-file.log
node scripts/coding-scorecard.mjs /tmp/dominion-two-file.log /tmp/dominion-one-file.log
~~~

CI executes these with pipefail, and the parser refuses duplicated cases, incomplete evidence, negative or missing counters, and mixed provider cohorts.

## Fair real-model comparison

Use identical frozen coding tasks, source revision, model settings, attempts, tool permissions, resource limits, and isolated workspaces on every branch. Count independently verified task completions, effect replays, failed tests and repairs, checkpoint recovery, model calls, token/cached-token usage, CPU/RSS, SQLite bytes and actual wall time. Keep user-configured unit prices and KRW cost estimates separate from provider invoices. A task that only prints a success message without matching independently verified artifacts is not completed.

The new indexed execution timeline and cancellation race fixes are separate from the scripted scorecard. Their own tests and the final branch HEAD must be checked independently.

## Additional dependency and forensic integrity hardening (2026-11-17)

Three bounded source fixes on `dominion-sub3` (no change to `main`, UI layout, account credentials or persisted snapshot schema):

1. `crates/dominion-agent-sdk/src/scheduler.rs` now reserves every prior operation ID yet only allows **successful** prior receipts to satisfy new action dependencies. An unsuccessful/skipped prior receipt is rejected before a tool batch can be admitted; the model must propose a new successful prerequisite. The execution fence still independently rechecks results.
2. `crates/dominion-agent-sdk/src/replay.rs` flags mismatched names/arguments of ordinary planned actions and purported successful actions whose dependencies lack successful receipts. Resolved `dominion_tool_call` bridge operations retain their explicit exception to raw argument equality.
3. `src/engine/native_replay.rs` treats a host operation without a terminal `completed` or `failed` status as inconsistent even if an incorrectly persisted record says `outcomeUnknown=false`. Read-only paired quality comparisons require a consistent host journal.

New deterministic test cases were **added**, not executed in this editing environment. The commits use `[skip ci]` in line with the owner's cleanup-first direction. No real model performance, reduced token usage or runtime speed-up is asserted for these fixes.

Focused follow-up on a Rust-enabled runner:

~~~bash
cargo test -p dominion-agent-sdk scheduler::tests:: -- --nocapture
cargo test -p dominion-agent-sdk replay::tests:: -- --nocapture
cargo test native_replay::tests:: -- --nocapture
~~~

Also rerun the full SDK/kernel suite and a real persisted native job with a known failed prerequisite, a rejected uncertain host operation, and a clean success path before declaring the new HEAD release-ready. These are provenance and safety checks, not current-world acceptance proofs.

## Kernel verification namespace and replay attribution hardening (2026-11-17, follow-up)

Source-only change on `dominion-sub3`, based on the judged 69/100 baseline. No changes to `main`, UI, credential paths, or serialized Snapshot fields.

- `crates/dominion-agent-sdk/src/acceptance.rs` declares the single host-only `kernel-verify-` namespace.
- `src/lib.rs` creates mandatory test operation IDs from that constant. `scheduler.rs` rejects any model-proposed action using the reserved prefix before tool admission.
- `replay.rs` now distinguishes model-planned tool receipts from host-generated acceptance receipts. A receipt with no model action and no valid host-check shape marks the trace inconsistent. Valid host checks appear as `kernelVerificationReceipts`. The report remains read-only: identifying a receipt is **not** proof of its external side effects; `native_replay::report` independently checks the host operation journal.
- New deterministic regression cases exercise model ID spoofing; legitimate host check receipts; unattributed receipts; tool-name substitution; mismatched step; and a forged skipped check.
- Old checkpoints with no decision tape still deserialize; they remain ineligible for confident decision-to-effect replay. No schema migration was introduced.

**Verification state:** Source changes committed with `[skip ci]`. GitHub source was read back after writing. Rust compilation and tests have **not** run in this editor, and neither real-model success rate nor runtime cost can be inferred. A Rust-enabled runner should execute:

~~~bash
cargo test -p dominion-agent-sdk scheduler::tests:: -- --nocapture
cargo test -p dominion-agent-sdk replay::tests:: -- --nocapture
cargo test -p dominion-agent-sdk --test multifile_fault_recovery -- --nocapture
cargo test native_replay::tests:: -- --nocapture
~~~

The improvement targets replay evidence integrity, not a new measured benchmark point. For comparison, Qwen Code's October 2026 session multi-agent design describes explicit persisted records and replay, while Codex app-server separates lifecycle events and approvals. Neither external system's published design establishes a Dominion pass-rate claim.


## 2026-11-18 failure-evidence correction

`crates/dominion-agent-sdk/src/failure_guidance.rs` now distinguishes **source mutated after a failed check** from **the same check succeeding**. An edit leaves the original receipt visible as `source_changed_recheck_pending` and points the model to the kernel-owned acceptance pass; an exact successful repeat of the same tool and arguments clears the hint, while unrelated tool successes do not. This preserves error evidence without blindly repeating an unchanged failing check.

Commit: `21616fea` on `dominion-sub3` only. Added deterministic test cases, not executed here (Rust unavailable). Scoped verification: `cargo test -p dominion-agent-sdk failure_guidance::tests::`, followed by relevant SDK acceptance tests. The fix is a source-level lifecycle correction, not evidence of improved real-model completion.


## 2026-11-18: evidence-driven Program IR repair and adaptation (branch-only)

This is **not** another SDK optimizer or candidate-armed success counter. It corrects existing production call paths:

- `src/engine/program.rs`: validate every nested program node once with total-tree/depth bounds; apply real nested pending-node replacements; reject unknown/no-op revisions, in-flight effects, completed ancestors/loop-scoped operations, unauthorized nested tools and forward/parallel observation dependencies. `evaluate_verification_checks` reuses the existing three-valued predicate kernel.
- `src/engine/chat/runtime/program.rs`: a failed `verify` returns a real repair failure instead of being marked completed; an Unknown pauses without executing later nodes. Nested branch/sequence/loop failures propagate; concurrent parallel child failures settle without falsely completing their parent. A mutation increments a durable epoch and invalidates prior verification. Concurrent mutation plus verification is treated as stale until rechecked after the join. Revision admission is no longer credited as repair success; terminal, epoch-fresh predicate evidence is required for positive repair credit.
- `src/engine/adaptation_loop.rs`: score intervention discrimination only where conflicting live hypotheses predict different values on an *observed* field named by an executable expected check. Read-only diagnosis is preferred when causes are unresolved; a model-supplied historical-effect float without observed citations earns no bonus. Candidate results are independently classified as verified, failed or unrated using the selected action's host-observed before/after state, not only tool return status.
- `src/engine/chat/runtime/adaptation.rs`: actually capture the selected tool's filesystem before/after metadata through the existing host observation function; store the classification in the existing execution/adaptation checkpoint, and send it to the algorithm evidence path.
- `.github/workflows/sub3-product-acceptance.yml`: add development-profile Rust regression filters for these modules; no release/CI run was triggered by these `[skip ci]` source commits.

**Regression cases added:** missing/failed/passing predicates; nested branch replacement; completed ancestor and loop-scoped effect fencing; unknown revision targets; in-flight execution fencing; unauthorized nested tools; parallel forward dependency; nested budget evasion; parallel freshness; no positive credit for a bare success echo; and diagnostic prediction agreement/disagreement.

Targeted Rust commands for a Rust-enabled **macOS** developer/CI environment:

```bash
cargo test --bin dominion-rust engine::program::tests
cargo test --bin dominion-rust engine::adaptation_loop::selection_tests
cargo test --bin dominion-rust engine::chat::runtime::program::evidence_join_tests
```

**Verification provenance:** Source written and read through the connected GitHub repository on `dominion-sub3`. This editor has no `cargo`/`rustc`, so the above Rust tests have **not** been executed here. No real-model improvement, cost reduction, run completion, or pass-rate increase is claimed. The independent grader should check the exact branch HEAD and run the listed filters. The SDK's native job loop and chat's Program IR are still separate orchestration paths; this patch improves the actual chat path without claiming full engine unification.

**2026 implementation references inspected:** Google `google-research/harness2` (`harness2/core/recursion.py`, `selection.py`, and workflow tests), `google-research/veriharness` (isolated evidence-backed verification); Airbnb `agent-harness-optimizer` (`optimizers/prism/loop.py`, `types.py`); DeepSeek `deepseek-harness` (`tool-executor-ptc/src/ptc.ts`, `ptc-calls.ts`). These are architecture comparisons; no claim of their full recursive optimizer, isolated multi-rollout harness or sandbox being replicated.


## 2026-11-18: Dominion-specific evidence-lineage adjudication

**Research audited (2026):** arXiv 2611.05650 on executed counterfactuals and replay certificates; arXiv 2610.32285 on native harness trace and failure attribution; Google VeriHarness code; Harness² execution loops; Airbnb PRISM implementation. These are evaluation references, not directly transplanted source or scoring formulas.

**Actual production path:** native_agent.rs pins original acceptance artifacts before the first model/tool operation, captures per-action before/after workspace-write file hashes in its existing durable operation journal, and seals the final owner and kernel-derived acceptance artifacts. native_replay.rs reconstructs the precise host lineage. native_adjudication.rs aligns decision dependencies by observed topological positions (rather than ephemeral tool IDs), rejects candidate comparisons with divergent initial accepted artifact states or common-prefix host tool results, and traces only explicit descendants of a candidate's divergent decision to actual recorded mutation pre/postimages and final kernel-checked file bytes.

**Public read-only API:** POST /api/sdk/runs/:id/adjudicate with candidateRunIds (1..8) plus /api/jobs/:id/adjudicate. It reads historical jobs only: no model calls, tool replays, promotion, new project state or unapproved experiments. A successful candidate is reported as an observed outcome gain, with a possible artifact-linked mechanism; neither is called a causal effect without controlled executed intervention on a complete restored world.

**Regression tests added but not run here:** actual filesystem witness and unsafe path; mismatched kernel/file verdict; initial witness mismatch; dependency graph topology; pre-intervention host result drift; connected job/host-operation replay and candidate outcome comparison. Cargo/rustc are absent from the current editing container. Run focused tests:

    cargo test --bin dominion-rust engine::native_adjudication::tests
    cargo test --bin dominion-rust engine::native_replay::tests
    cargo test --bin dominion-rust api::native_replay::tests

**Missing:** true isolated causal fork with identical full-world state and replay agreement measurement; quantified pass-rate or token gains. The algorithm refuses causalWinner instead of fabricating certainty. This is evidence-specific code integrated with Dominion's own SDK and host journal, not a generic PRISM/Harness² clone.


## 2026-11-18: complementary research combination, not a new optimizer loop

- **Google Harness²**: reuse execution contrast and its distinction between a model suggestion and an observed task result; no new rollout calls are created.
- **Airbnb PRISM**: reuse 2026's multi-objective candidate-evaluation pattern instead of a scalar invented success score. The local portfolio compares observed evidence grade, failed receipts, recorded tokens, elapsed time and owner-configured KRW cost.
- **Google VeriHarness**: use kernel acceptance, hashed artifact lineage and evidence provenance to qualify candidate observations before any tradeoff selection.
- **DeepSeek Harness**: respect the existing pinned Rust SDK tool-authority and budget/execution contracts. The historical adjudicator never bypasses those boundaries or dispatches external effects.

`src/engine/native_adjudication.rs` now returns `evidenceQualifiedPortfolio` and `portfolioMetrics` through the already wired `POST /api/sdk/runs/:id/adjudicate` endpoint. The frontier excludes unverified runs, missing price/latency/token measurements and dominated tradeoffs, preserving incomparable cost-vs-speed alternatives for owner review. An unmatched baseline/candidate pair cannot generate a recommendation. No holdout reliability, repeated-trial success probability, causal claim or automatic promotion is inferred from one task.

Regression cases added: evidence-first admission, conflicting cost/latency dimensions, missing price, missing independently corroborated lineage, absent matched candidate, and existing persisted native job evidence. Scoped development Rust tests remain to be run on a Rust-enabled macOS runner; this editing environment has no `cargo`/`rustc`. Do not count this patch as measured superiority before compilation, test pass and same-task comparisons.
