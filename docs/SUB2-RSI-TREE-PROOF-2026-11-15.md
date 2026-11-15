# Sub2 RSI: independent evaluation → adopted Git tree (2026-11-15)

## Executable path

- API / campaign → Rust `rsi_evolve::run_loop` → detached `rsi_worktree::prepare/finish` → model-generated diff.
- `rsi_worktree::compare_at` verifies origin source-clean status, the frozen base commit and patch SHA-256, then invokes coordinator-owned `scripts/rsi-lab.mjs` against baseline and candidate worktrees created from the same immutable revision.
- Before injected oracle code or compiled tests can alter working trees, `rsi-lab.mjs` records the baseline and candidate Git tree object hashes. `scripts/rsi-measure.mjs` places them in the evidence report. Rust checks baseline against the actual base commit, refuses invalid/identical candidate trees and binds both hashes to the evaluation.
- `rsi::replayed_pair_metrics` replays the original measured cases and verifies the report-tree fields against the stored candidate evaluation. `rsi_evolve::adopt` compares the staged descendant source tree to the evaluated tree before commit. `rsi_worktree::apply_campaign_to_sub` rechecks the paired receipt and verifies the reconstructed patch tree equals the selected commit tree and its recorded evaluation tree.

## Constraints and evidence

The frozen A/B holdouts decide measured quality. Tree attestation prevents a different source revision from being reported as that evaluation, not performance overfitting. Old v2 receipts remain readable but carry no pre-oracle tree proof. Detached Git worktrees isolate source and indexes, **not** the host OS or executed native code. No production deployment occurs from an experimental adoption.

## Verification commands

`node --test scripts/rsi-lab.test.mjs`

`cargo test --release paired_v3_tree_attestation_rejects_mismatched_descendant_and_accepts_legacy`

`cargo test --release`

The last command and long-running multi-generation proof are separate from source-only checks. No unexecuted check is reported as passed.
