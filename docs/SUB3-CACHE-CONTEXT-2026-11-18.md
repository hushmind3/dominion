# Sub3 — Context reuse, relevance-gated Skills, and cost evidence (2026-11-18)

## Source-path diagnosis

The historical [2026-11-09 room benchmark](../benchmark/ROOM-100-TURN-LUNA-2026-11-09.json) showed 621 `history_budget` rebases among 721 rebases and 477,568 cached input tokens out of 2,630,782 prompt tokens. **This older benchmark is not a measurement of the current sub3 HEAD.** Since then, `src/engine/chat/continuity.rs` gained durable per-participant working views, snapshot rotation and provider affinity. `src/engine/chat/codex_ws.rs` already supports incremental transport when the connection and input-prefix chain survive. The SDK native agent already applies `skill_discovery::initial_context`; chat still had a separate eager skill path.

Observed redundant context in `src/engine/chat/context.rs::context_block_with_manifest`:
- iterated over the full stored Skills table, built digest records for all visible candidates, then inserted the **first five** Skill instruction snippets (up to 700 characters each) regardless of task relevance;
- used different enablement and scope rules from `skill_search` / `skill_read`, which could make a chat prompt expose a Skill its retrieval tools would not authorize;
- produced changing dependency metadata for irrelevant Skills and wasted model input on procedures unrelated to the request.

## Changes on `dominion-sub3` only

1. `src/engine/skill_discovery.rs::select` is now reused as a crate-local, Rust-owned selection contract by chat and native paths. It applies the same scope, enabled/status checks and task relevance; the Skill search/read tools still perform independent permission checks.
2. `src/engine/chat/context.rs` preloads up to **two relevant** Skill instructions, each clipped at 1,000 Unicode characters, rather than the first five arbitrary stored records. The manifest tracks only Skills actually surfaced. Other eligible Skills remain discoverable through `skill_search` and `skill_read` when tools are enabled. User memory, conversation history, owner directives, persisted Skill records and permissions are unchanged. The prompt labels Skill text lower-trust, not new owner instructions.
3. `src/engine/chat/continuity.rs::project` now exposes `estimatedReusedHistoryTokens` and `estimatedNewHistoryTokens` for each model-visible working-view update, with explicit `historyEstimateKind`. A budget/contract rebase records zero reused frames. These numbers are serialized-text estimates, **not** provider KV-cache hits, billed new tokens or actual savings. Provider usage/cached-token fields remain the authority for monetary comparisons.

## Regressions added, validation boundary

- `context::tests::chat_skills_are_relevance_selected_not_eagerly_injected_from_db_order`: unrelated Skills stored earlier are not preloaded, a relevant later Skill is, disabled Skills are omitted, and an unrelated task does not inherit the selected procedure.
- `continuity::tests::committed_working_view_keeps_wire_prefix_when_selection_slides_and_routes_change`: existing projection/route/correctness fixture now asserts reuse/new split and zero reused history following a contract rebase.
- Source edits are committed with `[skip ci]` to preserve the owner's CI-later cleanup preference. **Rust compiler and tests were unavailable in this editing runtime; neither test has been executed here.** There is no claim of changed cache-hit rate, 100-turn recall, or lower model billing.

Focused checks in a Rust-enabled environment:

```bash
cargo test --bin dominion-rust engine::chat::context::tests::chat_skills_are_relevance_selected_not_eagerly_injected_from_db_order
cargo test --bin dominion-rust engine::chat::continuity::tests::committed_working_view_keeps_wire_prefix_when_selection_slides_and_routes_change
cargo test --bin dominion-rust engine::skill_discovery::tests
```

To measure real gains, replay equal tasks/models/provider accounts/settings and compare provider-reported `promptTokens`, `cachedPromptTokens`, billable uncached input, output tokens, recovery/recall success and wall time; avoid comparing projected character estimates with billed tokens. `previous_response_id` can reduce client-side replay and latency but does **not** remove prior-context input billing by itself.

## 2026 references

- Claude Code cache-prefix design (2026-04-30): https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything
- Anthropic deferred tool loading and cache stability (2026-09-08): https://claude.com/blog/reducing-cost-and-improving-performance-with-claude-platform
- Hermes Agent compression/usage anchor: https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/context-compression-and-caching.md
- OpenAI Responses conversation billing: https://developers.openai.com/api/docs/guides/conversation-state
