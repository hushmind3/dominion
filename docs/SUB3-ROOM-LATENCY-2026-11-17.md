# SUB3 — room progress, transport reuse and durable job finalization (2026-11-17)

Scope: `dominion-sub3` only. Main branch, GUI design, authentication, existing SQLite records, and public API routes remain unchanged.

## Baseline observed from GitHub Actions

- Commit `0b67abf8c8b0`: focused Rust SDK verification **passed**, focused product acceptance **passed**.
- Full CI on the same commit: Ubuntu and macOS jobs **passed**. Windows built the React frontend, passed 604 Rust unit tests, then failed **2 of 75** E2E tests (`a_room_greeting_reaches_the_whole_discussion_room`, `a_short_greeting_reaches_all_nine_when_the_turn_cap_covers_the_roster`).
- The Windows E2E job checkpoints show successful sequential replies continuing: the 2-person room had persisted 1 response at its 15-second deadline; the 9-person room had persisted 4 responses at its 20-second deadline. This supports *time-budget mismatch for sequential work*; it is not evidence that a participant was skipped.
- No controlled before/after timing was available in that log. Treat performance gain as **unmeasured** until a same-runner comparison is performed.

## Changes

1. `src/state.rs` caches `reqwest::Client` instances by request timeout on the shared `AppState`; cloned handles share HTTP connection pools. `src/engine/chat/runtime.rs` reuses them for each participant instead of constructing a new transport per turn. Individual authorization headers and message context stay request-local; room timeout policy is unchanged. The approach follows the Rust reqwest client reuse recommendation: https://docs.rs/reqwest/0.12.28/reqwest/struct.Client.html
2. `src/engine/rooms.rs` persists `elapsedMs` for each completed speaker alongside its immutable output and emits `speaker_finished` progress with the elapsed time. Sequential turn order and previous-peer context remain intact.
3. `tests/e2e.rs` replaces **only the two brittle room greeting tests' total 15/20-second deadlines** with a durable-progress watchdog. A new reply extends the observation window; 18 seconds without another recorded speaker fails with the exact job checkpoint. It also rejects failed, cancelled or review-required jobs.
4. `src/engine/jobs.rs` records `job.completed` / `job.failed` only after the terminal state checkpoint is persisted. A concurrent owner cancellation cannot produce a false successful finish audit, and an unsuccessful terminal commit is audited as `job.final_checkpoint_refused`.
5. `src/state.rs` exposes a shared cancellation notification after durable cancellation/epoch updates. `src/engine/chat/runtime.rs::wait_cancelled` registers a waiter before reading the durable state and uses a 500ms fallback only for external-process writes. The prior 50ms SQLite polling path performed up to 20 checks per second per active turn; the normal idle fallback now performs up to 2 checks per second and local cancellation is signalled immediately. These are inspection-based rate ceilings, not measured CPU benchmarks.

## Deterministic regression coverage

- Client reuse within the same timeout and across `AppState` clones, with distinct timeout policies.
- Nine-speaker room output contains per-speaker latency observations, enabling actual slowdown diagnosis without relying on UI clocks.
- The 2- and 9-speaker real-process E2E tests enforce complete roster coverage, no fabricated owner messages, and a **no-progress** rather than arbitrary total-runtime deadline.
- Cancelled job terminal updates are rejected and previously persisted cancellation remains authoritative.
- Local cancellation notify and external durable cancellation fallback both end waiting turns without continued model generation.

## Verification boundary

These changes were committed to `dominion-sub3` with CI deferred by `[skip ci]`. **Do not claim post-change CI success or measured throughput improvements.** For an eventual focused check use the full Rust crate tests plus the two named Windows `--test e2e` cases, and compare the newly recorded `elapsedMs` values before deciding whether deeper context/SQLite optimization is warranted.
