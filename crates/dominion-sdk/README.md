# DOMINION Rust SDK

The app and external Rust integrations use the same native loop. No separate
agent implementation is introduced.

- `dominion-contracts`: core types, budget, journal, tool/model capability contracts.
- `dominion-harness`: native runtime and Host/Journal ports.
- `dominion-core`: compatibility facade for the original public imports.
- `dominion-extensions`: independent external application manifest contract.
- `dominion-algorithms`: executable iteration algorithm.
- `dominion-sdk`: public facade used by the app itself. Embedded hosts implement
  `Host` and `Journal` and supply model/tool/approval/verification adapters.
  Disable default features for embedding without the HTTP client.
- `packages/sdk-typescript`: JavaScript/TypeScript HTTP transport.

## External Rust integration

In a checkout, add the dependency using the local path (or a Git revision
with package = "dominion-sdk"). Registry publication is not implied.

```toml
dominion-sdk = { path = "/path/to/dominion-rust/crates/dominion-sdk" }
```

```rust,no_run
use dominion_sdk::{Client, StartRun};
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let sdk = Client::connect("http://127.0.0.1:4317")?;
let run = sdk.start_run(&StartRun {
    instruction: "Inspect this project".into(),
    idempotency_key: Some("request-unique-1".into()),
    ..Default::default()
}).await?;
let run = sdk.get_run(&run.id).await?;
println!("{:?}", run.status);
# Ok(()) }
```

Wait helpers return at owner-action boundaries. Only verified native success
counts as success. The SDK never automatically approves or replays uncertain
effects. Callers may provide a unique stable idempotency key per logical request.

## Native embedding

Use `default-features = false` and implement `dominion_sdk::Host` plus the
durable `Journal` contract. Construct `Context` with a native snapshot, budget,
and journal, then invoke `dominion_sdk::run(&mut host, context)`. The product
chat adapter uses this exact entry point. An embedding host is responsible for
its own permission enforcement and durable adapters; merely linking this
library does not grant access to the app database, model keys or tools.

Run `cargo test --locked -p dominion-sdk` and
`cargo check --locked -p dominion-sdk --no-default-features`.

Both clients expose the existing source history and model inference support endpoints.
Use `runs_by_source`, `inference_support`, and `inference_support_batch`
without inventing model capabilities. The external-consumer tests in
`tests/embedded.rs` and `tests/http.rs` exercise the public crate surface;
they use a deterministic host and loopback HTTP fixture, not a paid provider.

## Register and run a separately installed app

The owner can register and enable an app in Extensions > Installed > External
SDK apps or with the SDK. The app owns its implementation and lifecycle
outside Dominion. No source import or rebuild of the product is necessary.

```rust,no_run
use dominion_sdk::{Client, StartRun, extensions::Manifest};
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let sdk = Client::connect("http://127.0.0.1:4317")?;
// Owner setup: registration starts disabled, then explicit activation.
let manifest = Manifest {
    schema_version: 1,
    id: "example.assistant".into(),
    name: "External assistant".into(),
    version: "1.0.0".into(),
    description: "Independent app".into(),
};
sdk.register_extension(&manifest).await?;
sdk.set_extension_enabled(&manifest.id, true).await?;
// In the independently running app:
let run = sdk.start_extension_run(&manifest.id, &StartRun {
    instruction: "Inspect this project".into(),
    idempotency_key: Some("assistant-request-1".into()),
    ..Default::default()
}).await?;
println!("{:?}", sdk.get_run(&run.id).await?.status);
# Ok(()) }
```

Disabling rejects new managed requests. Existing identical retries return
existing runs, and the owner may recover/inspect them after unregistering.
Unregister requires a disabled registration. Prior run/version history is
retained. This owner API is for trusted apps, not a per-app credential or
OS sandbox. Unmanaged owner SDK calls remain backward compatible.

### Scoped application credentials

The owner can issue, rotate and revoke a per-app credential from Extensions > Installed > External SDK apps. The delegated mode is isolated text chat only: pinned provider/model, per-run token budget and 1–100 new requests per issuance; no tools, global instructions, other app results or owner API. Existing managed owner SDK jobs retain full native execution and approval semantics. Tokens are returned once, stored as hashes, and never sent to the model. Revocation/disable blocks new scoped requests; existing owner work remains recoverable. A scoped credential is an HTTP capability, not an OS sandbox for local processes. Do not give untrusted programs the owner's loopback access or admin token; use OS isolation or a restricted proxy for them.

Owner: `Client::issue_extension_grant(id, &GrantInput { provider_id, model, token_budget, max_runs })` returns a one-time token. The delegated app uses `Client::scoped(base, id, token)`, then `start(instruction, retry_key)` and `wait(run_id, deadline)`. The owner may query the grant balance or revoke the grant.
