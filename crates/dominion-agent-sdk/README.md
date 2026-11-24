# DOMINION standalone Rust agent SDK (sub2)

Use this package from a Rust application, plugin host or CLI without linking
the DOMINION desktop server. For the full native product with SQLite, model
providers, jobs, Room, Team and tools, use the existing `dominion_rust::sdk`.
For a separate process controlling a running product, use the HTTP SDK or
`sdk/typescript`. This crate does not install plugins or execute native
tools automatically.

From a local checkout, add this to an external project's Cargo.toml:

```toml
[dependencies]
dominion-agent-sdk = { path = "/path/to/dominion-rust/crates/dominion-agent-sdk" }
```

Or use the Git repository and a pinned commit using Cargo's `git` and `rev`
dependency keys. Repository access is required; the package has not been
published to crates.io.

## Execution

Implement `ModelPort` for your provider and `ToolPort` for your actual
tool host. Pass them, the conversation history, tool schemas and explicit
`RunOptions` to `execute`. Tools are off by default. For custom algorithms,
implement `AgentLoop`; the runtime still owns admission and cannot be
overridden by the strategy. The application uses the same exported
`Driver` and `Harness`, while its host supplies streaming and journaling.

- `runtime`: independent execution protocol, run-state and host ports.
- `workflows`: built-in algorithms, selectable strategies and task DAG.
- `decisions`: independent verification, failure, budget and decision policy.
- `context`: provider-independent bounded historical context processing.

For long executions, opt into `execute_compacted` with a byte budget for completed tool frames. The same product context policy keeps the latest observation and prioritizes failed or uncertain effects in the remaining budget. Full original receipts remain in caller-owned history; this is not a hard model-token cap or an evidence store.

The SDK does not grant filesystem, network or shell authority. Concrete
adapters must enforce owner consent, sandboxing, budgets and durable effect
reconciliation; the portable runner only enforces its model/dispatch gates.
An answer is not proof of external task completion. On ambiguous dispatch
the transcript preserves the unknown-effect receipt, and does not retry it.

## Checks

`cargo test -p dominion-agent-sdk --locked`

The integration tests exercise the public SDK from an independent host using
deterministic model responses, an external algorithm, tool-admission
restrictions and uncertain effects. Real providers and the desktop need
separate integration verification.
