# DOMINION native Rust SDK

Embed the same engine the application uses, without running the HTTP server
or linking the desktop UI, SQLite product store, or provider adapters.

In your own Cargo project, add:

```toml
[dependencies]
dominion-agent-sdk = { path = "/absolute/path/to/dominion-rust/crates/dominion-agent-sdk" }
```

A Git dependency pinned to a reviewed commit is also supported by Cargo.
No registry publication is implied.

Implement the SDK's `Host` trait for your model, tools and verification,
and `Journal` for durable checkpoint storage and cancellation. The host
owns authorization, sandboxing, provider authentication and effect receipts;
use `dispatch_batch` for native effect admission and do not bypass it.
Persist the initial snapshot, then pass the same snapshot and journal to the
SDK. For example, given an implemented host and journal:

```rust,ignore
use dominion_agent_sdk::{run, BudgetLedger, Context, Snapshot};
use std::time::Duration;
let snapshot = Snapshot::new("unique-run-id", "inspect and verify", 8)?;
journal.save(&snapshot)?;
run(&mut host, Context {
    snapshot,
    budget: BudgetLedger::new(8, 16, Duration::from_secs(120)),
    round: 0,
    journal: &journal,
}).await?;
```

The SDK is not a ready-configured provider or sandbox; hosts must implement
these interfaces. Do not resume an unsettled paid or tool intent without
reconciliation. A completed lifecycle is not proof that a host's independent
verification succeeded. The product's HTTP SDK remains the convenient
choice for plugins that want the application's configured providers, SQLite,
approval UI, and full workflow handling.

Dependency direction: product / external host -> SDK -> algorithms -> core.
The SDK also reexports core contracts. The product calls this entry point
directly; there is no second model/tool loop.

Run `cargo test --locked -p dominion-agent-sdk` for the independent host test.
For HTTP clients, see [TypeScript SDK](../../packages/sdk-typescript/README.md).

## Simpler external host integration

Use `run_embedded` if you do not need custom phase scheduling. Implement only
`CheckpointStore` (a single-run durable Journal with load), `ModelPort`,
`Dispatcher` and `Verifier`. The adapter translates structured model answers
and tool actions into the **same** native Engine phases. It preserves receipts,
verification retry feedback and reported token totals. An unsuccessful verifier
returns `Verification::Retry(reason)`; the next model turn sees this in the
checkpoint events. No second loop, HTTP server or provider is required.

```rust,ignore
let result = dominion_agent_sdk::run_embedded(
    &journal, &mut model, &dispatcher, &mut verifier,
    dominion_agent_sdk::EmbeddedLimits {
        max_rounds: 8, max_tools: 16,
        max_elapsed: std::time::Duration::from_secs(120),
    },
).await?;
```

The caller creates the initial checkpoint once, exclusively leases that run,
and implements authentication, sandboxing and approval inside the dispatcher.
Tools must have an advertised JSON schema; an empty catalog or model-only
frame cannot fall back to unrestricted dispatch. Limits count persisted
rounds and receipts; elapsed time applies to the current invocation. A provider
adapter must enforce its own request timeouts and report failed/unknown costs.
On error inspect the checkpoint; never automatically restart unknown effects.
The integration tests in `tests/ports.rs` exercise an actual local test artifact,
failed verification repair, uncertain write recovery and model-only denial.

## Native layers and host boundary

The Rust SDK reexports the four reusable execution/decision surfaces:

| Layer | Rust entry point | Responsibility |
| --- | --- | --- |
| Core | `dominion_agent_sdk::runtime` | Checkpoint, budget, program IR, predicates, tool admission contracts |
| Harness | `dominion_agent_sdk::harness` | Single native agent execution loop |
| Algorithm | `dominion_agent_sdk::algorithms` | Memory ranking, experience, learning, planning, adaptation, team decisions, tool selection, procedure evaluation |
| SDK | `dominion_agent_sdk::run`, `run_embedded` | Public embedding entry points |

Host is a port implemented by the desktop/server product or an independent
embedding application, not a second engine. SQLite, provider transport, MCP,
actual filesystem observations, remote worker authentication, workspace isolation
and owner approvals stay in the product adapters. Pure algorithm decisions take
authorized data and observations; they do not authenticate a plugin, grant tool
permissions or certify a task without trusted receipts. External HTTP plugins
use the registered SDK API and are managed in Connections / Plugins. Embedded
offline hosts are not automatically visible to that server. Existing native MCP
transport remains a distinct tool-provider path rather than an HTTP SDK client.

This split preserves existing product data and execution semantics; it does not
imply that every product adapter can be removed or every standalone SDK host
automatically inherits the desktop application's learning database.
