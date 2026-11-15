# Native source verification and autonomous repair — 2026-11-15

## Single end-to-end product behavior

The second focused Dominion capability is not a new page or generic coding prompt. It is a **Rust-owned code acceptance loop**: owner permission and exact test specification → actual workspace reads → bounded exact edits → durable tool receipts → independent source tests → diagnosed failure → revised edit → new passing tests → final status. The React workspace and TypeScript SDK submit one request to the same native implementation.

~~~text
Rust TaskInput (allowWrite, allowShell, acceptanceChecks, maxSteps/tokenBudget)
          │
          ▼
SQLite queued task ─── same-workspace writer lease ──► claim
          │
          ▼
Model inspect (workspace_list/search/read/read_lines)
          │
          ▼
Bounded write / exact replace / staged multi-file replace
          │                 │
          │       SQLite tool_start → actual effect → tool_result
          ▼
Model proposes completion
          │
          ▼
Native acceptance check dispatcher
   ├── no check + strict evidence → needs_review (cannot certify)
   ├── previously passed after newest edit → reuse durable proof
   ├── failed at unchanged revision → reuse diagnostic, no re-run
   └── missing/stale proof → direct argv verifier (90-second cap)
                           │
             stdout + stderr + exit status recorded
                           │
                ┌──────────┴───────────┐
                ▼                      ▼
           exit success          actual failure
                │                      │
         acceptance_complete      repair_required
                │                      │
         optional critique    full diagnostic to model
                │                      │
                ▼                  new source edit
           final task result ◄────────┘
~~~

All external side effects have persisted intent preceding execution; receipt persistence is required before continuing the next model turn. Tool failure, timeout or process crash never silently becomes a success. The existing task continuation interface uses the archived event trail to flag uncertain effects rather than blindly replaying them.

## Deterministic invariants

