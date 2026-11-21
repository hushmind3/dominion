# Remote Worker (P3) — owner-requested multi-device execution

DOMINION's optional remote control plane uses the same Rust binary. It is
**disabled by default** and is **not** a replacement for the local Harness.
The original 2026-11-20 architecture makes multi-device routing conditional;
the owner requested it explicitly for Sub3.

## Coordinator (the existing DOMINION machine)

Set:
- `DOMINION_REMOTE_WORKERS=1`
- `DOMINION_REMOTE_WORKER_TOKEN=<random, non-whitespace ASCII 32+ chars>`
- `DOMINION_REMOTE_PORT=7410` (must differ from product API port)

Start DOMINION normally. A second Axum listener binds ONLY
`127.0.0.1:7410`. Never bind this port directly to a public address.
Use a private VPN with TLS termination or SSH local forwarding to reach the
gateway from another device. Only these isolated endpoints are present:

- `GET /v1/worker/health`
- `POST /v1/worker/claim`
- `POST /v1/worker/heartbeat`
- `POST /v1/worker/complete`

Every endpoint requires `X-Dominion-Worker-Token`. No product management,
MCP, filesystem, shell or owner-secret API is exposed on this listener.

The local owner creates and reads work through:

- `POST /api/remote/tasks` (body `{"prompt":"...","maxTokens":2048}`)
- `GET /api/remote/tasks`
- `GET /api/remote/tasks/:id`
- `POST /api/remote/tasks/:id/cancel`
- `GET /api/runs/:id` (same canonical RunView, ExternalAgent surface)

Existing local origin/admin-token policies apply. The Rust and TypeScript SDKs
also expose these routes.

## Remote machine

A remote machine needs only the DOMINION Rust executable, a reachable **HTTPS
gateway or localhost SSH tunnel**, and its own OpenAI-compatible model endpoint.

Set:
- `DOMINION_REMOTE_GATEWAY=https://<private-host>/`
  (or `http://127.0.0.1:7410/` through a forwarded SSH tunnel)
- `DOMINION_REMOTE_WORKER_TOKEN=<same-secret>`
- `DOMINION_REMOTE_WORKER_ID=<stable-device-identifier>`
- `DOMINION_WORKER_MODEL_ENDPOINT=http://127.0.0.1:11434/v1/chat/completions`
- `DOMINION_WORKER_MODEL=<installed-local-model>`
- Optional `DOMINION_WORKER_MODEL_KEY` (stays **on the worker**, never sent to coordinator)

Run `dominion-rust remote-worker`. It claims one bounded text-inference task,
runs inference on its own endpoint and returns one text receipt with reported
usage. No arbitrary executable code, shell, shared workspace access or
provider API key is transmitted by the coordinator.

## Task Team use

A read-only Task Team node can set `"runtime":"remote"`. It shares the
existing lead, dependency DAG, cancellation and read-only evidence contract.
Write tasks or requested native tool grants are rejected by the Team validator.
The Team adapter binds one durable remote ticket per child; parent cancellation
revokes the lease. The returned evidence is `external_unverified`, **not**
a host-verified file mutation.

## Failure and replay

A claim, lease, attempted model call and receipt are durable SQLite records.
One worker owns a nonce-bound 180-second lease at a time. Expired in-flight
work transitions to `needs_review`; it is **never silently requeued**, as a
remote paid model call may have finished during transport loss. An old worker's
late result cannot overwrite owner cancellation or another worker's record.

The 120-second HTTP timeout is shorter than the lease; no prolonged inference
is admitted in v1. Remote write/merge or multi-GPU sharding is outside the
safe read-only remote protocol. Those require separate explicitly reviewed
workspace synchronization, sandboxing and approval contracts.

## Verification

`cargo test --bin dominion-rust api::remote_workers::tests -- --test-threads=1`

`cargo test --bin dominion-rust storage::sqlite::tests::remote_`

`cargo test -p dominion-agent-runtime --lib`

Sub3's normal product acceptance workflow also runs the remote HTTP
authentication/cancel test, lease/receipt tests, SDK checks, and Team
validation. No cloud deployment or credentials are required.
