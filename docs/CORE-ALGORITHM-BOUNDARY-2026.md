# DOMINION Core / Algorithm Ownership — 2026

## Non-negotiable boundary

The `crates/dominion-core` Rust package is the stable execution harness.
It owns **phase order, model/tool round lifecycle, budgets, approval/effect
receipts, journaling, cancellation and final verification**. It has no
dependency on `src/addons`, model vendor selection, memory or experiment code.

The only intelligence-facing runtime contract is
[`TurnStrategyPort`](../crates/dominion-core/src/strategy.rs).
It describes decisions, not their implementations:

| Port method | Product-owned implementation | Core behavior |
| --- | --- | --- |
| `execution_limits` | Versioned execution-budget algorithm | Enforces SDK + native ceilings |
| `tool_discovery` | Discovery strategy | Preserves read-only and approval checks |
| `discovery_overridden` | Strategy telemetry | No tool authority changes |
| `decompose` | Acceptance-lineage DAG strategy | Still verifies actual tool evidence |
| `planning_policy`, `needs_plan` | Optional planning algorithm | Runs ordinary model/tool loop |
| `harness_plan` | Optional procedure strategy | Tools still enter the shared effect gate |

The product host assembles the port through
[`runtime_hooks::strategy_port`](../src/addons/runtime_hooks.rs).
The current implementation is
[`ScopedAlgorithms`](../src/addons/strategies/turn_policy.rs).
The native chat adapter calls the port and **does not name algorithm
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

- `cargo test --locked -p dominion-core strategy::tests`
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
