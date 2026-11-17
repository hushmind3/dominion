# Sub2 — independent SDK model-order tool batching (2026-11-18)

## Problem
The live `parallel_prefix(calls,capacity)` function checked `capacity <= 1` but then called `take_while().count()` **without `take(capacity)`**. For 40 adjacent read tool calls, it admitted 40 into a nominal four-slot batch, consumed all related proposal/budget bookkeeping and could delay model-visible progress. `plan(names,4)` bounded the batch but was not the routine used at the paid live call boundary.

## 2026 references
- DeepSeek Harness (2026-07-10) models each tool as concurrency-safe or exclusive, allowing argument-sensitive per-tool policy: https://github.com/deepseek-ai/deepseek-harness/blob/master/.agents/notes/implemented/feature/2026-07-10-parallel-tool-call-execution.md.
- Qwen Code (2026-09-27) preserves workspace tool turn order and exclusive workspace execution: https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-09-27-hosted-workspace-tool-turn.md.
- Sub3 `dominion-agent-sdk/src/runtime.rs` owns per-call budget admission and read/write barriers independent of the host. Sub2 adapts the safe admission concept without installing sub3's parallel or session engine.

## Code
- `crates/dominion-agent-kernel/src/scheduling.rs`: public, model/provider-independent typed `ToolBatch`, `safe_to_overlap`, `parallel_prefix`, `plan`. Maximum 16, default caller capacity 4. Tool argument strings must decode to JSON objects. Unknown, mutating, browser, GitHub mutation, and MCP tools stay exclusive.
- `src/engine/chat/tool_schedule.rs`: re-exports independent classifier; retains existing rolling async pool and cancellation/drain contract. No new process or job loop.
- `src/engine/chat/runtime/turn_body.rs`: unchanged, already invokes `tool_schedule::parallel_prefix(...,4)`, so now honors the upper bound before proposal/tool charging and dispatch.

## Regression focus
- Forty adjacent safe calls admit only four.
- A write barrier/invalid argument cannot be crossed.
- The native Rust SDK and chat caller use the same decision source.
- No release builds, frontend tests or multi-platform jobs; run related Rust debug tests only.

## Optional Money dormant-state cost
- A separate 30-second native maintenance tick previously called `Store::all("money_autopilot")`, deserializing every archived autopilot record to decide whether Money is enabled.
- `Store::money_enabled_autopilot_exists` uses `SELECT EXISTS` bounded by the existing `records(kind,id)` primary key. If disabled, the single probe returns without source collection, scraping or an LLM request; no new index or per-tick cache invalidation is added.
- SQLite read failures are audited and stop execution rather than silently starting a Money campaign. A targeted Rust test checks 50 dormant records, active/disabled transitions, and the empty-store case.
- This is a structural reduction in JSON deserialization, not a measured CPU/time improvement.
