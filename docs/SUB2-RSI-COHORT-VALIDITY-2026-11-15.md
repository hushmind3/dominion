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
