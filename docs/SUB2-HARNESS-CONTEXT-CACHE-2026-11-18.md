# Sub2 — Context reuse, lazy skills and measurable cache behavior

2026-11-18 | Branch: `dominion-sub2` only.

## Baseline evidence, not a claim about today's runtime

Repository fixture `benchmark/ROOM-100-TURN-LUNA-2026-11-09.json` reports a 9-agent, 100-owner-turn experiment (older `dominion-sub` branch, Codex Luna). It recorded **2,630,782 prompt tokens**, **477,568 provider-cached prompt tokens** (18.15%), **2,153,214 non-cached prompt tokens**, and **621 `history_budget` rebases among 721 total rebases**. Recall at turns 50, 60, 75 and 100 was **0/9**. The observation applies to that old experimental route; it cannot establish the current branch's cache-hit rate or current recall.

`benchmark/CODEX-WEBSOCKET-INCREMENTAL-2026-11-08.json` recorded 76.9% less third-turn transcript payload for a 9-person Codex WS experiment, while explicitly noting provider-side cache variability and no controlled billing-cost conclusion.

The owner's later 18:17 sub2 three-turn benchmark used **81,311 total room tokens** and produced 30 messages. Exact cache-hit measurements are absent.

## Observed code-level causes

- `continuity::project` was limiting **accumulated committed WS history** to the room's `contextBudgetTokens` (default 2,000), which is already the budget used to select **each fresh room context**. Once a few valid older frames filled that total, the projection became `history_budget` and reset `workingView.frames=fresh`. For a WS chain this throws away usable prior history earlier than necessary. In an ordinary HTTP model, expanding that limit would be wasteful, since the whole history is re-sent on each call.
- `context::context_block_with_manifest` previously included up to 700 **instruction characters per enabled skill** in every request, even when none of those skills applied. The group-context evidence selector also took whole skill instructions as query-retrieval material.
- There was no native `skill_read` tool to fetch the full text after the model chose a skill from a small index. An index alone without a read path would lose functionality.
- Rebase diagnostics did not report separate **fresh selected history** versus **retained continuation history** estimates, making new-prefix cost difficult to attribute.

## Changes, with preserved guarantees

### Codex-only bounded continuation horizon

`continuity_history_allowance` now separates the *fresh room context* budget from the incremental WebSocket working-view retention horizon. Only a `provider.kind=="codex"` session with WS enabled gets an effective carried-over allowance `max(roomBudget,min(4 * roomBudget,12000))` tokens, still bounded by the real provider's declared context limit minus reserved output and fixed prompt. The base room selector's current-frame `contextBudgetTokens` is **unchanged**.

**HTTP, Cloudflare, Ollama, other local providers and Codex with `DOMINION_CODEX_WS=off` keep the existing budget.** They cannot benefit from a server-held chain and must not pay for a larger replay payload.

Prefix contract revisions, changed source content, changed evidence/authority, stale provider cursors and physical context limits still force rebase. No unsafe prefix skip or fabricated remembered transcript is introduced. Existing persisted transcript and owner-state records are untouched.

The `workingView` metadata now records `historyAllowanceTokens`, `freshHistoryEstimatedTokens`, `retainedHistoryEstimatedTokens`, `reusedFrames`, `appendedFrames`, `epoch`, and `rebaseReason`. These are **harness estimates**, not billable tokens or provider cache hits.

### Progressive skill loading

Both normal-chat memory and 9-person room evidence now provide enabled, visible skills as **stable ID + name + short description** rather than repeating their full instructions. The normal-chat index sorts by name and ID, caps listing at 12, and limits descriptions to 120 characters. Existing memory visibility/partition checks still apply.

The native **`skill_read({skillId})`** tool returns full instructions (up to 12,000 characters with truncation flag) only after a participant selects a listed skill. It rejects unauthorized, disabled, missing and draft skills. The tool is read-only, works in parallel read-only agents, and is forbidden by the UI when `skillsEnabled=false`. The tool schema is advertised only when visible approved skills exist and survives native tool-catalog selection.

When tool execution is disabled for a turn, the compiler keeps bounded skill instructions in context instead of advertising an unusable lazy reader. This preserves guidance without undermining owner capability settings.

### Exact billing and no false savings

The existing `turn_body` usage path already records per-request:
- `providerPromptTokens`
- `providerCachedPromptTokens` (null when omitted by provider)
- `providerNewPromptTokens` = prompt minus cached (null when unknown)
- `providerUsageExact`
- UI/SQLite `cachedPromptTokens`, `uncachedPromptTokens`, `completionTokens`

These are the metrics that should be paired with new `workingView` estimates. Neither a smaller WS transcript payload nor a cache affinity header proves a real monetary discount. A model without reported cache tokens cannot be scored as a cache hit.

## Regression tests

- `continuity::tests::only_persistent_ws_enlarges_bounded_history_not_http_replay` checks Codex WS, explicit WS off, HTTP, Ollama, physical model-context cap.
- `context::tests::skill_manifest_is_compact_sorted_and_never_embeds_full_instructions` checks index order, bounded context and hidden full instructions.
- `tools::tests::skill_read_requires_enabled_visible_skill_and_returns_original_body` checks full-text recovery and permission isolation.
- `dominion-agent-kernel::tests::scoped_skill_read_is_not_an_unknown_remote_effect` preserves safe retry admission.
- Existing 9-agent owner-state, context provenance and WS-prefix tests remain in place.

No 100-turn live Codex rerun or local 27B model speed test has been performed in this change. The expected reduction in `history_budget` rebase events is a **design hypothesis** pending same-model, same-budget replay, not a claimed measured win. The objective is to keep recall quality and safety fixed while reducing **uncached** tokens, time-to-first-token and unnecessary rebase events.
