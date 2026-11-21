# DOMINION Core / Algorithm Ownership — 2026

## Non-negotiable boundary

The `crates/dominion-core` Rust package is the stable **execution infrastructure**.
It owns **model/tool intent journaling, bounded budgets, cancellation, snapshots,
policy checks and host trait contracts**. It owns no agent-iteration algorithm
or phase-order loop.

The `crates/dominion-algorithms` Rust package depends on `dominion-core`
and owns the **single existing planning → model → tools → verification loop**.
The product gateway (`src/api`, `src/bootstrap`, chat adapters) composes the
driver, host implementations and native tool approvals. The core has no
dependency on the algorithms crate, model vendors, memory or experiments.

**Ownership terminology:** `dominion-algorithms::Engine` is a mandatory,
single product harness, **not** one of the optional swappable policies.
Only model/tool/planning decisions under `TurnStrategyPort` are
configurable strategies. These crates compile into the same Rust binary;
runtime dynamic code loading is not implemented.

The execution host contract is
[`runtime_port::Host`](../crates/dominion-core/src/runtime_port.rs), consumed by
[`dominion_algorithms::engine::Engine::drive`](../crates/dominion-algorithms/src/engine.rs).
The replaceable strategy-selection contract is
[`TurnStrategyPort`](../crates/dominion-core/src/strategy.rs).
It describes decisions, not their implementations:

| Port method | Product-owned implementation | Core behavior |
| --- | --- | --- |
| `execution_limits` | Versioned execution-budget algorithm | Enforces SDK + native ceilings |
| `tool_discovery` | Discovery strategy | Preserves read-only and approval checks |
| `discovery_overridden` | Strategy telemetry | No tool authority changes |
| `decompose` | Acceptance-lineage DAG strategy | Still verifies actual tool evidence |
| `planning_policy`, `needs_plan`, `planning_decision` | Optional planning algorithm | Versioned typed `PlanningDecision`; native Harness still runs model/tool loop |
| `harness_plan` | Optional procedure strategy | Tools still enter the shared effect gate |

The product host assembles the port through
[`runtime_hooks::strategy_port`](../src/addons/runtime_hooks.rs).
The current implementation is
[`ScopedAlgorithms`](../src/addons/strategies/turn_policy.rs).
The product gateway calls the port and **does not name algorithm
implementation IDs**. Versioned strategy records can change selection without
editing the Rust execution driver.

The tool-discovery implementation source is
[`chat_tool_catalog.rs`](../src/addons/strategies/chat_tool_catalog.rs)
and is not under `src/engine/chat/runtime/` anymore.

## Owner-level controls

The backend extension policy accepts `"strategies": false` at
global, session, room or agent scope. The same attenuation rules used for MCP
and memory apply. Disabled strategy execution uses the native limits,
null discovery, zero optional subtasks, no optional planning and no strategy
usage signals; it **does not** turn off the model, tools, approvals, verifier,
or persistent execution journal.

The `memory`, `skills`, `mcp`, `connectors`, and `channels` policies
remain independent. Existing deployments inherit enabled add-ons if they
do not opt out. No UI navigation changes and no persistence migration are
required for this extraction.

## Ownership enforcement

`scripts/check-runtime-architecture.mjs` fails if the native turn loop
reintroduces concrete budget/discovery/decomposition algorithm names,
reimports the decomposition implementation, or moves the tool catalog back
inside the execution engine.

Regressions are covered by:

- `cargo test --locked -p dominion-core --lib`
- `cargo test --locked -p dominion-algorithms --lib`
- `cargo test --locked --bin dominion-rust addons::turn_policy::tests`
- Existing focused Rust, real HTTP E2E, Windows and frontend CI.

## What this boundary does *not* claim

Product add-ons are still linked in the same Rust executable. The
`src/engine/mod.rs` compatibility re-exports and some existing product
adapters preserve old Rust module paths. They should be progressively
retired through explicit host composition; this commit does not claim
separate processes, binary unloading, hot-swappable Rust dynamic libraries,
or a sandbox security boundary between in-process add-ons and the kernel.

OpenClaw's 2026 runtime and plugin architecture similarly distinguishes
the reusable core, host-owned runtime, documented capabilities and plugins:
- https://docs.openclaw.ai/agent-runtime-architecture
- https://github.com/openclaw/openclaw/blob/main/docs/plugins/architecture.md

## Shared execution events

[`run_event::RunEventRef`](../crates/dominion-core/src/run_event.rs)
is a versioned, typed execution correlation reference. The Rust product
event bus adds `runRef` only when a real execution ID is present in a
committed execution/message/approval/SDK record. This is additive: public
SSE event fields and existing room/job statuses are not renamed.
`RunLifecycle` intentionally excludes room presentation states like
`idle` or `silence`. Existing `seq` remains the durable replay position.

## Versioned strategy decisions (2026-11-21)

The mandatory Harness still calls one `TurnStrategyPort`. The product
strategy implementation remains optional. This contract exposes explicit
Rust types with `schemaVersion:1` for:
`PlanningDecision`, `DiscoveryDecision`, `DecompositionDecision`,
and `ProcedureDecision`. For compatibility, the existing strategy
functions still implement their legacy return shapes behind default typed
adapters. No new JSON-defined executable module is accepted.

`DecompositionDecision` validates the strategy's subtask list before handing
it to the runtime (objects only, maximum 64). It retains the complete
compatible payload for existing record consumers. `ProcedureDecision`
likewise restricts optional procedures to 32 object candidates. None of
these types grants approval, bypasses sandbox or declares task verification.

## Model/endpoint capability negotiation (§6 architecture.md)

A single Model Gateway now merges declared capability profiles in priority:
**endpoint+model → endpoint → model → provider default**, using an explicit
`endpointId` or the persisted `baseUrl` as endpoint identity.
`ModelCapabilities` carries text, native/text tools, vision, reasoning,
streaming, parallel tools, context-window capacity, output ceiling and
prompt-cache support. An explicit `false` blocks unsupported tools, images,
streaming or cache hints instead of relying on a model-name guess. The
resolved output and context ceilings feed the existing native token-budget
admission. These are owner/provider declarations or actual negotiated wire
evidence, **not claims that a model name alone proves capability**.
External CLI agents remain `AgentBridge` workers, not Model Gateway routes.
