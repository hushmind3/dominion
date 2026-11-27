# Sub2 — single native tool owner and indexed effect admission (2026-11-18)

Branch: `branch2`. This document describes committed source changes and test intentions; no Rust build, test run, CI pass or real-model performance gain is claimed in this session.

## Execution topology

```text
Owner request / frozen verified replacement
    → model decision OR exact deterministic acceptance contract
    → one native dispatch_tool_call gate
        → tool argument normalization / configured capability check
        → durable prior-effect admission (indexed by session/resource/effect)
        → owner approval / native tool executor / cancellation and file locks
        → execution_operation record + effect seal/read-back + metrics
        → provider tool message only for a matching model tool-call envelope
    → postcondition checks / terminal result
```

The previous model-free `deterministic_tool` function carried a second partial tool executor with its own approval, dispatch, metrics and receipt writes. It is now an adapter to `dispatch_tool_call`, removing approximately **92 lines** from the previous dedicated implementation. It retains `origin=deterministic_shortcut` so a zero-model action is independently traceable and excluded from learned strategy attribution.

**Transcript invariant:** a model-free native file action does not have a preceding assistant `tool_calls` frame. The common dispatcher therefore does not persist a bare `role=tool` chat message for this origin; it *does* persist full execution-operation receipts, emits live tool events, and performs normal approval/effect verification. Ordinary model-issued and ProgramIR tool messages remain unchanged. This avoids corrupting a later provider replay while preserving recovery evidence.

## Indexed effect history without dropping old failures

Previously, every mutating tool queried and deserialized the session's *entire* lifetime `execution_operation` history. Successful read-back of a resource did the same again when closing unresolved effect states. Reused long sessions therefore paid growing SQLite JSON decode costs even for one unrelated file or API target.

`Store::related_effect_operations(session, resource, effect)` uses two partial composite expression indexes and a de-duplicated `UNION ALL`:

- same-session **resourceKey** records: preserves the latest version and *all* earlier pending resource receipts;
- same-session **effectId** records: preserves the exact prior non-idempotent request, even if its resource was recorded differently.

The query returns the same newest-first order as the old method and excludes matching rows from the second branch when already selected. `effect::prepare`, `effect::unresolved` and `effect::mark_inspected` retain their existing decision logic. There is **no recent-N cap**, no cache eviction and no new background worker. Legacy `Store::session_operations` remains available for callers needing full history.

The indexes are created with `IF NOT EXISTS` during normal `Store::open`, maintaining the current JSON record schema and historical task records. SQLite failures continue to propagate rather than being treated as no pending effect.

## Regression coverage

- `src/engine/chat/runtime/deterministic.rs`: native read and conditional write retain verified receipts; a read-only child cannot mutate; model-free tool actions do not persist orphan provider chat messages.
- `src/db.rs`: 320 unrelated operations do not enter the related-history result; identical resource/effect records are returned once; other sessions are excluded; unmatched resource and effect paths are included; `effect::prepare` and `effect::unresolved` match historical full-session decisions.
- A SQLite query planner fixture checks both partial expression indexes are used. The same SQL shape was manually exercised with SQLite before commit; this is **not** a Rust test pass or a product throughput benchmark.

## 2026 comparative design context

- DeepSeek Harness's 2026 tool executor separates tool selection from an owned execution/admission mechanism: https://github.com/deepseek-ai/deepseek-harness
- OpenHands's 2026 software-agent SDK retains first-class event provenance separately from reconstructed conversation views: https://github.com/OpenHands/software-agent-sdk
- The existing sub2 ProgramIR candidate trial and the model-free replacement now both use the same native tool receipt and cancellation ownership, rather than independent permission or history systems.

No dependency, menu, frontend, public SDK or database record format has been changed. All work is on `branch2` and does not merge into `main`.

## Verification limits

- Rust compiler / Cargo: unavailable in this workspace. Newly added Rust tests have **not** been executed here.
- GitHub Actions / release / frontend / macOS / Windows: **not run** in this session.
- Measured CPU, SQLite decode time, latency, completion rate and token/cost deltas: **not measured**. The source-level improvement is an index-constrained history query and the elimination of one duplicate executor, not a claimed runtime percentage.
