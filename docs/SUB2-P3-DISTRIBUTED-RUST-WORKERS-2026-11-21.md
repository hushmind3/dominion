# DOMINION sub2 — P3 native distributed Rust Workers (2026-11-21)

Branch: dominion-sub2 only. The existing Rust Harness, React/Tauri UI, SQLite event journal and native Job engine remain the only execution owners. The Cloudflare account/model gateway is a separate product from this Rust worker-control plane.

## Executable design

Coordinator (machine A): Axum/Tauri desktop stays localhost-only. A separate optional remote-only Rust listener exposes five authenticated endpoints. The leader's managed Task Team persists remote Worker identity and task receipts in SQLite.

Worker (machine B/C): the same dominion-rust binary runs in outbound-only remote-worker mode. It polls an authenticated private/HTTPS coordinator, claims a durable task, and performs the work using its own native Rust Job and Harness. It never hosts an inbound HTTP listener or creates a second model loop.

Data flow:

    React/Tauri/SDK -> Lead / Task Team -> SQLite remote_task
                                      -> Remote-only Axum listener
                                           <- Remote Rust Worker polling
                                           <- Worker registration + heartbeat
                                           -> Task payload + leased identity
                                           <- Native Job status + evidence
                                      -> SSE journal -> Lead follow-up Worker

## Implemented code

- src/remote.rs: isolated authenticated listener, registration, node inventory, capability checks, deterministic task submission, SQLite CAS claim/renew/settle, epoch fencing, cancellation and uncertain outcome review.
- src/remote_worker.rs: executable outbound CLI, local agent discovery, persisted epoch, deterministic native Job/session IDs, local-only read policy, heartbeat, cancellation and bounded result reporting.
- src/bootstrap.rs: only opt-in DOMINION_REMOTE_LISTEN starts the separate port. A remote-worker CLI subcommand selects outbound execution.
- src/engine/services/tools/tools_team.rs: team_workers exposes remoteWorkers; team_execute accepts a per-task remoteWorkerId; normal DAG ordering, native success criteria and source-addressed predecessor results still apply.
- src/state.rs, src/db.rs, src/events.rs: existing owner cancellation, CAS transaction, persisted journal and typed SSE event contracts. No new UI/API server replaces them.

## Coordinator configuration — private endpoint

Generate the same long random token for all trusted nodes. Never commit this secret or include it in agent prompts.

    export DOMINION_REMOTE_CLUSTER_TOKEN="$(openssl rand -hex 32)"
    export DOMINION_REMOTE_LISTEN="127.0.0.1:4941"
    ./dominion-rust

For another computer to reach this port, expose only the remote listener over a properly authenticated HTTPS reverse proxy or private encrypted network interface. The desktop UI and ordinary API must not be published. Bind to your actual private VPN IP instead of loopback when necessary. The cluster secret must be 32–512 bytes.

Remote-only endpoints (POST, JSON with x-dominion-remote-token header):
/v1/remote/register, /heartbeat, /claim, /renew, /settle.

## Remote Worker configuration — other PC

Configure local agent profiles, provider credentials and a separate native workspace/data directory on the Worker PC first.

    export DOMINION_REMOTE_COORDINATOR_URL="https://your-private-controller.example"
    export DOMINION_REMOTE_CLUSTER_TOKEN="THE_SAME_RANDOM_SECRET"
    export DOMINION_REMOTE_WORKER_ID="worker-b"
    ./dominion-rust remote-worker

To use encrypted private overlay HTTP instead of HTTPS, explicitly set DOMINION_REMOTE_ALLOW_PRIVATE_HTTP=1 on that Worker; do not use it on ordinary public networks. Changing DOMINION_REMOTE_WORKER_ID or its SQLite data directory changes the execution identity. Remote file-changing work is off by default; enabling it requires DOMINION_REMOTE_ALLOW_WRITES=1, and native local permissions/approvals still apply.

## Lead job request with a genuine remote target

Call team_workers first. Pick an actual remoteWorkers entry and an agent ID that it advertises. Then the existing native tool team_execute accepts:

    {
      "tasks": [
        {"id":"remote-research","agentId":"ACTUAL-REMOTE-AGENT-ID",
         "remoteWorkerId":"worker-b","prompt":"Inspect code and report evidence",
         "readOnly":true},
        {"id":"local-implementation","agentId":"ACTUAL-LOCAL-AGENT-ID",
         "prompt":"Implement using source-addressed research",
         "readOnly":false,"dependsOn":["remote-research"],
         "successCriteria":[{"type":"command_succeeded","command":"cargo test"}]}
      ],
      "wait":true,
      "timeoutSeconds":120
    }

