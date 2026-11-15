# Sub2: independently measured A → B → A transfer (2026-11-15)

## Source-to-result chain
1. Each generation must have an adopted candidate with a frozen, independently evaluated holdout gain in its **targeted domain** (not just a free-text claim).
2. The evaluator replays the full original `rsi_pair` measurement against the original frozen plan, requires the v3 pre-oracle Git-tree attestation, and matches candidate, base, accepted successor, and per-generation evidence SHA.
3. Three measured, consecutive **accepted source revisions** must form A→B→A where A != B. Unsuccessful research attempts may occur between commits; an accepted revision with missing evidence or a broken Git lineage interrupts the chain.
4. For system-wide RSI, the actual spawned worker executable and compiled controller also need a matching durable receipt. Flags, patched summaries and a forged domain label cannot earn transfer credit.
5. The existing `GET /api/rsi/campaigns/:id` and `/verify` surface `crossDomainTransfer`, and the Self-Improvement screen shows measured cycles or a next-return-domain **hint**. No unverified model output is used as a verdict.
6. In automatic whole-system evolution, the GUI sends `crossDomainCycle:true`. The API enables this only when the initial owner request was empty (autonomous mode); user-specific objectives and manual selection remain authoritative. Only after **two independently measured accepted improvements in distinct domains** does the freshly built descendant independently replay the A/B records and propose returning to the earlier domain. It does not claim success until a new candidate passes its own held-out comparison. The supervisor and the actual worker both independently re-audit any override of a previously assigned paired target; a forged compiled-controller plan, unverified receipt or user-specified source objective cannot trigger it.

This is a source/measurement lineage proof, not yet a controlled causal experiment showing the changed improver is better than an unchanged generator at equal budget. The latter still relies on the separate `rsi_meta` frozen matched-budget downstream worker episodes.

## Downstream paired-episode replay (Sub2 next stage)
`rsi_meta::verified_episode_receipt` now reconstructs per-task baseline/candidate quality and mean model-call counts from the preserved coordinator-owned episode rows. It requires a complete set of both arms, matching generator revisions, fixed subject revision, model routing, bounded per-arm tokens, real spawned workers, source-tree attestation in system mode, and the raw subject grader report for every positive gain. Invalid, missing, duplicated, or edited rows cannot silently earn an improver win by changing a summary field. The same verifier runs before writing the meta receipt and again whenever `rsi::verified_pair_receipt` replays an improved candidate's frozen evaluation. It is replay-only and does not add a model call.

## Verification
- `cargo test --release actual_replayed_holdouts_prove_a_to_b_to_a_not_three_flags`
- `cargo test --release heldout_or_executable_tampering_never_counts_as_transfer`
- `cargo test --release successor_plan_is_authoritative_and_rejects_parent_override`
- `npm run typecheck --prefix web`
- `cargo test --release`

The unit tests contain synthetic but internally recomputed receipts; they do not claim a real paid/model-driven cross-domain cycle has already succeeded.
