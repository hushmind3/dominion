# Sub2: independently measured A → B → A transfer (2026-11-15)

## Source-to-result chain
1. Each generation must have an adopted candidate with a frozen, independently evaluated holdout gain in its **targeted domain** (not just a free-text claim).
2. The evaluator replays the full original `rsi_pair` measurement against the original frozen plan, requires the v3 pre-oracle Git-tree attestation, and matches candidate, base, accepted successor, and per-generation evidence SHA.
3. Three measured, consecutive **accepted source revisions** must form A→B→A where A != B. Unsuccessful research attempts may occur between commits; an accepted revision with missing evidence or a broken Git lineage interrupts the chain.
4. For system-wide RSI, the actual spawned worker executable and compiled controller also need a matching durable receipt. Flags, patched summaries and a forged domain label cannot earn transfer credit.
5. The existing `GET /api/rsi/campaigns/:id` and `/verify` surface `crossDomainTransfer`, and the Self-Improvement screen shows measured cycles or a next-return-domain **hint**. No unverified model output is used as a verdict.

This is a source/measurement lineage proof, not yet a controlled causal experiment showing the changed improver is better than an unchanged generator at equal budget. The latter still relies on the separate `rsi_meta` frozen matched-budget downstream worker episodes.

## Verification
- `cargo test --release actual_replayed_holdouts_prove_a_to_b_to_a_not_three_flags`
- `cargo test --release heldout_or_executable_tampering_never_counts_as_transfer`
- `npm run typecheck --prefix web`
- `cargo test --release`

The unit tests contain synthetic but internally recomputed receipts; they do not claim a real paid/model-driven cross-domain cycle has already succeeded.
