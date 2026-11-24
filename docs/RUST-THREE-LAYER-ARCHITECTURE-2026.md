# Rust three-layer crate boundary — 2026-11-20

## Concrete workspace

```text
Cargo Workspace
├── crates/dominion-core/          # infrastructure and traits
│   └── src/runtime_port.rs         # Host, Context, Flow, ModelDecision, model_call
├── crates/dominion-algorithms/    # agent iteration + algorithm tests
│   ├── src/engine.rs              # sole Engine::drive loop
│   └── src/engine_tests.rs        # former core loop regression suite
└── dominion-rust (root package)   # gateway + application composition
    ├── src/api/                   # HTTP / SSE and SDK-facing endpoints
    ├── src/engine/chat/           # model & tool host adapters
    └── src/addons/strategies/     # optional algorithms and policies
```

Dependency direction is `dominion-rust -> dominion-algorithms -> dominion-core`.
The gateway additionally imports core contracts directly for its adapters.
No `dominion-core -> dominion-algorithms` import exists.

The **single production loop** is a mandatory harness implementation
(package name `dominion-algorithms`) rather than an optional selectable
agent execution kernel. Moving it did not create an additional loop.
The one production loop moved, rather than being copied. The original
engine tests moved alongside it, so no duplicate retry controller, approval
path or effect execution path was introduced. `Host` remains a core trait;
`Engine::drive` is now an algorithm implementation.

`runtime_port::model_call` remains in core because it only admits and
journals model calls — it decides no prompts, strategies or loop phase order.
The algorithm crate re-exports this function temporarily so existing
auxiliary product callers use the same durable journal during migration.

## What has not changed

Existing model providers, native tool admission, approval policy, SQLite
execution snapshots, streamed user events, restart/cancel semantics,
multiagent workspaces and React UI are unchanged.

`src/addons/strategies/turn_policy.rs` remains a product-owned strategy
implementation because it accesses app-specific SQLite state. The 
standalone algorithms crate contains the model/tool iteration loop, not the
entire portfolio of product policies.

Rust compilation remains a **single application binary**, so this is
package/ownership isolation, not a WASM sandbox or hot-reload boundary.
Changes to the algorithm crate still rebuild dependent linked binaries;
the gain today is test scope, ownership and independent runtime evolution.

## Checks

- `cargo test --locked -p dominion-core --lib`
- `cargo test --locked -p dominion-algorithms --lib`
- `cargo test --locked --bin dominion-rust ...` (existing product regressions)
- `node scripts/check-runtime-architecture.mjs` (reverse dependency / loop ownership)
- CI macOS and Windows backend/frontend checks

## References (architectural examples)

- [ZeroClaw API/runtime crate ownership](https://github.com/zeroclaw-labs/zeroclaw/blob/master/docs/book/src/architecture/crates.md)
- [ZeroClaw request lifecycle](https://github.com/zeroclaw-labs/zeroclaw/blob/master/docs/book/src/architecture/request-lifecycle.md)
- [IronClaw runtime and WASM capability isolation](https://github.com/nearai/ironclaw)
- [OpenCrust Rust workspace](https://github.com/opencrust-org/opencrust)

These are patterns to compare, not source-code dependencies of DOMINION.

## Stable iteration interface

`dominion-core::runtime_port::AgentLoop<H>` is the gateway-visible iteration
contract. `dominion-algorithms::engine::Engine` implements the trait while
the product adapter invokes `AgentLoop::run(&Engine, host, context)`. The
original loop regression suite also runs through this trait. Alternative
implementations can be swapped at the product composition point without
adding algorithm imports to the core crate.

The algorithm remains in the same trusted Rust address space; the interface
is not a sandbox and its type-level boundary must not be mistaken for one.

## Context compaction ownership

The original `context::compact` projection and its unit tests now live in
`dominion-algorithms/src/context.rs`. It is an algorithm, not a durable
storage API. The gateway's existing provider-specific context compiler is
unchanged. The core retains snapshots and transcript persistence, never
deciding which previous messages to include in a model prompt.

## Room delivery cursor (2026-11-20)

The shared room transcript remains in SQLite `messages`. The
`room_agent_read_cursors` table records each participant's last
**durably completed reply**, the owner source cursor and exactly which
messages entered that participant's compiled model context.
A second round on the same owner message is allowed; stale commits and
cross-room/session/agent forgery cannot advance the cursor.
An in-flight crash still enters manual review; a cursor is never used to
assume that an uncertain tool action finished. The room coordinator updates
the cursor after the committed assistant message, and the context compiler
uses it as the previous-self anchor across long conversations.

The cursor is **not a global delivery acknowledgement for every earlier
message**, because token-limited context selection intentionally omits
some history. The exact `selectedMessageIds` set is the observation proof.


## External SDK app boundary (2026-11-24)

`dominion-extensions` owns versioned registration data and validation, with no
dependency on the engine. `dominion-sdk` re-exports the contract and provides
typed Rust and TypeScript clients. The product Host persists registrations via
`/api/extensions/apps`; the existing Extensions > Installed screen manages them.
The reserved SDK source `extension` admits a new run only while its registration
is enabled, using the same lock for registration changes and durable SDK
admission. The run records the registered manifest version; disable/unregister
preserves already admitted runs, approval/recovery and retry receipts.
MCP continues through the existing native MCP transport and tool policy.

External apps are installed and executed independently by their owners.
Registration is not package installation, a code loader, a sandbox, a scoped
API credential or proof of caller identity. The owner API remains trusted.
No personal assistant implementation is bundled by this change.

Native SDK embedding, the execution harness and algorithm loop remain the
same; product-bound model adapters, knowledge and strategy policies still
live in the product host. They are not yet separate crates.
