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

## 2026-11-18 task-local competing strategy probes (source-only implementation)

**Goal:** Move beyond selecting a historically successful fixed strategy (such as UCB1) by permitting the current task to present 2–3 explicit competing hypotheses with real bounded observations before applying effects.

**Implementation:** `crates/dominion-agent-sdk/src/probes.rs` models bounded candidate paths, rejects disallowed/effectful probes before dispatch, and compares success/failure evidence from real receipts. `src/lib.rs` incorporates the alternatives into the **existing** Rust scheduler, token/tool budgets, cancellation, owner directives, write-ahead snapshots and step loop. `replay.rs` attributes candidate-path receipts to the model's original decision rather than treating them as unplanned effects. `src/engine/native_model.rs` recognizes a standalone candidate proposal from native models while retaining native function calls for normal actions. The next planning turn sees a compact `candidateProbeComparison` with an optional tentative best path. Ties remain unresolved. Unchanged front-end, persisted user data, host approval policies and SDK request parameters.

**Safety and limitations:** All exploratory actions are host-declared read-only; speculative writes and external effects never run in the comparison. The preference is based on observed probe outcomes, **not** a guaranteed quality judgment or the ability to run several full candidate workspaces. This is not Harness²'s complete nested harness optimization, PRISM's multi-generation Pareto search, or DeepSeek's full TypeScript PTC sandbox. No new SDK, persistent optimizer, autonomous core modification or second job worker was added. The unchanged normal Rust agent handles remaining actions and mandatory owner acceptance.

**Public code consulted (2026 releases):**
- DeepSeek Harness: https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/tool-executor-ptc/README.md
- Google Research Harness²: https://github.com/google-research/harness2/blob/main/harness2/core/recursion.py
- Airbnb Agent Harness Optimizer: https://github.com/airbnb/agent-harness-optimizer

**Tests added but NOT executed in this editing environment:**
- `cargo test -p dominion-agent-sdk probes::tests::`
- `cargo test -p dominion-agent-sdk replay::tests::contrastive_probe_receipts_have_model_attribution_without_tool_replay`
- `cargo test -p dominion-agent-sdk --test kernel_replanning contrasting_read_only_probes_are_observed_then_the_kernel_replans`
- `cargo test -p dominion-agent-sdk --test kernel_replanning speculative_effectful_probes_are_rejected_before_any_tool_effect`
- `cargo test -p dominion-agent-sdk --test kernel_replanning probes_cannot_reserve_more_than_the_shared_tool_budget`
- `cargo test native_provider_preserves_typed_calls_and_accepts_probe_envelope_only_when_explicit`

No claimed runtime lift, token savings, real model completion increase or passing CI at this HEAD. A useful follow-up benchmark must compare identical frozen code tasks/model/tools/budget with and without candidate probes, including completion quality, wall-clock time, receipts, model calls and actual priced tokens.
