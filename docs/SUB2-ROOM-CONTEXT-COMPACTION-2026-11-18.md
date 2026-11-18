# Sub2: source-grounded group context compaction, 2026-11-18

## Why this changes native execution

In the supplied 9-person, 3-turn comparison, sub2 was reported at 81,079 total tokens vs main 74,045, with 6,916 of the 7,034-token difference attributed to **input**. The reported third-turn ratio grew faster on sub2 (31,295 vs first-turn 22,365). These are **user-supplied live run observations**, not model-provider measurements reproduced by this document.

The owner-directed room path is `rooms::run_locked` → `run_room_quiet` → `chat::run_room_turn` → `room_context::compile` → `context::render_history` → native chat provider. Original SQLite room messages, tool receipts and execution IDs remain canonical.

## Changes

1. **Do not replay previous private reasoning to room participants** (`src/engine/chat/context.rs`): prior assistant answers and valid assistant-tool/tool-result pairs remain model-visible, while `reasoning` and `metadata.providerReasoning` from completed earlier turns stay in the original DB row and are omitted from *new* room prompts. Non-room chat/reflection behavior is unchanged. No loss of committed answers.
2. **Profitability-gated, source-attributed peer excerpt ledger** (`src/engine/chat/room_context.rs`): for 5+ participants with multiple expensive recent peer prose messages, the prior policy injected up to 3 (casual) or 6 (deep) full raw peer turns as mandatory context for each next speaker. The new selector may instead include a bounded `room-peer-brief` with each source peer's real name and exact head/tail excerpts. Every original source message is retained under its stable message ID in SQLite, with references in `inputContext.peerBriefMessageIds`.
3. **Protect essential context**: no compression of the current owner's message, durable owner corrections, the newest peer's committed message, tool-call/response exchanges, code-formatted content, explicit decisions/approvals/corrections, URLs, or turns explicitly requesting original quotations or exact wording. Agent-specific memories still follow their own visibility scope. Missing sections are labelled as omitted, never inferred.
4. **Payback condition**: the compact ledger is admitted only if the projected input cost (including the additional ledger frame) saves >100 estimated tokens. `peerBriefEstimatedTokensSaved` stores the estimated delta, **not an actual provider-billed token saving**. There is no extra summarizer/teacher model invocation. Full per-round provider-wire breakdown and actual provider token usage remain distinct existing execution fields.
5. **Group coverage**: the existing 9-agent flow still routes each of 9 agents on every user turn. A new mocked native-stream test runs 3 sequential owner turns, verifies 27 actual provider calls and 30 persisted messages, and checks model-call IDs and non-zero recorded usage. A separate test shows full, non-truncated raw content is still available when the owner requests verbatim citation.

## Why this policy

2026 context research supports selective persistent state rather than indiscriminate message replay, but some summarizers distort source authority. In a multi-agent product the decisive constraint is to preserve the owner's exact directives, individual agent identity, and verification receipts while avoiding repeated stale private reasoning. We chose an **engine-local deterministic projection** with exact provenance, no independent summarizer, and a before/after cost gate rather than a new background summarization loop.

Related 2026 publications:

- [Shared Selective Persistent Memory for Agentic LLM Systems](https://arxiv.org/abs/2607.09493)
- [ContextWeaver: Selective and Dependency-Structured Memory Construction](https://arxiv.org/abs/2604.23069)
- [COMPASS: Enhancing Agent Long-Horizon Reasoning with Evolving Context](https://pmc.ncbi.nlm.nih.gov/articles/PMC13682274/)
- [Folio: Recoverable Multi-Resolution Context Folding](https://arxiv.org/abs/2611.16627), 12 November 2026.

The implementation takes design inspiration from these results; it does not reproduce a published benchmark and must not claim their performance gains.

## Tests and limits

Targeted Rust CI now includes:

- `room_context::tests::group_replay_keeps_committed_answer_but_not_prior_private_reasoning`
- `room_context::tests::nine_speaker_long_prose_uses_profitable_peer_brief_and_preserves_sources`
- `room_context::tests::explicit_exact_quote_request_never_compacts_peer_speech`
- `rooms::tests::nine_speakers_complete_three_owner_turns_without_false_completion`

The prior group cancel-epoch and tool-budget suites also run as configured. `wire_profile` is a deterministic estimate of a native pre-provider-adapter envelope, not an exact remote model tokenizer. **Actual live 9×3 same-model comparison and answer-quality equivalence are still required** before claiming that sub2 beat the 81,079-token baseline in the user's ZIP.

Only `dominion-sub2` was changed; no main/sub3/sub4 merges, frontend visual edits, or new persistent database collection.
