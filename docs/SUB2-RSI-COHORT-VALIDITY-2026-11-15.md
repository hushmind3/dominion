# Sub2 — Independent RSI controller selection and measured forecast feedback (2026-11-15)

## Native execution integration

- The existing native Rust RSI pipeline remains unchanged: `rsi_evolve` chooses a descendant task, `rsi_edit_method` verifies candidate adoption evidence, and `rsi_edit_genome::evolve` queries `rsi_rule_tournament::compare` to select or rechallenge an executable edit controller.
- The controller tournament now requires two separate **generator Git commits** to establish two wins. Changing objective or partition labels on the same generator commit does not create an independent win. Cross-context job/candidate reuse invalidates the affected contexts.
- Multiple retries per (source/model/objective/partition, executable policy revision) use the **lowest** measured reward; observation order no longer rewards cherry-picked outcomes. Nonfinite rewards and malformed generator hashes are excluded. This affects selection of the next *method*, never direct adoption of source code.
- `rsi_forecast` calibrates domain-specific prediction errors by recomputing signed error from the original forecast and the paired evaluator's `improvedHoldouts`. It no longer trusts stored `forecast.error` or `forecast.bias`. Domain observations precede global fallback even if a domain has not yet accrued a forecast record, and a result from the wrong domain or an ineligible/unpaired evaluation is ignored.
- Existing Rust worktree isolation, frozen paired evaluation, candidate adoption, tool permissions, and spawned worker provenance remain independent gates.

## New deterministic regression coverage

`rsi_rule_tournament.rs`: duplicate source under different objectives, conservative retry scoring, cross-context job/candidate reuse, non-finite rewards, and previous matched/contradictory trials.

`rsi_forecast.rs`: forged cached bias/error, domain-vs-global measured fallback, off-domain/unverified result exclusion, plus existing correction and empty-history coverage.

`rsi_edit_genome.rs`: existing descendant-policy promotion fixture now uses full 40-character generator commit identity to match production lineage requirements.

Suggested targeted checks, not claimed as executed:

```bash
cargo test --release rsi_rule_tournament::tests
cargo test --release rsi_forecast::tests
cargo test --release independently_promoted_policy_can_mutate_again_after_a_real_failure
```

## Evidence boundary

These changes prevent particular forms of spurious tournament wins and improve forecast-data integrity. They do **not** prove a real multi-generation causal lift on held-out tasks; that requires equal-budget frozen descendants evaluated independently before promotion. The implementation is on `dominion-sub2` with `[skip ci]`; no CI pass is claimed.

The distinction between selecting a candidate using independent empirical evaluation and claiming general self-improvement is consistent with the ICLR 2026 GEPA and Huxley-Gödel Machine directions, while no external agent runtime or training code was imported.

References:
- https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html
- https://openreview.net/pdf?id=T0EiEuhOOL


## Follow-up: end-to-end runtime audit (2026-11-15)

A deeper inspection of `rsi_evolve::evolve` → `rsi_worktree::compare_at` → `rsi::replayed_pair_metrics` → `rsi_edit_method::history_with_verified_payoffs` → `rsi_edit_genome::evolve` exposed two feedback problems:

1. Forecasting previously conditioned on eligible successful results. Neutral or regressed experiments that had a complete paired receipt were discarded, biasing the predicted improvement upward. A replayed but non-eligible candidate now supplies a measured **zero**. An unverified result contributes no sample at all. The forecast is calculated from a freshly revalidated history in the live campaign, while error/bias are recomputed from the originally issued forecast and observed result.
2. The edit-method optimizer could grant the neutral `0.10` reward from a candidate-shaped generation and a source label without checking the stored original receipt. `pairedEvidenceVerified` now requires an actual replayed candidate/raw pair identity, and positive reward still requires adopted source evidence. Model-origin work with zero native verified writes receives no model-editor reward. Replayed evidence must match the generating worker/campaign when those provenance fields are present.

Tournament matches now also require the *exact* frozen case-plan SHA-256, oracle version, and target domain, not merely source/model/objective. A forged `editPolicy.revision` is excluded from controller promotion by comparing the advertised revision against the canonical executable genome. These checks protect promotion, not the underlying Git source adoption gate.

The only runtime files changed in this continuation are:
- `src/engine/rsi_edit_method.rs`
- `src/engine/rsi_evolve.rs`
- `src/engine/rsi_edit_genome.rs`
- `src/engine/rsi_forecast.rs`
- `src/engine/rsi_rule_tournament.rs`

The `web/`, `src-tauri/`, `src/api/`, and SDK trees are untouched. The original main-branch UI stays intact.

### Evidence boundary and focused regression commands

New Rust tests cover neutral trial replay, tampered raw measurement, attribution to the wrong worker, absence of verified model writes, bogus policy revision, case-plan mismatch, task/domain mismatch, and forecasts conditioned on replayed zero-gain trials.

```bash
cargo test --release rsi_rule_tournament::tests
cargo test --release rsi_forecast::tests
cargo test --release replayed_neutral_pair_trains_zero_gain_without_faking_an_improvement
cargo test --release independently_promoted_policy_can_mutate_again_after_a_real_failure
cargo test --release edit_method_reward_requires_measured_quality_for_full_credit
```

No local Rust toolchain or full CI was available in the present session, so these tests are **added but not claimed to have passed**. A demonstrated improvement in descendant performance requires separate equal-budget paired model campaigns with retained raw outcomes.

## GitHub Actions verification request (2026-11-15)

This commit explicitly requests the existing push-triggered CI workflow on the current Sub2 HEAD, including the Ubuntu/macOS/Windows release-mode Rust tests and web typecheck/build. No UI files or application configuration have been changed. CI conclusions must be read from GitHub Actions and are not presumed here.


## Windows native execution and cross-platform test hygiene — 2026-11-16

This continuation is intentionally restricted to the `dominion-sub2` branch and leaves all UI assets untouched.

- `rsi_worktree::git_compatible_path` is used for Git worktree CLI destinations when Windows canonical paths carry `\\?\` extended-length prefixes; the native filesystem `PathBuf` stays unchanged.
- Compiler-assisted RSI suggestions normalize Rust compiler span separators before matching the independent `git diff --name-only` allowlist.
- Native skill installation avoids a case-insensitive source-to-itself copy; local GGUF IDs and unsafe audit findings use slash-normalized identifiers.
- Tests now drop the owning SQLite-backed `AppState`/`Store` and worker contexts before deleting temporary test directories, fixing the Windows file-lock lifecycle rather than skipping assertions.
- `.gitattributes` explicitly pins Rust source line endings to LF for deterministic source-driven RSI mutation checks on Windows. This is a repository source contract, not a UI change.
- Existing end-to-end chat fixture retains a bounded but Windows-realistic deadline.

Verification is the existing cross-platform GitHub Actions `cargo test --release` matrix, web checks and script tests. No tests are excluded, no platform jobs are disabled, and no runner result is claimed before its actual completion.
