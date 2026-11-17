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
