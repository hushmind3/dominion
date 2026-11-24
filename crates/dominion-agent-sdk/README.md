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

## Trusted Rust extensions

The independent SDK provides `extensions::ExtensionRegistry`. A host registers
named `ToolRegistration` values with actual `ExtensionTool` handlers, reads
`names()` to select exact per-run grants, and calls `registry.execute(...)` or
`registry.execute_compacted(...)`. Tools must also be enabled in RunOptions.
Registration is atomic; names are stable `ext__plugin__tool` identifiers.
Unregistering removes all tools of an extension before the next invocation.
The mutable registry borrow prevents handler replacement during a run.
Unknown grants and unadvertised tool calls fail closed. Handlers still enforce
host-level authorization, schema validation and actual effect verification.

This is runtime registration of trusted Rust adapters, not dynamic shared
library loading, a package installer, or an app plugin marketplace. For
independently distributed processes and untrusted extensions, use the app's
existing MCP transport and approval boundaries instead. No new application
execution loop or separate plugin daemon is introduced.

## External executable MCP extensions

The native app and independent SDK now share the same initialized stdio MCP
transport. From an external Rust host, call
`registry.register_mcp_stdio("my_plugin", &config, workspace).await` with a
trusted explicit command/args/env object. It starts the process, discovers
its tools and returns the exact grant names. Pass those grants to the
existing `execute` API. Unregister closes the process; a timed-out or
cancelled session cannot replay an uncertain call automatically. Discovery
supports bounded pagination. Aliases accept lowercase ASCII letters,
digits and single underscores, normalizing hyphens/dots; collisions or
unsupported names require an explicit adapter instead of ambiguous dispatch.

This shared path supports initialized MCP stdio protocol negotiation
(2024-11-05 through 2025-11-25), not modern stateless stdio discovery.
The native HTTP transport retains its separate existing modern/legacy
handling. The caller owns permission to start the executable; this is
not an OS sandbox or a Rust dynamic-library ABI. The integration test
runs the repository's existing Node stdio fixture through the actual
SDK model/tool loop (Node is required for that test).
