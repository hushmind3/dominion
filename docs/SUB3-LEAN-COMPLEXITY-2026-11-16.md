# Sub3 architecture transition: lean autonomous Rust harness

Date: 2026-11-16. Branch: `dominion-sub3` only.

## New owner direction (supersedes earlier video/RSI feature-growth objective)

The product should center on a **general Rust agent harness**, not an internal creative-production studio or a continuous self-modifying research platform. Keep request-driven programming, inspection, testing, verification and external MCP tool integration. Do not transplant RSI generation loops, arbitrary self-grading or automatic core adoption into `dominion-agent-sdk`. Preserve chat, memory, files, user corrections, verified task history and durable task receipts; historical SQLite records must remain readable.

The current judge specification still allocates 15 points to engine/RSI and 10 points to native video, and imposes an 80-point ceiling when those paths disappear. **The judge rubric must be revised separately for the lean product target**; this branch does not pretend the old 100-point score remains directly comparable.

## Implementation stage A — applied in Rust product

1. **Idle worker efficiency**: `Store::active_jobs()` in `src/db.rs` performs a SQLite expression-indexed query for only queued and running jobs. `jobs::next_queued` no longer clones and sorts all historical completed, failed and cancelled jobs every 400 ms. The existing room serialization, concurrency guards and CAS job claims remain intact. The index is created automatically for legacy SQLite files, without rewriting user records.
2. **Wake-on-work**: `AppState.job_wakeup: Arc<tokio::sync::Notify>` is signaled by `jobs::enqueue`/`enqueue_once` to wake a worker without 400-ms repeated scans. A 5-second rescan catches external writers, older checkpoint resumption and deferred work. This leaves SQLite authoritative and avoids treating an in-memory signal as a durable job.
3. **No default learning**: `chat::TurnRequest::from_value` and the existing Chat UI default `autoLearning` to `false`. Explicit user opt-in can still trigger the legacy recorded-memory workflow; the normal user request does not spend post-turn model tokens automatically. A Rust unit regression tests the distinction.
4. **Stop unattended campaigns**: the unconditional five-minute `retry_failed_learning`/agent-growth/distillation/learning sweeps have been removed from the worker startup path. Regular scheduled tasks and stale approval deadlines still run every 30 seconds, because those are explicit operational obligations rather than autonomous RSI.
5. **No spontaneous policy mutation after chat**: `chat/runtime.rs` no longer schedules policy experiments/candidate adoption on every finalized execution, nor creates new strategy proposals from reviewer or repeated-tool statistics, nor writes agent-growth or learned-principle candidates on terminal runs. It **still writes task outcome, failures, verified successful experiences, receipts, usage and operator correction evidence**; the current-turn reviewer can still repair a failed acceptance test.
6. **Tests**: `src/db.rs` checks an indexed view over 500 mixed-status jobs, preserving FIFO and active jobs; `src/engine/chat/tests.rs` checks default-off learning and explicit opt-in; pre-existing job/room CAS tests still exercise serialization. Product parity allows only the chat default change and verifies the work API is preserved.

## Stage B — pending, not represented as completed

The new owner target requires a dependency-first removal of all built-in video/audio production and independent RSI **from reachable runtime**. Current `src/engine/chat/runtime.rs` still contains RSI-only branches, `src/engine/jobs.rs` still has legacy video/RSI job-kind dispatch and recovery branches, `src/api/mod.rs` exposes old video/RSI endpoints and `web/src/App.tsx` still loads Video and SelfImprovement screens. Their module declarations, transitively referenced handlers, TypeScript SDK types, scripts, README sections and video-only library dependencies must be separated before deleting files. No cosmetic menu hiding or no-op shim should be presented as full removal.

The ordinary conversational SSE pipeline, active user sessions, room turns, user memory, file operations, MCP, approval, cancellation, execution journal and native agent SDK need end-to-end regression on each removal step. Historic video/RSI records must continue to be accessible as archived data, but their old jobs must not silently resume causing effects. A migration flag or read-only legacy inspector is preferable to deleting stored records.

## Expected measurements and limits

Before change, 4 workers each queried all jobs once per 400 ms while idle (theoretical **~10 scans/sec**, each O(total jobs)). After change, in-process enqueues wake immediately, and idle reconciliation is at most about **0.8 indexed active-job queries/sec** across four workers (one query per worker per 5 seconds) in the absence of notifications. **This is a static scheduling estimate, not a measured CPU benchmark.** A small number of newly queued jobs is still processed using the original CAS claim and room/learning gates. The queue index is a DB optimization; read throughput, actual CPU and execution quality need a controlled before/after run.

The new product shape does not rely on RSI experiments or local model training. This code state is **stage A**, not the final full source purge. Judge evidence should cite the exact CI head, the code and the scope that is still pending.

Reference: Tokio `Notify` semantics and single-permit wakeups ([docs](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html)). The design uses durable SQLite plus advisory wakeups rather than relying on an in-memory signal for recovering jobs.
