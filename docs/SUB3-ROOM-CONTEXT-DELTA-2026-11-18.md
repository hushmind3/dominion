# DOMINION sub3 — nine-speaker group context reuse (2026-11-18)

Scope: `dominion-sub3` only. The objective is **preserve relevant owner, self, peer and source evidence while reducing provider-uncached NEW input tokens per group model call**. This does not target the separate autonomous native SDK job loop.

## Code-path diagnosis

The active group call is `src/engine/chat/runtime/turn.rs` -> `room_context::compile` -> `continuity::project` -> provider. The compiler already carries independently pinned current owner text, previous owner turn, prior self answer, a small recent-peer selection, and retrieved room knowledge. Original messages, owner corrections, and source references remain in SQLite. `room_state::conversation_context` supplies a roster-derived shared status snapshot.

Before this change, that status used a `room-shared:{sourceMessageId}` frame and displayed current/previous *opaque owner IDs* in its text. Even when **all nine members answered in both successive rounds and there was no new tool work**, every owner turn generated a new status frame. `continuity::project` then treated the prior frame as stale and forced `room_snapshot_rotated`, losing a potentially reusable provider prefix. This is distinct from intentional invalidation when the **content** of decisions or tool evidence actually changes.

## Implemented behavior

1. `src/engine/room_state.rs`: preserve the complete `sourceMessageId`, `previousOwnerMessageId`, roster, response receipts and provenance in the persisted snapshot, but remove *opaque per-turn owner IDs* from the **model-visible shared status**. The bounded status carries the previous response count, names of truly missing speakers and host-observed tool work. A new `textDigest` hashes those actual model-visible bytes; the original cursor-aware `digest` is retained for audit.
2. `src/engine/chat/room_context.rs`: use `room-shared:{textDigest}` as the frame key; old persisted snapshots without `textDigest` compute the same key from stored text. The current owner message is still selected verbatim as its own distinct frame, and `owner-state` (corrections/decisions), personal memories and retrieved peer evidence are unchanged. The final context ledger records both `sharedStateDigest` (cursor/provenance) and `sharedTextDigest` (what the model saw).
3. `src/engine/chat/continuity.rs`: the existing exact content comparison and dependency guards remain authoritative. An **unchanged** status is appended only once to the participant's committed working view; new owner text is appended as new evidence. A **changed** response/work status gives a different key and forces bounded rebase, removing stale prior status. A changed owner decision, persona, authority, model tool schema or fresh evidence continues to trigger the existing safety invalidation paths. If history exceeds the bound, `history_budget` still rebases; this optimization does **not** disable the budget guard.

### Two independent digests

- `sharedStateDigest` is source/provenance-specific and changes when the owner-turn cursor changes, even if the visible response status is identical.
- `sharedTextDigest` is byte-identity of the visible compact status. The cached `room-shared:` frame uses this one; when true status changes, it no longer matches. If the digest were to collide, the existing `source_changed` check compares the actual message text and forces a rebase.
- Both are derived from host records; neither grants the model authority, fabricates a reply, or modifies a durable message.

## Source-level deep audit and additional repairs (same branch)

