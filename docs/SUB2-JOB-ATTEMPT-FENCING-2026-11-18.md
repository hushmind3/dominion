# Sub2 — cross-process native job attempt fencing (2026-11-18)

## Problem and evidence
- `src/engine/jobs.rs::JobContext::checkpoint` previously guarded only `status` and `cancelRequested`, so a late async response from job attempt N could overwrite status/results of newly started attempt N+1.
- `src/db.rs::Store::patch_if` used a process-local Mutex with a separate SELECT/UPDATE. Two independent SQLite connections could both read `queued` before either committed.
- `cancel_job` signalled the session before the CAS and returned `Ok(current)` when cancellation lost a race, potentially misreporting success.
- Sub3 `Store::checkpoint_running_job` and `Store::patch_if` use SQLite IMMEDIATE transactions and captured attempt generations. OpenHands uses explicit conversation state transitions and durable event logs; Qwen Code persists intent/receipt/checkpoint before resuming managed tool calls.

## Change
- `Store::patch_if` now performs compare+patch inside SQLite `TransactionBehavior::Immediate`, including multi-process access to the same on-disk DB.
- New `Store::checkpoint_running_job` rejects stale attempts, cancelled/terminal jobs and identity/cancellation-field overwrites. The existing nested `videoState` compatibility merge remains, while the narrow deferred path may return a running attempt to queued with its attempt refund.
- `JobContext` captures `attempts` once at worker claim, and uses the same store primitive for every progress and terminal checkpoint. New tasks, Money jobs, room jobs and SDK-created prompt jobs share the same host policy.
- `cancel_job` commits cancellation before signalling the live session, and accurately returns HTTP conflict through the existing adapter on lost CAS races.
- Test job fixture now claims running before invoking the native worker, matching actual dispatcher behavior.

## Verification
- Focused Rust debug-profile regression tests for independent Store instances, stale worker after requeue, cancellation race, and deferred admission; no cargo build/check or frontend build.
- The full engine can still persist certain chat messages through session-owned paths independently from the job transaction. A later change should fence these as well before claiming atomic job+message terminality.
- Model time, token/cost, and real-task completion rate are not empirically measured by this source-level correctness patch.

## Deferral/retry epoch correction
- A deferred job intentionally refunds `attempts`, so an attempt counter can repeat (1→0→1). The latest claim now increments separate `runEpoch` monotonically (both dispatcher `next_queued` and delegated `start_now`).
- `JobContext` snapshots both values, and SQLite compares both inside the same immediate transaction. Existing job records without `runEpoch` default to 0 for backward compatibility.
- Focused Rust tests prove an old context cannot finish a newly resumed worker even when `attempts` is identical.
