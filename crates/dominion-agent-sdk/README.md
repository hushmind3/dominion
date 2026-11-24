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
