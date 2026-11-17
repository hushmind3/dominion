# Sub2: bounded evidence-specific repair and non-confounded UCB credit
Date: 2026-11-18
Branch: dominion-sub2

## Source comparison
- Airbnb Agent Harness Optimizer (2026) tests harness changes on held-out tasks instead of grading by code volume: https://github.com/airbnb/agent-harness-optimizer
- Qwen Code 2026-10-09 recovery design emphasizes checkpoints and no speculative replay: https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-09-hosted-g3-step3-recovery.md
- Anthropic 2026 design guidance stresses selective tool context and evidence-backed completion: https://github.com/anthropics/skills/blob/main/skills/claude-api/shared/agent-design.md

## Corrected evidence attribution
`src/engine/algorithms.rs` already had a UCB1 selector and direct `mark_signal` receipts. Before the patch `usage_rating` gave **every** selected algorithm a whole-task success when direct domain-specific evidence was absent. This confounded several strategy choices in the same task, inflating the preference for all of them.

`competing_usage_counts` now groups existing usage rows by execution in one already-required memory scan. Direct domain evidence retains precedence. Task-outcome proxy credit is limited to an execution with exactly one selected algorithm; otherwise usage is `unrated` (retained) and explains the missing attribution. `finalize_execution` and explicit `rebuild_stats_from_usage` use identical rules, preventing a later rebuild from reintroducing false credit. Statistics are *not* silently rebuilt on startup, and the existing owner/API usage remains.

## Failure-specific retry
`crates/dominion-agent-kernel/src/failure.rs` implements a pure bounded classifier with seven categories. Existing `Cycle::retry_instruction` adds one category-specific next action to the already bounded retry prompt. `src/engine/autonomy.rs` stores the classification with the attempt receipt. It does not bypass explicit tool approval, retry after an uncertain external effect, extend the budget or add model calls. The host's proven verifier and decision gates remain authoritative.

## Regression and evaluation
Focused Rust debug-profile tests cover multi-strategy confounded successes, signal-based failures, classifier priority and test-specific next steps. No release or frontend builds are requested. Source-level attribution correctness is not a live-model completion gain; compare same model/temperature/tools/token budget across held-out tasks and report verified completion, average billed tokens, cost, wall time, and recovery reason separately.
