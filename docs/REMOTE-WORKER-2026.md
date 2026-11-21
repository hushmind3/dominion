# DOMINION Remote Worker — native SDK transport (2026-11)

A remote worker uses an ordinary DOMINION Rust server installed *on the
remote machine*. The coordinator does not reimplement the Harness and does
not accept a remote self-report as a verified native tool receipt.

## Coordinator

Set \`DOMINION_REMOTE_WORKER_TOKEN\` to a 32+ character randomly generated
secret. Expose the coordinator through HTTPS (or local loopback for testing).
Do not expose an unencrypted remote HTTP endpoint carrying this token.

The owner submits an explicit task:

\`\`\`http
POST /api/remote/jobs
Content-Type: application/json

{"instruction":"Inspect the test failure","maxRounds":12}
\`\`\`

Owner read / review endpoints:
- \`GET /api/remote/jobs\`
- \`GET /api/remote/jobs/:id\`
- \`POST /api/remote/jobs/:id/review\` with \`{"accept":true|false}\`
- \`POST /api/remote/jobs/:id/cancel\`

Worker-only calls require \`x-dominion-worker-token\`:
- \`POST /api/remote/claim\` with \`{"workerId":"machine-1"}\`
- \`POST /api/remote/jobs/:id/heartbeat\` with \`{"workerId", "leaseId"}\`
- \`POST /api/remote/jobs/:id/finish\` with \`{"workerId", "leaseId", "result"}\`

The 75-second lease must be refreshed by the same worker. A stopped or expired
lease is **never** re-queued automatically because effects may already have
happened on the remote device. Owner cancellation invalidates the lease.
A worker-provided \`remoteVerifiedClaim\` is evidence to review, not the
coordinator's own \`nativeVerified\` claim.

## Worker machine

Start a local DOMINION Rust server on \`127.0.0.1:4317\` with its own
model/SDK configuration and workspace. Run the standalone Rust binary:

\`\`\`shell
export DOMINION_REMOTE_COORDINATOR="https://coordinator.example"
export DOMINION_REMOTE_WORKER_TOKEN="REPLACE_WITH_RANDOM_32_PLUS_CHAR_SECRET"
export DOMINION_REMOTE_WORKER_ID="workstation-1"
export DOMINION_WORKER_LOCAL_URL="http://127.0.0.1:4317"
cargo run --locked --bin dominion-worker
\`\`\`

Optional \`DOMINION_LOCAL_ADMIN_TOKEN\` is sent only to the local SDK server.
The remote job has one deterministic SDK idempotency key, requires local shell
sandbox admission, and never forwards the coordinator's worker token to the
native SDK. Approval and uncertain effect states are returned for owner review,
not silently accepted. All decisions are journalled by the coordinator.

This protocol provides useful remote SDK execution but does **not** ship
workspace contents, add multi-host write transactions, or claim trusted
cross-host verification. Workers use independent local workspaces.

## Lead/Worker Team DAG remote research dispatch

The `team_execute` tool accepts `allowRemoteWorkers:true` and explicit step
`{"id":"research","profile":"general","remoteWorker":true,"prompt":"..."}`.
The remote adapter uses the existing CAS-admitted prompt job, writes a durable
replay fence, then enqueues one deterministic remote SDK ticket. A remote
report is admitted into Team synthesis only after owner review and a
provenance-checked assistant message; its evidence level is
`remote_owner_reviewed`, never `verified_tool`. Dependent Team steps wait
for that acceptance. Restarts, unknown effects and expired leases do not
automatically rerun work. Implementation/verification stay local because a
cross-device Git patch transfer and verifier proof is not implemented.

## Correlation and cost provenance

The coordinator attaches a remote ticket to its **existing Lead native
execution ID** only after verifying the owning Team and Lead session. The
remote device's own `remoteNativeExecutionId` is a separate namespace. Its
reported `modelAccounting` is retained as self-reported evidence (not
coordinator-native token receipts). Lead/Worker synthesis exposes distinct
`remoteJobId`, `externalRunId` and `executionId` properties rather than
fabricating a local execution identity for remote work.

## Explicit lease reconciliation

`POST /api/remote/reconcile` moves expired `leased` rows into
`needs_review` under the SQLite transaction lock and returns their IDs.
Worker claims perform the same sweep before selecting queued work. A
concurrent worker heartbeat cannot be overwritten by an obsolete expiry
observation. Missing evidence can be rejected or cancelled, but never
accepted as a completed remote task. The TypeScript client exposes
`reconcileRemoteLeases()`.


## P3 Multi-machine control plane (2026-11-21)

The owner controls worker identity, eligibility and dispatch through **the
existing Rust API and SQLite journal**, not a second execution kernel.

### Coordinator and owner authentication

Set `DOMINION_ADMIN_TOKEN` on the coordinator. It is mandatory for **all**
owner-facing `/api/remote/*` requests, including GET/list, when routed through
the API. Only machine-specific claim, heartbeat and finish endpoints bypass
the owner token; these still require `x-dominion-worker-token`.
Keep the coordinator on loopback or expose it behind HTTPS. The owner token
uses `x-dominion-token`; it must never be copied to the worker machine.

Owner API:

- `POST /api/remote/workers` body `{"workerId":"gpu-1","capabilities":["sdk","gpu"]}` creates an enrolled machine and returns its `token` **once**.
- `GET /api/remote/workers` lists enabled state, capabilities and last-seen time, never credential hashes or tokens.
- `POST /api/remote/workers/gpu-1/rotate` rotates the token and returns the replacement once. Existing in-flight leases can no longer heartbeat with the old credential; their uncertain effects require manual review.
- `POST /api/remote/workers/gpu-1/state` with `{"enabled":false}` pauses dispatch and invalidates heartbeats until enabled.

The per-machine `tokenSha256` exists only in coordinator SQLite. Event
publications exclude it. Registered workers cannot authenticate with the
old shared `DOMINION_REMOTE_WORKER_TOKEN`, even if they know that secret.
Unregistered legacy workers can still use the shared secret for **unconstrained**
jobs. Prefer enrolled identities for multi-device installations.

### Capability-pinned work

A job may now specify:

```json
{
  "instruction": "Inspect code on the GPU worker",
  "targetWorkerId": "gpu-1",
  "requiredCapabilities": ["sdk", "gpu"],
  "maxRounds": 12
}
```

The owner must have enrolled and enabled an eligible worker before such a
job is admitted. `requiredCapabilities` are owner-defined labels, never
claims made by a model or proof of verification. Only matching registered
machines can lease a pinned or capability-constrained job; multiple polling
workers cannot claim the same ticket, since the final transition is a SQLite
compare-and-patch under the connection lock.

Queued and active remote jobs have dedicated SQLite partial indexes. The
coordinator does not scan historical completed reports on each poll.
Leases carry fresh unpredictable `leaseId` values; heartbeat and finish
require exact worker identity, lease and unexpired deadline. A transport
failure on a worker's heartbeat attempts to cancel its local SDK run.
Expiration, process death, token rotation, owner cancellation and unknown
effects all **fail closed** without an automatic second execution.

### Invariants across P0–P3

- **P0:** `dominion-core` owns durability, cancellation, journal and policy.
  `dominion-algorithms::Engine::drive` remains the one native Harness.
  Run/Event correlation uses the existing `RunEventRef`.
- **P1:** Room cursors and selected-message evidence, endpoint capability
  negotiation, and native Lead/Worker DAG remain product-owned adapters.
- **P2:** Codex/Claude/Qwen Agent Bridge has a distinct external execution
  namespace, per-session event matching, and cannot forge native receipts.
  Strategy/memory decisions remain outside the Core.
- **P3:** The remote control plane leases the existing native SDK execution
  service on other machines; it does not introduce a second agent loop.

**Scope boundary:** This is a working multi-machine control plane and worker
transport, *not* a distributed file system or atomic cross-host Git merge.
Cross-host native verification is not asserted. Remote owner-reviewed reports
never set `nativeVerified:true`; remote write effects remain on the worker's
own isolated workspace until a separate explicit artifact-transfer contract
is implemented.
