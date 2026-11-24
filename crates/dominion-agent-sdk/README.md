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
