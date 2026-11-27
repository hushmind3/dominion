# Sub2 2026 native-chat acceptance repair gate

**Scope:** `branch2` only. Existing product chat path `run_turn_body` → `verification::check_acceptance` → existing Rust tool dispatcher → `finish_attempt`. No new worker, agent runtime, policy owner, database collection or prompt-driven autonomous scheduler.

## What was broken

- Once a user-facing action finished with unmet deterministic acceptance conditions, the general-chat runtime attempted a second independent review-model call even when there was **no successful tool evidence** to review.
- The existing acceptance retry counter admitted further model attempts without checking whether the deterministic gaps and tool result set changed. Repeating a file read or a failed command could keep buying inference.
- The model got a generic `try another strategy` retry instruction rather than the existing shared Rust SDK failure-class guidance.
- A final answer boundary was not an explicit hard stop for uncertain non-idempotent external effects before the proposed next repair.
- Model review received `serde_json::to_string(evidence)` without a bound; a large tool output could be re-sent in a second provider prompt.

## Changed execution semantics

```text
User action + frozen acceptance checks
  ↓
model tool execution (unchanged native dispatch, authorization, effect seal)
  ↓
deterministic acceptance verifier
  ↓
indexed per-execution tool operations + effect states
  ├─ unresolved remote mutation → stop and require state inspection
  ├─ same gaps and same unique tool results after a retry → stop (stalled)
  ├─ unmet checks + no successful tools → skip secondary model review
  ├─ unmet checks + successful tools + first retry → optional bounded reviewer
  └─ admitted retry → shared Rust SDK failure-class next step
  ↓
existing finalization with receipt/usage audit
```

### Specific distinctions

1. **No false progress from tool IDs.** `repair_signature` fingerprints sorted unmet checks plus canonical tool names, arguments, result and effect states. Operation UUID and timestamp are excluded, and identical repeated observations are deduplicated. Model prose is never proof of progress.
2. **No reviewer without evidence.** `review_worth_calling` requires a completed tool operation; the independent reviewer can only advise, and its responses cannot override `verification::check_acceptance`.
3. **Bounded secondary input.** `bounded_review_receipts` takes at most 8 successful operations; any result excerpt is at most 400 characters. Full receipts are neither altered nor removed from SQLite. No arbitrary terminal logs or page bodies are pasted into the helper prompt.
4. **Unknown external effect.** `unresolved_mutation` uses the existing `effect::is_effectful` classification and recognizes pending / unknown states. An `inspected` operation may retain its historical `outcomeUnknown` flag for audit while no longer being a *pending* barrier; it is not upgraded to a verified payment, repository change or other external fact.
5. **Shared failure taxonomy, not an extra harness.** Native chat retry text now draws from `dominion_agent_kernel::failure::classify(...).next_step()`, the same Rust SDK kernel used by autonomous job execution. This distinguishes compile errors, failing tests, missing evidence, model/provider trouble and denied permissions.
6. **Existing limits stay authoritative.** `max_acceptance_retries`, `max_rounds`, the tool budget, cancellation, token reservations, approval and normal tool gateway remain the original sources of execution permission. A reviewer never grants a tool permission.

## Regression coverage

The current tests exercise no-evidence reviewer skipping, one-review-only gating, unchanged repeated reads, changed observation contents, uncertain external effects, inspected historical uncertainty and bounded reviewer input. Registered in `.github/workflows/ci.yml`:

```sh
cargo test --lib engine::chat::runtime::action_review::tests
```

The existing CI also runs scoped SDK kernel, tool scheduling and durable effect recovery tests. These are structural and deterministic execution-path tests, **not measurements of Qwen/DeepSeek/Claude real-model task success**.

## Relevant 2026 work

- [NanoHarness: Beyond the Model, harness effects, arXiv 2609.32459](https://arxiv.org/abs/2609.32459): tool schema, planning, context compression and harness architecture influence agent completion.
- [HarnessFix: diagnosing/repairing harness flaws, arXiv 2606.06324](https://arxiv.org/abs/2606.06324): use execution provenance to attribute failures and constrain repair operators rather than treating a model's claimed completion as a result.
- [Bounded Loops, arXiv 2609.27871](https://arxiv.org/abs/2609.27871): model execution and independent acceptance should remain separated under an explicit budget.

These papers motivate the repair-gating design. This patch **does not reproduce their published benchmark results**, nor does it claim that this implementation caused a specific percent-point lift.

## Remaining work

Live model A/B under the same task/model/tool grants, same `maxRounds` and token budget; explicitly record accepted tasks, externally verified effects, model/tool call counts, prompt tokens, retries, wall time, and near-duplicate tool calls. Full release CI and actual deployment compatibility are out of scope for this targeted development check.

No modifications to the main, sub3, sub4 branches or frontend visual layout.
