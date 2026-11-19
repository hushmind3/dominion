# DOMINION sub2 — active Rust architecture (2026-11-19)

This document describes **dominion-sub2 source**, not the current main backend.
The React frontend was synced from main at `f86a36e`; backend-specific
differences remain intentional and are checked in CI.

## Execution ownership

- `src/api/mod.rs` accepts HTTP work, chat, room, model and account requests.
- `src/engine/chat/runtime.rs` and `src/engine/chat/runtime/turn_body.rs` own
  model iteration, tool admission, provider messages and terminal outcomes.
- `crates/dominion-agent-kernel/` defines the sub2-native budget, scheduling
  and verification contracts. External tool adapters are not independent agents.
- `src/engine/jobs.rs` owns the durable job queue; retired Money wake-up
  loops, domain-specific jobs and HTTP routers are no longer registered.
- `src/engine/effect.rs`, `src/engine/verification.rs` and `src/db.rs`
  preserve permissions, verification receipts and durable SQLite state.

## Conversation / token economy

- `src/engine/rooms.rs` selects speakers; examples containing a participant
  name do not count as a direct call. Explicit `@name` and genuine address
  particles select participants.
- `src/engine/room_state.rs` preserves source receipts separately from
  model-visible semantic status; unchanged room outcomes have a stable digest.
- `src/engine/chat/room_context.rs` selects committed source, prior self-answer,
  owner facts, peer excerpts and bounded retrieved evidence for each room turn.
- `src/engine/chat/continuity.rs` deduplicates immutable frames and rotates
  outdated room observations instead of accumulating contradictory states.
- `src/engine/chat/trace_budget.rs` retains the latest source-read result
  while compacting older tool calls and large writes in the provider-wire view.

Provider calls are normally stateless HTTP requests: SQLite and the selected
wire context supply continuity. A cache hint is not an observed billing
discount without explicit provider telemetry.

## Current main UI integration

The web tree comes from main and calls Rust routes already present or
backported on this branch. New model catalog reads are
`GET /api/providers/catalog/:id/models` and
`GET /api/providers/openrouter/free-models`. The saved provider can fall back
to exact-endpoint public metadata when an unauthenticated `/models` route
cannot be read. The `gh` login UI uses the first-party CLI broker and vault
rather than an inert browser-only button.

## Source review and gates

- `scripts/check-sub2-contract.mjs` scans active Rust/TypeScript source for
  removed Money calls, checks duplicate HTTP routes and critical UI endpoints.
- `web/tests/*.test.mjs` covers model selection, free-model labeling,
  refresh targeting and chat stop behavior.
- `.github/workflows/ci.yml` runs Rust regression contracts and Node tests.
- Real model, browser, desktop and end-to-end credential checks are separate
  from these static and unit gates.
