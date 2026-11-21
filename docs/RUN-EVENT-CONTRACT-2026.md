# DOMINION Run/Event Contract v1 — 2026-11-21

The Rust execution ID is authoritative. SDK run IDs, room IDs, task episode IDs,
job IDs and session IDs are **not interchangeable** with native execution IDs.

- `dominion-core/src/run_event.rs` owns the versioned `RunEventRef`,
  `RunEventEnvelope` and `RunEventPage` Rust types.
- `src/events.rs` attaches `runRef` only where a trusted record contains an
  actual execution ID. The existing wire payload stays additive.
- The SQLite `event_log` owns monotonically increasing global `seq`.
  `GET /api/executions/:id/events?after=42&limit=100` pages events by
  **native execution ID** in increasing, exclusive cursor order.
- `hasMore` indicates more matching events after the page; `nextAfter` is
  the last returned global sequence. Empty pages keep the previous cursor.
- `earliestAvailableSeq` and `historyTruncated` expose the bounded log's
  retention gap. If truncated, fetch authoritative current run state from
  `GET /api/executions/:id` before resuming UI rendering.
- `GET /api/events` still streams the original global event bus. Clients that
  stream must subscribe before replay and deduplicate by `seq`.
- TypeScript SDK `DominionAgentClient.getExecutionEvents()` consumes this
  canonical timeline. `getRunEvents()` resolves an SDK facade to the real
  execution reference, or refuses with 409 until the native run exists.
- SDK presentation status (e.g. `in_doubt`) is not a native execution
  lifecycle claim. Only the native execution status populates `runRef.lifecycle`.
- Room and Task remain distinct orchestrators; neither receives a second
  execution loop. They reuse the shared journal whenever the execution ID
  is present. Events without a trusted execution ID are deliberately
  uncorrelated rather than assigned a fabricated one.

Compatibility: existing `record`, `message`, `approval`, `seq` and streaming
payloads are unchanged. This is an additive query and SDK contract, not a
migration of past event rows. Prior rows without `runRef` remain in the original
global log but cannot be retroactively attributed to an execution.

Tests: Rust typed event roundtrip, per-run scoped replay across restart,
API pagination/404, SDK cursor+identity and unadmitted run guard.

## Room, Team and external worker linkage (2026-11-21)

- A room speaker reply publishes `room_agent_reply` with the true native
  `executionId` **only after** the SQLite participant read-cursor transaction
  advances. Retries or stale readers do not produce a second delivery event.
- Native Worker jobs save `executionId` from the committed assistant message
  only if the referenced native execution is owned by that worker's session.
  A `parentExecutionId` is attached only if the owning Lead team and native
  execution both exist. No worker is merged into the Lead's execution identity.
- The Lead/Worker coordinator emits a bounded `team_outcome` event after
  its terminal job state transaction commits. Its `workerEvidence` points to
  independently verified Worker receipts and does not assert patch integration.
- External CLI inspections preserve a separate `externalRunId`; `executionId`
  on an external receipt is set **only** to a proven owning Lead native
  execution, and `nativeToolReceipts` remains false. This is correlation,
  not delegation of DOMINION's native tool authority to an external CLI.
- Strategy planning returns a serialized, versioned Rust `PlanningDecision`.
  Native execution and permission ownership are unchanged.

## Atomic event sequencing under concurrent workers

The in-process event bus serializes three operations with one shared lock:
assign `seq`, persist with `INSERT` (never replace a previous event), then
broadcast. A failed SQLite write cannot increment the durable cursor or
broadcast a false committed event. All Bus clones share this ordering lock.
The reconnect contract is tested with 6 concurrent producers × 40 events,
followed by process recreation. This does **not** imply multi-process writable
SQLite event bus support; DOMINION currently uses one local Rust server.

## Live external agent control planes (2026-11-21)

Optional `codex-app-server` launches `codex app-server` and speaks
bidirectional line-delimited App Server requests. Optional `qwen-acp` launches
`qwen --acp` and uses ACP session negotiation, prompt updates and permission
requests. Both are opt-in **isolated inspection** workers, not native model
providers or authorization bypasses.

Live progress is journalled as `external_agent_event` with bounded text,
`jobId`, `externalRunId`, and an actual parent `executionId` only when
previously proven. Server-initiated approval requests persist as ordinary
DOMINION `approval` records. Existing `GET /api/approvals` and
`POST /api/approvals` control them; no approval is automatically granted.
Unsupported privilege-escalation RPC methods are denied. Disconnects and
cancellations invalidate pending approval requests. Native tool receipts
are never fabricated for work an external CLI executed in its own process.
