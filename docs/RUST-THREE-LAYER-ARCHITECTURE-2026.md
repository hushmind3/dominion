# DOMINION modular Rust architecture — 2026-11-24

## Concrete workspace and ownership

```text
External apps (separate repositories/processes)       MCP servers
       | Rust / TypeScript SDK                              | MCP
       v                                                     v
   SDK app registration / owner API                 Native MCP adapter
       |                                                     |
       +------------------ product Host ---------------------+
                                 |
                           dominion-sdk
                                 |
                      dominion-algorithms
                      (one Engine::drive)
                                 |
                       dominion-harness
                 (admission, dispatch, host port)
                                 |
                      dominion-contracts
              (journal, budgets, types, capabilities)
```

| Package | Owns |
|---|---|
| `dominion-contracts` | Core types, model/tool capability contracts, durable journal transitions, budgets, stable IDs, delegation and strategy interfaces |
| `dominion-harness` | Native runtime snapshot, ordered dispatch, model-call admission, Host/Journal ports |
| `dominion-algorithms` | Sole iteration loop, context projection, task decomposition, plan scoring, planning decisions and reasoning policies |
| `dominion-sdk` | Public native embedding and HTTP client; product chat calls the same SDK execution entry |
| `dominion-extensions` | Versioned external application manifest and lifecycle validation; no model, database or runtime dependencies |
| `dominion-core` | Compatibility re-export facade only; original public paths preserve type identity |
| `dominion-rust` | Product Host, SQLite, actual provider/OS/approval adapters, UI/API and persisted extension registry |

Dependencies point from algorithms to harness and core contracts, never the
other way. The product Host implements the shared ports. External SDK apps do
not link the product binary or embed a second agent loop. The existing MCP
transport remains distinct from the SDK task API.

The original source files and tests were moved, not copied. The compatibility
facade preserves the previously exported `dominion_core::hosted`,
`dominion_core::runtime_port` and domain/journal/type paths. Serialized records,
stable identity calculations, and the execution journal remain unchanged.

The standalone algorithms crate owns the pure algorithm implementations.
Product-bound policy selection, memory retrieval and learning still use
product Host adapters and SQLite. These are core product capabilities, not
external apps. This change does not claim all Host-side policy or provider
implementation has been extracted into independent crates.

## Checks

- `cargo test --locked -p dominion-contracts -p dominion-harness -p dominion-algorithms -p dominion-extensions -p dominion-core --lib`
- `cargo test --locked -p dominion-sdk`
- `cargo test --locked --test e2e external_rust_sdk_runs_real_product_with_approval_and_retry`
- `npm test --prefix packages/sdk-typescript`
- `npm run typecheck --prefix web`

The Rust app remains a single linked binary. This is compile-time ownership
isolation, not a dynamic code loader or sandbox.

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

Native SDK embedding uses the separated harness and core-contract packages.
Product-bound model transport, knowledge and strategy selection remain Host
adapters. No second execution loop is introduced.

### Scoped application credentials

The owner can issue, rotate and revoke a per-app credential from Extensions > Installed > External SDK apps. The delegated mode is isolated text chat only: pinned provider/model, per-run token budget and 1–100 new requests per issuance; no tools, global instructions, other app results or owner API. Existing managed owner SDK jobs retain full native execution and approval semantics. Tokens are returned once, stored as hashes, and never sent to the model. Revocation/disable blocks new scoped requests; existing owner work remains recoverable. A scoped credential is an HTTP capability, not an OS sandbox for local processes. Do not give untrusted programs the owner's loopback access or admin token; use OS isolation or a restricted proxy for them.

Pure lexical memory ranking, experience similarity and bounded learning-candidate parsing now live in `dominion-algorithms`. Host-side visibility and source verification, learning job execution, persistence and receipt attribution remain product adapters. No algorithm reads SQLite directly.
Architecture references consulted 2026-11-24: MCP 2026-07-28 authorization/security considerations (audience separation, no credential passthrough) and Octos embeddable Rust runtime (shared contracts and plugin boundary); this is a local scoped capability protocol, not an implementation of MCP OAuth.
