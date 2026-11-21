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
