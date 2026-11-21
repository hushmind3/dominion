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

`team_start` accepts `allowRemoteWorkers:true` and explicit step
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
