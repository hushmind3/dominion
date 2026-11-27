# Sub2 — Native harness decision fixes (2026-11-18)

Scope: `branch2` only. This is a **behavior change in the Rust SDK/verification harness**, not another job-status repaint. No other branch or frontend menu was modified.

## Why this was necessary

The owner's 2026-11-18 18:17 experiment for commit `555f73b0` reported sub2 coding files **3/3 externally verified**, **71,286 tokens** and coding t2 finishing with `needs_review`. The original raw bench ZIP was in the owner's private Downloads folder, not attached here. The precise t2 failure class therefore remains an unconfirmed hypothesis, not a proven attribution.

Inspecting actual Rust source revealed independently reproducible harness defects:

1. `crates/dominion-agent-kernel/src/cycle.rs::Cycle::assess` calculated `Decision::Accept` for evidence-backed completed work, then **unconditionally overwrote it to Review** whenever `token_budget_exhausted` was set. Completing the last valid work at a token boundary could therefore yield `needs_review` even with no unmet criteria.
2. `crates/dominion-agent-kernel/src/lib.rs::side_effect_unknown` passed only `terminal_exec.arguments.command` into `read_only_shell_command`; tool `{"command":"cargo","args":["test","--lib"]}` was judged as bare `cargo`, not an actual `cargo test`. Safe, finished verification work could prevent a necessary repair attempt by being classified as non-replayable.
3. The prompt-job HTTP adapter dropped any `successCriteria` (only `verifyCommand` survived), and `autonomy::execute` always inferred file targets from unstructured natural language. For tasks mentioning reference files and outputs, the host had no frozen explicit owner criteria to separate the two.
4. `add_explicit_verifier` rejected adding a previously present verify command when the criteria array already held 20 checks, incorrectly reporting a 21st criterion.

## Changed behavior

### Kernel terminal budget gate

- Verified **Accept survives an exhausted token budget**, provided the owner's goal has not changed and no genuinely unknown tool result is present.
- An unfinished task still cannot purchase more model attempts after budget exhaustion: `Repair → Review`.
- Owner revisions, stale worker epochs, unknowable remote effects and denied permissions continue to block success.
- SDK unit test covers verified success, unmet conditions, owner steering race, and unknown remote effect.

### Native command provenance

- `terminal_exec` read-only classification reconstructs the executable plus **actually supplied argv** only for its policy check; it never passes those strings through a shell.
- Plain read-only `cargo test`, `npm run typecheck`, `git -C project diff`, and `rg` no longer become false-positive non-replayable effects.
- `git commit`, `npm install`, arbitrary `python -c`, malformed argv, or shell metacharacters remain blocked from automatic replay.
- `observed_unknown_outcome` exposes truly uncertain operation receipts separately from *known settled but non-idempotent* work. A known successful POST is not silently replayed but a verified final result can still complete; an expressly unknown write cannot.

### Frozen owner criteria

- `POST /api/jobs` accepts `input.successCriteria` (up to 20 verified `file_exists`, `file_changed`, `file_equals`, `file_contains`, `file_absent`, `tool_succeeded` and `command_succeeded` checks) and validates it using the existing native validator.
- At HTTP ingress, only documented fields are copied; a client cannot inject `baseline`, `postSha256`, `creationOnly` or fabricated proof.
- `autonomy::acceptance_contract` freezes these checks **before the first model/tool operation** using host-measured baseline, or falls back to the existing inferred acceptance criteria when no explicit contract exists.
- `verifyCommand` is still independently enforced, remains post-edit fresh and is deduplicated against preexisting `command_succeeded` checks.
- Owner goal *replacement* resets superseded criteria and their evidence epoch; old effects remain recorded.
- No artifact success is inferred from assistant prose or client-provided receipts.

### Terminal audit record from prior change

`autonomousState` now seals on completion/review with exact unmet checks, evidence receipts, error, and separate replay-safety/unknown-outcome fields. It is not a substitute for independent verification.

## Regression tests

- `dominion-agent-kernel` Rust tests: budget/acceptance and native `terminal_exec` argv/no-replay policy.
- `engine::autonomy::tests::frozen_owner_acceptance_contract_overrides_ambiguous_path_inference`: reference-file versus output-file, validated literal target and verifier deduplication.
- `api::jobs::harness_contract_tests`: public job request preserves owner criteria but rejects forged baseline metadata.
- Previous `verifier_terminal_checkpoint_records_review_without_promoting_file_success` and native cancellation/lease guards retained.

The development workflow selects this suite and frontend typecheck. **CI success should be claimed only for the GitHub Actions run attached to the code SHA, not inferred from a documentation-only commit.**

## What has NOT been proven

The original t2 `needs_review` may have been caused by the terminal budget overwrite, structured argv misclassification, absent explicit acceptance checks, lack of a workspace SHA receipt from a shell edit, or another detail. Without its raw `job.result.receipts`, `unmetChecks`, `tokenUsage`, and actual tool events, no single cause is asserted.

This fix intentionally does NOT automatically certify file changes performed by an arbitrary shell command solely because the file exists. User-file correctness, provenance of the operation, and safety of replay remain separate checks.

Research context (not a substitute for repository evidence):
- [Rethinking Verification in Agent Harnesses (2026)](https://arxiv.org/abs/2610.34577)
- [Structured Feedback Improves Repair in an LLM Agent Loop (2026)](https://arxiv.org/abs/2607.14167)

Next A/B metric: coding `3/3`, job `completed/needs_review`, number of model calls, input+cached tokens, verifier gaps, and completion quality at equal budget. A drop in tokens has NOT yet been measured on live GLM-4.7-Flash.