Further review compared actual implementation in `main`, `dominion-sub2` and the group/compiler/transport/usage code path. The independent `dominion-sub4` contents path was not available to this GitHub connection and is not claimed as inspected. Published 2026 research and implementation references include [TokenPilot](https://aclanthology.org/2026.findings-emnlp.899/), [Cloudflare Think](https://github.com/cloudflare/agents/blob/main/docs/think/index.md), [Hermes context compressor](https://github.com/NousResearch/hermes-agent/blob/main/agent/context_compressor.py), [Claude Code prompt caching (2026-04-30)](https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything) and [Context Has a Price (2026-11-17)](https://arxiv.org/abs/2611.22238). Their implementations and tradeoffs informed these changes; this branch does not claim their measured gains or copy their full pipelines.

- **Monotonic, source-verified owner facts** (`src/engine/chat/continuity.rs`). A newly selected confirmed fact becomes a new append-only frame rather than rewriting a previous fact block. The full stored ledger is checked for genuine edits, revocations and corrected facts; these *must* rebase and exclude obsolete text. Legacy working views without owner-state revision proof rebase once.
- **Separate retrieval selection versus retained cache epoch**. `contextBudgetTokens` controls the per-request evidence selector. `workingViewBudgetTokens` optionally controls the accumulated cache epoch. Absent explicit owner configuration, a remote route keeps up to four selections (bounded 2k–12k and by provider context/output limits), instead of rotating at the 2k selection budget after every owner turn. A local route without verified cache telemetry keeps the smaller selection budget by default, avoiding reprocessing a larger *uncached* prefix. Non-room chats retain the historical 40k allowance.
- **Source-scoped owner-fact carry on budget rotation only**. The optional retained lines must be verified local-owner statements from the *same room*, available by the *current owner message cursor*, and checked against an existing prior owner-state proof. No scanning all room ledgers; no future owner correction, foreign room data, tool outputs or unverified model claim can be promoted. Other rebase reasons drop stale prior frames entirely.
- **Out-of-order owner turns**: a delayed group request whose source precedes the participant's last committed source forces `owner_cursor_rewound` before any cached future-owner content could be projected. If a new source has no authorized owner-state block, cached owner-state material is invalidated.
- **Expiry-aware runtime metadata**: a date change invalidates a prior `runtime-date:` frame instead of appending contradictory dates; unchanged same-day metadata remains reusable.
- **Stable group tool schema order** (`src/engine/chat/runtime/selection.rs`): score by relevance to choose up to eight authorized definitions, then restore the canonical existing host-catalog order. Relevance score changes alone no longer change the API tool schema prefix; actual membership/schema changes still invalidate its contract.
- **Per-call measured input** (`src/engine/chat/runtime/turn.rs`): `modelCallComponents` stores `providerUncachedInputTokens = providerPromptTokens - providerCacheReadTokens` *only* when the provider reports both. Missing cache usage remains `null`, never a fabricated zero. Separately expose `projectedReusedHistoryTokensEstimate` and `projectedNewHistoryTokensEstimate` labelled as host estimates.

**Tradeoff:** no arbitrary in-place context trimming is attempted. The Nov 17, 2026 paper demonstrates that dropping old observations can *increase* paid uncached-token and follow-up-call costs under prefix caching. A larger prefix also increases local-model processing if caching is absent; hence the route-sensitive default and user override.

## Source-level regression coverage

- `room_state::tests::identical_roster_outcomes_reuse_display_digest_but_keep_distinct_provenance`: exact persisted cursor differences, identical completed outcomes, newly missing speaker invalidation.
- `room_context::tests::nine_speaker_three_turns_keep_owner_and_self_while_bounding_repeated_transcripts`: adds equality of two successive **nine-speaker all-answered** content keys while continuing to require owner and previous-self content.
- `continuity::tests::identical_room_status_reuses_prefix_but_changed_status_rebases_without_erasing_memory`: models two owner turns with identical shared outcomes and a third with changed outcomes; asserts exact prefix reuse, no duplicate shared state, zero stale state leakage, owner decision and self-answer retention, and the distinction between reused-history estimates and actual cache tokens.
- Existing room rotation tests remain unchanged and continue to require rebase when the actual frame content changes.

Additional authored tests cover `nine_speaker_hundred_turn_projection_keeps_memory_and_reuses_prefixes`, `budget_carry_rejects_foreign_and_future_room_facts`, `delayed_owner_turn_cannot_reuse_facts_from_a_future_source`, `runtime_date_changes_remove_stale_date_without_losing_owner_decision`, `local_without_reported_cache_uses_small_default_epoch_but_respects_owner_setting`, `owner_fact_selection_reuses_prefix_and_appends_missing_facts_until_real_revision`, and `room_tool_catalog_order_stays_stable_across_relevance_changes`.

The same-model/provider **uncached-token savings have not been measured yet**. This is source-level behavior plus authored tests. The source editing runtime does not contain a usable local Rust toolchain. The older 2026-11-09 100-owner-turn nine-agent benchmark (18.15% cache ratio, 621 budget rebases) was from a different branch state and must not be presented as the new result.

## How to evaluate (same 9 agents, 100 owner turns)

Record per-model-call `totalPromptTokens`, `cachedPromptTokens`, `newPromptTokens = total - cached` **only when the provider supplies real cache telemetry**; report unknown separately instead of zero. Track `rebaseReason` by category, `sharedTextDigest`, `estimatedReusedHistoryTokens` and `estimatedNewHistoryTokens` **as host estimates**, not billed savings. Verify last owner correction, previous self, named peer contribution, long-distance recall and the absence of a future owner message in each speaker's context. Compare time, cost, memory recall and task completion with the same model, settings, tool schema, turn order and budget before claiming a win.

Expected *direction* of improvement: fewer content-equivalent room snapshot rotations and fewer redundant status tokens; no guaranteed numerical savings, because prompt-cache TTLs, provider behavior, changed decisions, tool catalog mutations and the still-necessary history-budget rebases vary.

## 2026 research comparison

- [TokenPilot, June 2026](https://arxiv.org/abs/2606.17016): cache-aware context pruning; arbitrary shifting-prefix evictions can eliminate useful reuse. DOMINION's change is limited to semantic identity of its **derived group status**, not wholesale model history deletion.
- [KVCMAS, September 2026](https://arxiv.org/abs/2609.34060): agent-specific prefixes can defeat shared KV-cache reuse. DOMINION does **not** implement its server-side KV-cache correction or claim cross-model cache sharing.
- [Anthropic prompt caching docs](https://platform.claude.com/docs/en/build-with-claude/prompt-caching): static-prefix / varying-suffix separation. Provider cache reads must be measured; equality of host digests alone is not a billable cache hit.
- [OpenAI prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching): provider-side input reuse and cache keys are distinct from client-side wire deltas.

No full CI, release build, live 9-agent benchmark or production deployment was executed as part of the source edit. All changed files are on `dominion-sub3`.