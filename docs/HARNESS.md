# DOMINION Rust agent harness — current contract (2026-11-17)

## Execution graph

`Chat / Rooms / Tasks / Workflows / SDK → product adapter → dominion-core → model / MCP / tool drivers → SQLite journal`

- The **Rust core** owns execution stages, tool admission, budgets and replay-safe receipts.
- Product adapters own provider transport, task context, verification evidence and storage.
- Tools are gated by approval, read-only delegation, cancellation and remaining budget.
- Side-effect results that cannot be confirmed after a crash are marked **needs_review**, never silently replayed.
- User-requested source edits follow ordinary `inspect → edit → test → verify → report`; no RSI campaign or auto-adoption loop.
- Explicit user corrections, verified procedures, failures and existing chat/file/task records remain persistent.


## Evidence-conditioned agent execution (2026-11-18)

The Rust product path preserves real task-verification evidence across attempts.

- On failed tool actions, the existing adaptation planner may propose competing causal hypotheses and diagnostic predictions. The separate pure decision logic in src/engine/adaptation/diagnostics.rs admits only workspace-local reads with differing hypothesis predictions. It selects a cost-aware informative read, not the historically most successful candidate.
- Each read passes through the normal budget, approval, cancellation, journal and tool-dispatch pipeline. A receipt ID, result digest, compact observation, prediction and actual three-valued verdict are retained as execution.adaptationDiagnosticHistory. Missing observations never refute hypotheses. Four diagnostics is the per-failure ceiling, and one tool slot is reserved for remediation.
- Actual read results can remove contradicted explanations and trigger one additional budgeted model call to recompose remediation actions. Diagnostics and repairs remain distinct; merely reading a file cannot satisfy a mutating goal.
- For ordinary tasks, verification records exactly which acceptance postconditions remain or were resolved after each tool round. execution.acceptanceTrajectory stores up to 12 snapshots. Unchanged gaps mark a stalled task even if tools continue returning successful receipts; the subsequent model step receives the concrete missing condition and evidence-based instruction to change its approach. Identical round snapshots do not duplicate progress.
- The original acceptance verifier continues to own completion. Model predictions, tool transport success, and self-reported progress are never sufficient alone.

Current scope: evidence-conditioned adaptation on tool failures plus progress-based replanning for ordinary action goals. This is not yet isolated counterfactual rollouts or benchmark-driven automatic harness promotion. Model predictions are not calibrated likelihoods; cross-task improvements and token/cost lift have not been independently measured. No front-end UX or SDK protocol change was made. Focused dev-profile regression checks are in .github/workflows/ci.yml.

## Retired subsystems

Native FFmpeg/ComfyUI video composition, narration generation, scene regeneration, YouTube auto upload, RSI campaigns, source-mutation generations, autonomous grading, automatic core adoption and associated external runtime orchestration are retired. Existing media files and job records remain available through read-only archive endpoints. New media creation belongs to explicit MCP integrations or external APIs, not a built-in engine.

## Provider connection reuse

All ordinary chats, room turns and SDK invocations use pooled reqwest transports on cloned AppState handles, keyed by the existing per-request timeout policy (room and standard). This reuses sockets and TLS negotiation; bearer credentials remain attached to individual model requests and are not cached in shared state. The same provider fallback and request deadlines remain in force.

## Background behavior

Ordinary chat and rooms default to `autoLearning: false`. Owner corrections remain independently recordable. One bounded event-driven dispatcher wakes on queued jobs; a ten-second fallback reads only indexed queued/running records to protect durable work from missed events. Unrelated bus traffic never triggers dispatch scans. The 30-second automation and approval maintenance pass uses indexed SQLite queries for due/enabled automations and pending approvals rather than deserializing all historical records. Due automation claims enqueue their job and advance the next execution time in the same SQLite transaction, preventing duplicate launches across concurrent ticks and process crashes. Restart recovery scans only active jobs and running executions/tool operations; uncertain effects are detected in a single SQL join. Completed history stays untouched and in-flight tools are held for review. Deterministic experience guidance and distillation run only after changed evidence rather than constantly reevaluating untouched history. Free-model catalog refreshes run only via the explicit Models-page action; an idle server does not poll every provider's model endpoint. No RSI autonomous sweeps run.

## Durable job terminal claims

A job's terminal status is committed only while it is queued or running and has
not been cancelled. Terminal audit events are recorded after the durable write,
including any delegated worktree patch receipt; a late completion cannot
overwrite cancellation or an already finalized run. Failed persistence records
`job.finalization_failed` instead of falsely reporting success.

## Durable room progress

A room speaker's completed response now records elapsed milliseconds in its durable output and publishes a `speaker_finished` event containing the room/agent IDs, elapsed time and step count. The room still follows its existing sequential participation and shared-owner-message semantics; this instrumentation does not change UI routing or its model choice. It supports diagnosing a slow participant instead of guessing a fixed whole-room timeout.

## Evidence-based SDK completion

SDK startup stores the run and its session (including budget owner, token/price limits and workspace scope) in one SQLite transaction. Neither an unbounded orphaned session nor a queued run without its configured budget can be created by a partial commit.

Manual tool reconciliation is a single claim: the SDK owner must be failed
or interrupted, the latest operation must still be unobserved, and both facts
are verified within the same SQLite transaction that saves the observation
and native journal changes. Concurrent or repeated operator decisions cannot
replace the first recorded effect, and already-completed/cancelled runs cannot
be reopened by a stale reconciliation request.

SDK `in_doubt` status takes precedence over `failed` and `interrupted` whenever the latest native run retains an unknown tool effect. `resume` fails closed until the operator reconciles the effect; terminal completed/cancelled history is not rewritten by the read projection.

SDK tool approvals require a live, uncancelled owner run and propagate resolver failures. Cancellation first atomically claims the SDK run's live status, then signals the shared session epoch; a late cancellation never overwrites completed history or silently stops a finished job.

A single SDK run's poll reads only that session's latest execution, pending
approval and unknown tool effects through dedicated SQLite indexes. Global SDK
listing still batches its projection once; a session-less legacy record stays
read-only without constructing a global index.

The SDK retains the model reply for inspection but exposes the terminal `taskOutcome`, `verified`, and `acceptancePassed` from the canonical native execution. SDK status views query only the latest execution of SDK-owned sessions and unknown associated tool effects, using SQLite partial indexes rather than deserializing full historical execution/tool collections on every UI poll. A requested verification command without independent success evidence cannot return a successful SDK status. Terminal writes use a status compare-and-patch so cancelled runs and owner edits cannot be overwritten by an obsolete completion. On restart, completed SDK runs are settled from their persisted execution verdict, not answer text alone. Legacy conversational clients still receive a `completed` status without an explicit verification request, alongside `verified: false` for unproven outcomes.

## Regression contract

Routine main pushes run focused Rust tests in the development profile on macOS. The release workflow runs full tests and desktop builds on Windows and macOS. Linux is not a product test target. A commit is verified only after its relevant focused tests pass; release packaging is checked separately.

## Git metadata and scoped acceptance evidence

Source builds do not watch `.git/HEAD` or moving refs as Cargo inputs. An
explicit packaged release can embed `DOMINION_RELEASE_COMMIT`,
`DOMINION_RELEASE_BRANCH`, and `DOMINION_RELEASE_DIRTY`. A normal local source
checkout reports its observed Git revision once at runtime with
`identitySource: source-checkout`, rather than claiming to embed that revision.
Ordinary chat acceptance reads `execution_operations(runId)` only for relevant
executions, not the entire historical tool log.