**Independent verification.** The model cannot mark a test as passing. Only a native \`workspace_check\` receipt with a real process exit code can satisfy an exact owner-defined executable+arguments combination. The engine runs it itself after the model proposes completion. A model executing some *other* harmless green check does not prove the requested acceptance.

**Proof freshness.** \`src/engine/verification.rs\` maintains a monotonically increasing mutation revision. Every attempted local write/replace/batch, MCP effect or source-format command invalidates earlier test results, including potentially partial failed writes. A green test on revision 3 cannot prove revision 4. Multiple required checks each need independent green evidence on the final revision. A failed check for unchanged revision is presented as feedback without repeating the same potentially expensive command.

**Repair.** On a failed check, the engine records the check and bounded stdout/stderr, then appends a clearly labeled untrusted diagnostic. With remaining owner-specified step budget, the same agent must choose an actual source change before a new green proof is admitted. When steps or permission are insufficient, the task ends \`needs_review\`. There is no invented workaround or fabricated test pass.

**Commands and budgets.** The verified command set is small by design: approved \`cargo test/check\`, \`npm test\`/known build scripts, and \`node --test\` with relative local JavaScript test filenames. Rust never concatenates executable arguments into a shell script. Commands have bounded output, timeout and argument counts; they execute only when \`allowShell=true\`. Per-task token/estimated KRW ceilings and cancellation remain in the native event loop. User tests can be supplied via \`acceptanceChecks\` (up to four). Without explicit checks, the implementation detects a known manifest only when appropriate. No manifest or test runner means no claimed test proof.

**Single-file durability.** Native \`workspace_write\` now stages and syncs file bytes before adoption, checks the prior revision, preserves executable permissions and attempts backup restoration on a failed readback. Native \`workspace_replace\` calls the same staged exact-edit transaction as \`workspace_replace_many\`, eliminating the previous in-place truncate/write path. These are best-effort local filesystem protections; a crash during rename may still leave a recoverable backup, and other OS processes are not serialized by the in-memory workspace lease.

**Multi-file source changes.** \`workspace_read_lines\` returns lines, cursor and file SHA-256 even for large UTF-8 sources. \`workspace_replace_many\` prepares up to 8 distinct files after checking exact source spans and optional source SHA-256. Updated bodies are staged next to the originals, synced, source revisions rechecked, moved into place with per-file backups, read-back verified and rolled back if adoption fails. This is **best-effort staged multi-file adoption**, not a filesystem-wide atomic transaction, Git commit or safe rollback after a process kill. An uncertain failed operation remains visible in the event journal.

**Concurrency.** One shared native workspace lease spans the *entire* mutating task and its final acceptance, rather than protecting only each file write. No other Dominion task with write/shell/MCP capabilities can interleave edits and invalidate the first task's proof. Truly read-only tasks remain parallel. The current single-owner desktop runtime is not a multi-user sandbox; cross-process locks and per-task worktrees are separate future work.

**Completion evidence.** On success, \`result.acceptance\` includes the exact passing check names, revision and \`passed:true\`; \`result.verification\` distinguishes fresh commands from write-readback-only evidence. The React tasks screen displays latest \`acceptance_result\`, \`repair_required\` and failure stderr; the SDK's TaskInput and TaskRecord carry the same data.

## Entry points

| Layer | Implementation |
| --- | --- |
| Rust SDK task admission and allowed checks | \`src/runtime.rs\` → \`TaskInput::validate\` / \`normalized\` |
| Persistent source and acceptance evidence | \`src/store.rs\` events / \`last_acceptance\` |
| Actual agent orchestration | \`src/engine.rs::run\` |
| Deterministic acceptance state and command policy | \`src/engine/verification.rs\` |
| Tool transport and bounded source edits | \`src/engine/files.rs\`, \`src/engine.rs::tool\` |
| Native task API | \`src/api.rs\` /api/tasks and per-task SSE |
| Browser | \`web/src/pages/NativeWorkspace.tsx\` |
| External typed transport | \`sdk/typescript/src/index.ts\`, \`sdk/typescript/src/runtime.ts\` |
| Functional source integration tests | \`src/engine/acceptance_tests.rs\` |

## Verified test cases

The project workflow runs \`cargo test\` and independent TypeScript/workspace checks. The acceptance tests run native Node processes and use a mock model **only to choose its next action**. They are not mocked command results.

- \`wrong_multifile_repair_receives_real_test_failure_and_fixes_it\`: model reads actual source and test, first changes the wrong expression, produces a final answer, receives a real failed Node assertion and repairs the exact expression in the same Rust task. A new real test passes.
- \`repeated_final_claim_cannot_recheck_unchanged_failing_source\`: repeated model success text without a new source change does not re-run the same failing test and cannot yield a completed status.
- \`old_green_test_is_invalidated_by_newer_bad_write\`: a successful user-requested Node check *before* a destructive later edit cannot certify the new revision.
- \`native_agent_can_change_two_files_then_pass_real_joint_acceptance\`: exact replacement on two modules is staged and read-back verified; an actual joint Node test passes and the final record contains two verified changes plus native acceptance proof.
- \`concurrent_coding_tasks_hold_the_workspace_lease_until_source_acceptance\`: simultaneous writers invoking a deliberately delayed mock model observe a maximum active writer count of one and persist both artifacts.
- \`engine::verification::tests::fresh_proof_is_invalidated_by_even_a_failed_source_edit\`: unit proof that an uncertain attempted mutation invalidates old green status.
- \`engine::files::tests::multi_file_edits_are_prevalidated_and_readback_verified\`: preflight rejection leaves both original files unchanged; a valid batch commits both and verifies hashes.
- \`engine::files::tests::ranged_source_read_returns_stable_revision_for_large_files\`: numbered source reading works beyond the earlier all-or-nothing file size bound.

For real frontier parity, compare on identical SWE-bench Verified tasks, repositories, model, time, granted tools, budgets, tests and scoring harness; benchmark outcome is **not measured here**.

## 2026 design comparisons

- OpenAI Codex Rust tool handler uses explicit patch parsing, hook-aware authorization and runtime execution separation. We adopted the principle of native-side verified edits and committed evidence, not the external Codex engine itself. https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/apply_patch.rs
- Anthropic's July 2026 verification-loop guidance differentiates repeated real tests from model self-assurance. We enforce a deterministic acceptance gate with fresh source-revision proofs and actual diagnostics. https://claude.com/blog/building-verification-loops-in-claude-code-with-skills
- Qwen Code's 2026 agent documentation treats independent specialized workers as focused tasks with separate state and evidence; Dominion continues to use its Rust subagent tool rather than a copied vendor SDK. https://github.com/QwenLM/qwen-code/blob/main/docs/developers/tools/task.md

## Unfinished frontier work

Real-provider repair success rate and token cost across diverse codebases remain unmeasured. Source edits currently run directly in a single-owner workspace under a shared lease, not a hardened process sandbox or isolated per-task Git worktree. Rollback after abrupt power loss and conflict resolution with external editors require further work. Browser visual regression and dependency-install policies are not present. None of those are represented as passing features by this proof.

## Check-time source integrity

Every approved executable check now measures a bounded, deterministic SHA-256 inventory of workspace source/configuration files before and after the real process. A green exit code is insufficient if the command rewrites or creates source files. The receipt includes sourceUnchanged, sourceBeforeSha256 and sourceAfterSha256; any mismatch forces success=false and invalidates earlier acceptance proofs. The agent gets the recorded diagnostic and can attempt a new repair within the original step budget.

Excluded generated dependency/build directories include node_modules, target, dist, build and caches. The audit does not follow symlinks and fails closed if the project cannot be read or exceeds its declared inventory bounds. It is not a hardened sandbox: transient modifications reversed before the final hash and changes to non-source artifacts can escape detection. Untrusted projects require OS isolation as a separate engine backend.

Real Node integration regression: green_test_that_rewrites_source_is_rejected_at_real_native_completion. Unit checks cover generated output exclusion, symlink non-following and invalidation of a prior green proof.
