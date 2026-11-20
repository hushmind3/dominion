# Independent owner-task kernel on Sub2 (2026-11-17)

The `dominion-sub2` branch separates the **model-independent task decision core** as a buildable Rust library at `crates/dominion-agent-kernel`. It is compiled into and **used by** the existing `src/engine/autonomy.rs` durable owner job worker through the `src/engine/agent_kernel.rs` compatibility export. It is not a second executor.

## Responsibilities

- The independent crate owns bounded attempts, focused/conservative repair vs review, replay barriers for tools whose effects cannot be safely assumed, monotonic usage deltas, versioned owner instruction replacement, and the chat runtime's real round/tool/proposal budget ledger with deadline admission checks.
- The root Rust engine owns model/provider protocol, SQLite transactions, tool approvals, multi-agent scheduling, cancellation, real acceptance verification and user assets.
- `src/engine/execution_scope.rs` limits Home SDK tool-policy/budget effects to the active compatible job and calculates its budget relative to its own persisted session baseline. Standard chat is no longer constrained by the *finished* Home job.
- Existing embeddable Rust SDK (`src/sdk.rs`), optional TypeScript SDK, UI/SDK compatibility API, SQLite schema, and durable user records remain unchanged.

## Evaluation boundaries

This source separation is meant to make replay decisions independently testable and portable, **not to claim frontier parity**. The available CI exercises the new workspace library's unit tests together with the root package. End-to-end real-model patch completion rate, restart-on-unknown-effect tests and token/latency benchmarks still require comparative runs against the same task set and budget.

This work does not modify `main`, does not add detached campaign engines or built-in generators, and does not delete stored data.

## Cancellation and resource boundaries

The chat turn's cancellation watcher registers with `AppState.cancel_notify` before checking the persisted session epoch. Same-process cancel wakes listeners immediately; a 1-second fallback observes cancellation persisted by another process or a crash/restart without relying on in-memory notification. The previous 50ms loop queried cancellation and generation from SQLite on every interval (up to 40 individual lookups per second per active watcher). The replacement reduces the **configured polling frequency** to at most two epoch lookups per fallback check; runtime CPU/DB measurements are pending.

The independent `BudgetLedger` now refuses to admit new rounds, proposals or tools after its wall-clock deadline, including parallel branches. Its existing disjoint child allocation and reconciliation behavior is covered by the kernel's own Rust unit tests.

## Dedicated cancellation supervisor

`src/engine/chat/cancellation.rs` is the single owner of waiting for cancel, draining in-flight tool receipts, and recording unknown effects after a timeout. The 5.6k-line chat runtime only calls `supervise_tool_cancellation` and cannot silently mark an in-progress effect as rolled back. Integration tests exercise both immediate in-process notification and the persisted epoch fallback without a shared notifier.

## Bounded main UI polling

The Home `/api/sdk/runs` adapter now looks up pending approvals only from the engine's live waiter IDs rather than reading all historical approval records for every poll. Final answers are fetched directly by the persisted message ID and checked against the session; running jobs no longer show an unrelated prior assistant response. Legacy terminal jobs without an explicit message ID keep a fallback lookup. The persisted JSON API contract is unchanged; this is a query-path optimization, not a new scheduler or cache.

## SDK-controlled ordinary prompt attempts (2026-11-17 re-judgment)

The standalone `dominion-agent-kernel::Cycle` now admits the real
`jobs::run_prompt -> autonomy::execute -> chat::run_quiet_outcome` attempt
loop. It owns bounded attempt advancement, retry feedback, stalled evidence,
token-limit termination, and the replay-safe response to concurrent owner
steering. Previous ad hoc copies of that logic were removed from the product
adapter. The product is still responsible for model I/O, the real verifier,
durable receipts, atomic steering gate closure and tool approval; these are
not spoofed by the SDK. Every persisted autonomous receipt also carries a
kernel attempt-budget snapshot. Kernel unit tests exercise multi-attempt
termination, stalls, unknown remote writes and owner changes.

This is a concrete improvement in SDK ownership, not a claim that every
chat/room turn now uses a full standalone agent SDK. The existing JSON task
contract and frontend remain unchanged. Actual model success-rate and
round-trip latency still need workload measurement.

The terminal budget gate now observes the real session's cumulative
token usage (not just an error message). The kernel refuses another
model attempt after the limit is reached even if steering changed during
the previous turn. Retriable feedback is written into the SDK cycle before
its checkpoint snapshot is persisted.
