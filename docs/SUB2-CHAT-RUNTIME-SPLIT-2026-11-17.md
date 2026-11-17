# Sub2 chat runtime responsibility split — 2026-11-17

## Why

The older single `src/engine/chat/runtime.rs` contained 5,510 lines: turn
entrypoints, a 1,873-line model-turn routine, durable program execution,
deterministic tools, tool dispatch, review, policy snapshot, session helpers,
and three groups of tests. This made a small change liable to affect
unrelated execution responsibilities and made code reviews painful.

## New ownership

- `runtime.rs`: entrypoint/turn lifecycle coordinator and shared imports.
- `runtime/turn_body.rs`: provider turn, context, replay admission, completion.
- `runtime/program.rs`: durable program checkpoints, loops, repair, restore.
- `runtime/deterministic.rs`: frozen exact-replacement tasks and approvals.
- `runtime/tool_calls.rs`: tool dispatch, effect receipts and result persistence.
- `runtime/action_review.rs`: action reviews and planning choices.
- `runtime/selection.rs`: immutable selection and evidence snapshots.
- `runtime/session_state.rs`: session setup, budget-exhaustion and stats.
- `runtime/*_tests.rs`: previous real Rust tests, now alongside the modules.

The existing `TurnRequest`, `run_turn*` public API, approval/authorization,
SQL-backed execution history and checkpoint serialization are not replaced.
Functions are moved with their original bodies; the coordinator exposes them
to sibling private submodules using scoped Rust imports instead of making
new public APIs. No `include!` trick or duplicated alternate runner.

## Follow-up profiling

The large model-turn handler itself remains approximately 1,875 lines.
Future extractions should isolate stream assembly, context compaction and
terminal receipt production with focused tests rather than moving unrelated
behavior into new public SDKs. Actual model/runtime performance changes
must be measured separately; file-size reduction by itself is not a
CPU benchmark.
