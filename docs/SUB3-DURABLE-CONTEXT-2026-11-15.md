# Dominion sub3: durable, retrievable Rust context (2026-11-15)

## Public source comparison — verified source paths

This implementation is based on specific observed design mechanisms, not a claim that external SDKs are embedded:

| Public 2026 implementation | Source read | Mechanism observed | Dominion implementation |
|---|---|---|---|
| OpenAI Codex | `codex-rs/core/src/context_manager/history.rs` and `codex-rs/core/src/compact.rs` | The model's bounded context window is separate from retained host-side facts and archived history; compaction does not imply deletion of the authoritative rollout | `crates/dominion-agent-sdk/src/context.rs::project` retains full `Snapshot.receipts`, exposing a bounded per-turn view |
| DeepSeek Harness | `packages/core/agent-loop/src/agent.ts`, `packages/compaction/compaction-tool-result-pruner/src/index.ts` | New model requests are derived from durable session facts, and oversized tool results are pruned only in the model-visible surface; the event history remains authoritative | Typed receipt records persist and `Kernel` reconstructs model input each turn without rewriting stored receipts |
| OpenCode | `packages/core/src/session/compaction.ts` | Large-context admission, token reserve, selective retained tail and compaction; history is not simply a fixed window of last 12 tools | Strategy-bounded recent observations + older failure retention + queryable history index; more sophisticated background model-produced compaction remains an independent future improvement |

Public source URLs:
- https://github.com/openai/codex/blob/main/codex-rs/core/src/context_manager/history.rs
- https://github.com/openai/codex/blob/main/codex-rs/core/src/compact.rs
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/agent.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/compaction/compaction-tool-result-pruner/src/index.ts
- https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/compaction.ts

## Production execution path

```
POST /api/jobs
    → src/api/jobs.rs
    → src/engine/jobs.rs::run_prompt
    → src/engine/native_agent.rs::run
    → dominion_agent_sdk::Kernel::{run,resume}
         ├─ context::project(receipts, strategy, result_budget)
         │    ├─ recentObservations (head+tail, bounded)
         │    ├─ importantPastFailures (retained beyond recent window)
         │    ├─ historicalReceiptIndex (bounded metadata)
         │    └─ executionDigest (counts, paths, omissions)
         ├─ Model::complete(system, bounded_prompt)
         ├─ scheduler::arrange(action_graph)
         ├─ context::execute_local(receipt_search | receipt_read, same_run_receipts)
         │    └─ bounded, read-only, no external tool permission / subprocess
         └─ Tools::execute(other_tool) → standard approvals + durable operation log
```

The kernel adds two internal, always available read-only tools to the *model* tool catalogue, not to external user grants:

- `dominion_receipt_search` with `query`, `offset`, `limit`: case-insensitive search across this run's previous operation IDs, tools, arguments, and persisted result JSON. Returns IDs and short previews, newest first, with pagination.
- `dominion_receipt_read` with `id`, `startChar`, `maxChars`: returns a bounded Unicode character slice of the *exact* durable result JSON. No re-execution of a shell command, MCP effect or file mutation occurs.

The JSON results from all kernel-local calls are also persisted as ordinary `Receipt` objects, so resume uses the same search and reading evidence. There is no second, unsynchronized memory database. Unauthorized external tools are still rejected by the Rust scheduler. The model can retrieve only receipts from its own run; requesting a foreign ID fails.

**Budget design:** `LowCost` projects up to 5 recent observations and ~4,500 characters of evidence-surface budget, `Balanced` 10/~10,000, `VerifyFirst` 16/~16,000, with separate small budgets for older failure and historical IDs. The full receipt collection is unaffected. These are approximate character budgets for the model's observation subview; actual provider token accounting and generated system tool schemas remain separately metered. Prompts keep task, owner directives, verification gaps, and current plan independently of truncation.

**Diagnostic handling:** very long results preserve the start and end with an omission marker. A broken build's final compiler diagnostic is not silently dropped because a large earlier log filled the viewport. Historical failure summaries remain discoverable even after hundreds of successful actions.

## Executed focused tests

- `crates/dominion-agent-sdk/src/context.rs`: projection size bounds, paginated search, no cross-run data, unicode-safe read, important failure retention.
- `crates/dominion-agent-sdk/tests/context_recall.rs`: a scripted planner resumes a long durable receipt log, searches the first failure, reads a page of exact content, and finishes without calling any external tool; invalid foreign-ID requests fail locally. A scripted model verifies the **kernel** context pipeline, not general intelligence.
- The dedicated `.github/workflows/sub3-rust-sdk-check.yml` checks Rust SDK, product Rust adapter, persisted-workflow recovery, frontend typecheck and production build. These are not full Dominion CI or SWE-bench results.

## Remaining frontier gaps

This implements **one real capability family**: durable context projection and model-driven retrieval for long tasks. It is not a parity claim for Codex's native Responses streaming, model/server-side compaction, provider prompt caching, dynamic tool schema negotiation, concurrency-controlled subagents, or broad long-run coding benchmark performance. The TypeScript SDK remains a remote client of the same Rust job path.
