# Sub2: runtime separation, tool-schema admission and atomic checkpoint merge
**Date:** 2026-11-16  
**Scope:** only `dominion-sub2`, without main/other branch writes

## Real connected path

```text
User / Home UI / room / TypeScript SDK / native Rust SDK
    → API / SDK submit → SQLite job → native Rust jobs worker
    → chat::runtime::run_turn_body → tool_projection::select*
    → model provider → native tool dispatcher + owner policy/approval
    → durable operation evidence → outcome::{determine_task_outcome,compliance_detail}
    → finalized execution, verified experience → GUI / SDK status
         ↘ JobContext::checkpoint
           → Store::patch_if_all_merge (single connection lock)
           → SQLite current status + cancellation guards
           → fresh videoState fieldwise merge → event bus
```

## Before / after and structural ownership

- Before: `chat/runtime.rs` was **7,290 lines**, including tool inventory ranking, Home/room selection, size trimming, finalization semantic fingerprints, completion compliance and success experience. The main and sub3 branches still carry tool selection inside `runtime.rs` at inspection.
- After: approximately **6,773 runtime lines**; **344 lines** in `chat/tool_projection.rs`; **238 lines** in `chat/outcome.rs`. Public agent, SDK, video, OAuth, room UI, DB schema and provider APIs remain unchanged. This is a responsibility boundary change, not a benchmarked latency or quality gain.
- Tool definitions with empty, missing, whitespace-containing or control-character names are excluded. The former `trim_tool_definitions` could keep its first oversized schema; the new implementation uses serialized bytes and obeys `budget` including the first entry. Type-specific dispatch approval remains in the existing tool dispatcher, not in the projection.
- Before: `JobContext::checkpoint` read a job, merged `videoState` on that stale snapshot and called CAS only against `cancelRequested`. Two simultaneous writes with unchanged cancellation status could lose each other's nested data.
- After: `Store::patch_if_all_merge` checks both `status` and `cancelRequested` and merges the nested keys against the most recent row while holding the same SQLite connection lock used for the update. Queued/running jobs may checkpoint; cancelled/terminal ones cannot. No new table, migration or serialization format.

## Comparator source reading

- OpenHands 2026 [`parallel_executor.py`](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/parallel_executor.py) explicitly separates parallel tool dispatch and resource locks from conversation state; [`local_conversation.py`](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py) owns session state. Dominion continues using its own Rust loop and storage, taking only the responsibility-isolation principle.
- Chinese openJiuwen [A4P preflight](https://github.com/openJiuwen-ai/jiuwenswarm/blob/develop/jiuwenswarm/agents/harness/common/a4p/preflight.py) and [runtime](https://github.com/openJiuwen-ai/jiuwenswarm/blob/develop/jiuwenswarm/agents/harness/common/a4p/runtime.py) separate session-scoped authorization from tool runtime. Dominion's schema projection does **not** grant permissions, preserving the existing owner policy.
- 2026 [11-harness source-code comparison](https://arxiv.org/abs/2609.00006) highlights purpose-built loops and deterministic retrieval; it is background taxonomy, not proof of superior task completion by Dominion.

## Regression routes and evidence quality

1. Existing runtime tool projection tests still call the extracted actual functions; newly added independent module tests reject malformed definitions and verify the first oversized schema is excluded.
2. `db::tests::checkpoint_merges_concurrent_scene_updates_without_lost_progress` forks two threads, checks persisted scenes/prompt/counters and verifies a cancellation blocks the next write.
3. `jobs::video_checkpoint_tests::checkpoint_preserves_scene_provenance_and_rejects_post_cancel_writes` uses a real temporary SQLite store and checks the native `JobContext` path.
4. CI on Windows/macOS/Linux runs type checks, SDK contracts, web bundle, scripted RSI evaluation and `cargo test --release`. The exact post-change SHA and individual CI results determine pass/fail.

**No performance claims without controlled measurements:** no same-task/model pre/post cost or quality delta was measured in this patch. A full refactor of program interpreter state (and the absence of a `product.rs` file in this branch) remains explicitly out of scope. Cancellations of already-dispatched remote side effects retain the existing independent reconciliation policy. These source and contract checks are not a substitute for real model/GPU/YouTube acceptance tests.
