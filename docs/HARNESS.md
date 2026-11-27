# DOMINION active Harness / Core contract — 2026-11-21 (`branch3`)

> Source of truth: [../ARCHITECTURE.md](../ARCHITECTURE.md). This document describes the current executable source, not the historical removed standalone harness.

## Real native run

```text
POST /api/jobs | POST /api/sdk/runs | compatible POST /api/harness/runs
            ↓
src/host/execution/jobs.rs (durable queue / ownership)
            ↓
src/host/execution/native_agent.rs (Host AgentPorts)
            ↓
crates/dominion-agent-algorithms/src/agent_loop.rs (ReActLoop)
            ↕
crates/dominion-agent-runtime/
  contracts.rs (AgentLoop/AgentPorts), run_contract.rs (RunRef/RunPhase/RunTransition)
  schema.rs, dispatch.rs, acceptance.rs, scheduler.rs
            ↕
src/host/journal.rs + src/storage/sqlite.rs (durable checkpoint/receipt)
            ↕
src/features/providers/ + src/integrations/ (authorized models and tools)
```

`crates/dominion-agent-sdk/src/lib.rs` is a **stable facade** re-exporting Runtime and Algorithms; it does not own another implementation loop. `sdk/rust` and `sdk/typescript` are client SDKs.

### Existing streaming conversation adapter

`/api/chat` and Room Chat still use `src/conversation/chat/runtime/turn.rs` for model-turn streaming and context. Its `kernel_bridge.rs` implements the shared SDK Journal/Dispatcher evidence boundary. This **upper conversation loop has not yet been migrated** into `AgentLoop::execute`. All surfaces share typed persisted `RunRef`/`RunPhase` projections via `GET /api/runs/:id`, not a second in-memory source of truth.

### Rooms and Team are different coordinators

- `src/features/collaboration/room_cursor.rs`: per-agent durable unread delivery, atomic cursor/answer commit, no early acknowledgement of peer messages
- `src/features/collaboration/workboard.rs`: scoped host receipts, not model-written success claims
- `src/features/collaboration/task_team.rs`: durable task DAG over native SDK child runs, not another model loop
- `src/features/collaboration/worktree.rs`: owner-selected isolated writer and digest-reviewed patch staging
- `src/integrations/agent_bridge.rs`: external Codex/Claude/Qwen process observations with explicit lower trust, not native tool receipts

Both SDK clients expose canonical RunView plus Task Team create/get/cancel/retry, external bridge preflight and Git Worktree preview/promotion. SDK clients cannot silently merge, commit or push changes.

## Retired standalone harness: preserved compatibility, not active execution

The former independent `crates/harness` and `dominion-harness` CLI were removed. Existing archived `sdk_run` evidence is readable only and not automatically resumed, replayed or upgraded. New compatibility endpoint requests are translated into native jobs with a narrower tool authorization scope.

## Invariants that cannot be traded for optimization

1. Model/tool requests carry pinned identity and bounded budgets.
2. Host writes intent before effect; failed/uncertain effects are review barriers.
3. Terminal durable status fences replay, even if an old phase field looks nonterminal.
4. External agent results are observations, not native Rust effect receipts.
5. Owner controls promotion of isolated changes by verified digest.
6. No additional app menu, microservice or model loop is created to support Team.

## Development verification (no installer build)

```sh
cargo test -p dominion-agent-runtime --lib
cargo test -p dominion-agent-algorithms --lib
cargo test -p dominion-sdk --lib
cargo test --bin dominion-rust host::execution::run_state::tests
cargo test --bin dominion-rust host::execution::run_view::tests
cargo test --bin dominion-rust features::collaboration::room_cursor::tests
cargo test --bin dominion-rust features::collaboration::task_team::tests
npm test --prefix sdk/typescript
```

**Remaining integration contract**: The Chat/Room upper model loop must be routed through a streaming-capable implementation of the shared `AgentLoop` with parity tests for streaming, cancellation, approval, context and exact cost/receipts before removing the legacy adapter. See [migration section](../ARCHITECTURE.md#9-migration-boundary-still-required-before-claiming-one-physical-model-loop). This has not been claimed completed.

A passed CI run applies to its precise commit and selected tests; it is not evidence of real provider credentials, cross-platform signed installers or measured frontier-model task parity.
