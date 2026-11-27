# Sub2 — nine-agent source-addressed incremental group context
2026-11-18 | branch: `branch2` only

## Owner objective

**Keep every fact the model must remember recoverable, but reduce newly processed tokens per model invocation.** This is a problem of nine-agent group discussion, not merely normal single-agent chat.

This implementation deliberately separates four quantities:

1. **Durable knowledge:** Original room messages, owner corrections, decided facts, tool receipts, and memory entries remain in SQLite.
2. **Selected working view:** The relevant recent peer speech plus short, attributed references to earlier peers; selected facts are views, not new authoritative assertions.
3. **Provider continuation:** When source IDs are already part of a participant's committed prefix, do not append the same source again as an excerpt. Codex WS may reuse earlier frames; HTTP still sends an explicit working view.
4. **Actual cost:** Cached, uncached and total provider input tokens are recorded only from provider usage when supplied. All Rust-side token estimates are **not billed-token counts**.

## Source-audited defect

`chat/room_context.rs::compile` selected only the most recent **3** speaker contributions for casual owner messages, or **6** for complex owner messages. Earlier members' answers from the *same owner request* were neither mandatory nor guaranteed to appear in the current compiled view. This made the model's local working memory behave as if some room participants were silent. Replaying every speaker's entire answer to every later agent would fix visibility at unacceptable fresh-token cost.

The previous peer brief compressed only members **already in the selected window**, so it did not cover the earlier omitted peers. A room-shared read model already protected roster and owner decisions but did not contain those peers' source text. The model needed a recoverable source index, not another generative summary.

## Implementation

### Exact source index, not a new memory record

`chat/room_context.rs::peer_index` iterates the **latest committed reply per agent for the current owner source**, excludes messages still supplied verbatim, and creates one compact frame per older member:

`room-peer-index:<original message ID>` with the participant name, exact source ID, original character count, and an excerpt bounded to 84 leading + 24 trailing characters for long speech. Short speech (up to 120 characters) remains entirely verbatim. Each excerpt is explicitly **quoted peer data, never a new owner instruction or confirmed fact**. The newest three speaker contributions remain selected, with the preexisting bounded peer brief reducing redundant long prose. Explicit verbatim/quote requests preserve the existing full-source path rather than silently presenting clipped speech as a full quote.

There is **no background LLM summarization call** or inferred claim. The original `message` rows, explicit owner state, corrections, tool events and private reasoning storage remain untouched. A later request can target the original by ID.

Unlike one mutable combined summary, the indexed frames each have **stable source identifiers**. As new speakers arrive, unchanged frames can remain at the same positions in a participant's previous transcript prefix. In `chat/continuity.rs::append_unseen_frames`, a short index is not appended when the original complete peer response already exists in the carried-forward working view. A source change still invalidates its previously selected frame; true rebases retain the index.

### Read the exact original, on demand

The native `room_message_read({messageId,offset?})` is a **bounded read-only tool**, not a new agent or vector store. It accepts only a committed assistant message from the same session's room and a member of that room's roster. It refuses foreign sessions, unknown IDs, other member-like records, owner user messages and tool-call transcripts. It returns only the assistant's committed **content** (not reasoning or credentials), in Unicode-character pages of at most 6,000, with `nextOffset` and `hasMore`. Original content remains in DB; the tool performs no write or external network operation.

The tool schema is only advertised to enabled, relevant large-room turns when the user asks to recall, quote, compare source statements or summarize the members. Casual greetings and non-room calls do not pay for this schema; tool permissions and the existing native execution/approval gateway remain authoritative. Tools-disabled rooms still receive short attributable excerpts but are never promised an unavailable reader.

### Group-native accounting

`room_context::compile.accounting` now separates `peerIndexedMessageIds`, `peerIndexEstimatedTokens`, `peerIndexOriginalTokens`, `peerIndexEstimatedTokensSaved` and existing `peerBriefEstimatedTokensSaved`. `wire_profile` attributes `peerSourceIndex` and `peerBrief` independently. `continuity::project.workingView` additionally emits `newHistoryEstimatedTokens` as a local *projected* incremental-context metric.

These are deliberately distinct from provider `cachedPromptTokens`, `uncachedPromptTokens` and `providerUsageExact` stored on an execution. A shorter native prompt does **not** prove a lower invoice; a larger cached prefix can sometimes cost less than repeatedly rewritten short prompts.

## Regression map

| Test | Property |
| --- | --- |
| `room_context::tests::nine_agents_retain_source_addressed_peer_coverage_without_nine_full_replays` | All five early source IDs reachable, last speaker exact, old prose not repeated, full original durable, index smaller than full replay |
| `room_context::tests::nine_speaker_long_prose_uses_profitable_peer_brief_and_preserves_sources` | Existing latest 3/brief behavior remains compatible |
| `room_context::tests::explicit_exact_quote_request_never_compacts_peer_speech` | Explicit literal quotation does not turn excerpts into assumed originals |
| `tools::tests::room_source_reader_preserves_full_peer_history_and_private_scope` | UTF-8 pagination, original completeness, room isolation, owner/private reasoning denial |
| `continuity::tests::indexed_peer_never_duplicates_a_full_reply_in_committed_prefix` | Tests append-only helper itself; full previous source prevents fresh-index repeat, rebase restores index |
| `dominion-agent-kernel::tests::scoped_room_message_read_never_requires_non_idempotent_replay` | Native read is not an uncertain external effect |

Development CI invokes the touched tests plus existing nine-speaker and owner-state coverage. **This design should not be scored as a 100-turn recall fix until a controlled end-to-end replay verifies it.**

## Acceptance measurement for judge

Compare 9 participants × the same owner prompts × 3 turns (and 100 turns for memory longevity), with identical models, tool rights, source message history, output limits and provider accounts:

- Valid replies and independently verified task/file success.
- Questions about **first owner instruction, later correction, early peer statement, and last agent**; score exact source correctness, not confident paraphrase.
- Newly **uncached** provider tokens per member/turn, cached input tokens, provider full input tokens and actual latency.
- `source_changed`, `contract_changed`, `history_budget` rebase counts; `workingView.newHistoryEstimatedTokens`.
- The share of early speakers still recoverable from exact source IDs without extra model cycles, and how often `room_message_read` is actually called.

The owner-supplied 2026-11-18 baseline for sub2 was **81,311 total room tokens across 3 owner turns**, but the raw ZIP is not present in this session. A historical 100-turn 2026-11-09 run reported 18.15% cache hits, 621 history-budget rebases and 0/9 late recall. **Neither result should be repurposed as this code revision's measured improvement.**

## 2026 research context

- OpenAI API prompt caching guide (2026 model/API): stable identical beginning, explicit breakpoints where supported: https://developers.openai.com/api/docs/guides/prompt-caching
- Anthropic 2026 prompt cache and tool-context guidance: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- *Context Has a Price: Adding and Removing Context for LLM Coding Agents under Prefix Caching*, posted 2026-11-17: https://arxiv.org/abs/2611.22238
- *Don't Break the Cache: An Evaluation of Prompt Caching for Long-Horizon Agentic Tasks*, posted 2026-01-09: https://arxiv.org/abs/2601.06007

The above sources motivate **provider-aware** measurement, not an invented guarantee. The source of truth for behavior remains the Rust implementation and CI results on this branch.