Workers on different PCs have distinct local filesystems. The protocol returns bounded answers and verification evidence, not an automatically copied Git patch. A shared repository change requires an explicit Git/workspace synchronization step; remote text is never promoted to a local source modification.

## Recovery and ownership

1. Every worker registers a stable node ID and persisted epoch UUID. A replaced epoch cannot settle an older lease.
2. Every claimed remote task receives a coordinator-issued lease UUID and a 45-second deadline, stored under SQLite CAS before any native job starts.
3. A worker heartbeats and renews about every 12 seconds; an unexpired lease allows reconnect to the same task. A deterministic local native Job ID prevents double submission.
4. An expired lease or unknown external effect becomes needs_review, not a second mutation on another machine.
5. Owner cancellation stops queued remote work, signals the active Worker via renewal, and the Worker uses the native Job cancellation path.
6. Completion retains the Worker ID, local native Job ID, verification metadata, operation count, and a short source-addressed answer; evidenceOrigin=remote_worker_reported distinguishes remote claims from proof of changed local files.

## Tests and honest completeness boundary

- cargo test --lib remote::tests
- cargo test --lib remote::worker::tests
- cargo test --lib engine::tools::team::tests
- node --test scripts/sub2-architecture-contract.test.mjs
- Existing UI / TypeScript SDK type checks are unchanged.

The included Rust test exercises an actual localhost TCP HTTP registration-claim-renew-settle cycle against persistent SQLite, plus stale ownership, writer consent and expiry. Operational testing with 2 physical PCs, model/API credentials, native approvals and long-running sessions still requires real hardware and connections. No release/installer build was performed in this iteration.


## P3 cross-machine remote Writer artifact transfer and approved merge

Remote Task Team nodes may opt into a bounded set of UTF-8 source artifacts. A remote Writer can return files as real SHA-256 checked receipts, and the Lead can apply them to its own workspace through the existing approved Rust multi-file patch executor, **not** by accepting an assistant-written claim that files changed on the coordinator.

Example native Lead DAG node:

    {"id":"remote-change","agentId":"configured-remote-agent",
     "remoteWorkerId":"worker-b","readOnly":false,
     "prompt":"Edit src/example.rs and provide the requested native artifact",
     "artifactPaths":["src/example.rs"]}

Flow:

1. The coordinator resolves and freezes the local base SHA-256 for every named path when it commits the remote task. The remote node can run only with explicit Writer opt-in and the existing per-tool policy.
2. The worker includes only listed non-symlink files (up to 8 files, 64 KiB each), UTF-8 bytes and SHA-256 in its terminal receipt. Arbitrary paths, Git internals, environment files and path traversal are refused.
3. The coordinator checks every artifact ID, path, size and actual SHA-256 against the fixed approved manifest; it stores bytes as an explicitly remote-reported observation.
4. team_status({teamId}) shows artifactApplyStatus=pending and exact paths. It does not silently sync files or mistake remote agent's native verification for local merge verification.
5. After ordinary Dominion user approval, the owning Lead runs team_remote_apply({teamId,remoteTaskId}). The Rust tool holds the native workspace mutex and SQLite Writer lease, compares every coordinator file's current SHA-256 against the frozen base, stages a multi-file Codex-style patch through the existing tools_patch executor, verifies final SHA-256, records the originating run ID, and marks the receipt applied.
6. If the coordinator's files changed, or the patch result is unknown, the tool returns a conflict or needs_review; no silent overwrite or automatic rerun is permitted. A second successful application returns the saved receipt, not a second write.

Line-based native patches require UTF-8 LF source and outputs with preserved final newline, or an empty file; unsupported line ending conversions are rejected during preflight. Remote file changes remain distinct from remote task model success and from any Git commit. Applying artifacts never implicitly publishes a repository.

CI runs cargo test --lib engine::tools::remote_artifacts::tests in addition to native remote/Team tests. These tests cover frozen base hashes, path confinement, a real local file application, durable applied state and idempotency.
