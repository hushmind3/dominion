# Sub3 durable event and per-agent room-cursor contract — 2026-11-20

Branch: `dominion-sub3` only. This document implements priorities P0/P1 of the supplied architecture review against the **actual sub3 source tree**. The review's `dominion-core`, `dominion-algorithms`, `TurnStrategyPort`, and `src/addons` describe a different main-line architecture; sub3 already has `dominion-agent-runtime` (contracts and admission), `dominion-agent-algorithms` (policy and orchestration), and the thin Rust SDK facade. No duplicate crate, external gateway, UI navigation, or second model loop was added.

## P0: Durable event ordering

`src/events.rs` no longer invents `seq` from per-process atomics. `Store::append_event_next` acquires a SQLite IMMEDIATE transaction, initializes the persistent `event_clock` from the existing `event_log` during upgrade, increments the sequence, inserts the event, applies bounded retention, and commits before broadcasting. Competing SQLite connections cannot overwrite each other. A failed transaction is never broadcast as a successful event. `try_emit` is available to callers that need fail-closed handling, while existing `emit` remains source compatible and logs failure. Retention does not reset the clock.

This repairs an actual cross-process replay bug without changing the API's existing event envelope. It does **not** claim a new background cross-process broadcast transport.

## P1: Committed participant read positions

`features/collaboration/room_cursor.rs` computes a deterministic, sequential read window for each (session, agent) binding, pinned to **committed message IDs**, not provider-dependent context offsets. Only owner messages and finished agent messages are eligible: tool fragments, arbitrary system frames and foreign sessions are excluded. At most 4 unread records and 4,800 characters are promoted to required context per turn. Backlog is retained, not skipped; the current owner request is independently mandatory.

`conversation/chat/room_context.rs` incorporates this window into its existing BM25/context budget projection without adding another chat loop. It exposes `readCursorPreviousMessageId`, `readThroughMessageId`, `roomUnreadPinned`, `roomUnreadRemaining` and `roomUnreadOversized` in execution accounting. `conversation/chat/runtime/turn.rs` commits the final room answer and its `room_read_cursor` record in **one SQLite transaction**, rejecting stale anchors, cross-session bindings, non-owner source messages and rewinds. Failed generations do not advance the read position.

Oversized messages remain unacknowledged rather than being silently skipped; the current reply remains usable. Historical transcript data is never deleted or rewritten and remains eligible for relevance retrieval. This is a monotonic delivery cursor for existing Room Chat, **not yet** a separate Task Team orchestrator, global multi-agent consensus, or a guarantee that every item in a huge backlog fits in one model context.

## Verification

- `cargo test --bin dominion-rust events::tests` — independent SQLite connections, parallel writes, restart and retention
- `cargo test --bin dominion-rust features::collaboration::room_cursor::tests` — contiguous delivery, isolation, missing anchors and oversized backlog
- `cargo test --bin dominion-rust conversation::chat::room_context::tests` — 100-message history, pinned deltas and no premature acknowledgment
- `cargo test --bin dominion-rust storage::sqlite::tests` — atomic answer/cursor commits and no rewind
- Existing sub3 Rust SDK + product frontend workflows, not full release CI.

References: Qwen Code's October 5, 2026 session multi-agent design (message-ID-anchored per-agent read cursor), Codex Rust app-server event architecture and Qwen's managed task event contracts. The implementation reuses Dominion's existing SQLite and session ownership; it does not copy their product protocol.
