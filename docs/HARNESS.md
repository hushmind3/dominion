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

- On failed tool actions, the adaptation planner proposes causal hypotheses and testable predictions. The Rust diagnostic policy in src/engine/adaptation/diagnostics.rs admits only workspace-local reads that classify every surviving hypothesis. A bounded contingent minimax policy (up to three observation steps, 12 probes, 16 hypotheses) minimizes worst-case remaining ambiguity under the actual remaining tool budget, rather than selecting a single balanced Gini split. A fresh real receipt triggers another policy calculation. The prediction partitions come from the model, never from claims of actual results.
- Each read passes through the normal budget, approval, cancellation, journal and tool-dispatch pipeline. A receipt ID, result digest, compact observation, prediction and actual three-valued verdict are retained as execution.adaptationDiagnosticHistory. Missing observations never refute hypotheses. Four diagnostics is the per-failure ceiling, and one tool slot is reserved for remediation.
- Actual read results can remove contradicted explanations and trigger one additional budgeted model call to recompose remediation actions. Diagnostics and repairs remain distinct; merely reading a file cannot satisfy a mutating goal.
- For ordinary tasks, verification records exactly which acceptance postconditions remain or were resolved after each tool round. execution.acceptanceTrajectory stores up to 12 snapshots. Unchanged gaps mark a stalled task even if tools continue returning successful receipts; the subsequent model step receives the concrete missing condition and evidence-based instruction to change its approach. Identical round snapshots do not duplicate progress. The latest acceptanceProgress holds the sole full proof lineage graph; bounded acceptanceTrajectory entries preserve lightweight gap/receipt deltas. A verified state or repeated unchanged receipts avoids another SQLite operation-history scan.
- The original acceptance verifier continues to own completion. Model predictions, tool transport success, and self-reported progress are never sufficient alone.

Current scope: evidence-conditioned adaptation on tool failures, adaptive diagnostic lookahead and progress-based replanning for ordinary action goals. This is not yet isolated counterfactual rollouts or benchmark-driven automatic harness promotion. Model predictions are not calibrated likelihoods; cross-task improvements and token/cost lift have not been independently measured. No front-end UX or SDK protocol change was made. Focused dev-profile regression checks are in .github/workflows/ci.yml.


## Distinct 2026 evidence-attribution adaptation: obligation lineage

The product runtime does not copy a reference agent's controller stages. Instead,
it uses native Dominion execution receipts, frozen postcondition verification and
the existing effect/approval kernel to reconstruct the smallest **attributable
resource-level repair frontier**.

- New `engine/obligation_repair.rs` links each *actually failing* deterministic
  postcondition to the ordered native `execution_operation` receipts on the
  same typed workspace/file resource. The graph contains failed obligations,
  tool receipts, same-resource temporal links and explicit non-discharge edges.
- A completed mutation is **counter-evidence** if the original verifier still
  fails. More nominally successful calls do not imply progress; a later read
  can show an unresolved contradiction and steer a local replan.
- Unresolved external effects are distinguished from failed mutations, unverified
  mutations, read-only observations and missing receipts; unknown side effects
  require inspection rather than an automatic retry.
- The stored `acceptanceProgress.obligationLineage` is generated from real
  verifier gaps and scoped receipts, not proposed causes. A stagnant run
  receives a resource-specific repair directive; the old acceptance gate
  retains final authority. This path does not auto-promote code or add loops.
- Verification fixtures cover conflicting read/write/read trajectories,
  unknown effects, unrelated targets, shared obligations, and prevention of
  model-invented failures. The focused CI exercises the Rust module directly.

The research context is 2026 harness trace attribution and behavior-aware
verification (HarnessFix 2606.06324 and HarnessLens 2608.27311). This is
a Dominion-specific synthesis over the native effect journal and acceptance
contracts, not an implementation of either paper's optimizer or a transplanted
Assess/Propose/Evaluate/Dispatch controller.

## Combined 2026 harness mechanics — one native adaptation path

This does **not** add another campaign, agent loop, language runtime, or independent
optimizer. It combines three 2026 open-source strengths at the existing
`attempt_model_adaptation` call site:

1. **Harness² (Google Research, 2026)** — use the existing alternative
   intervention proposals, actual discriminating probes, and evidence-conditioned
   revision. The hypotheses and diagnostics are explicitly proposals, not proofs.
2. **DeepSeek Harness PTC (2026)** — every intervention and diagnostic remains
   under the **existing** Rust tool-dispatch journal, approval, cancellation,
   budget and uncertain-effect barrier; a model-selected tool cannot bypass it.
3. **PRISM / Agent Harness Optimizer (Airbnb, 2026)** — before intervention,
   preserve the owner's frozen acceptance checks; after the real tool receipt,
   re-evaluate the **same** checks. A candidate is accepted only when at least
   one previously failed condition is resolved, no previously satisfied
   condition regresses, and the matching native receipt is complete and not
   uncertain. Only observed acceptance influences the live repair/retry path; historical
   algorithm success counters cannot overrule the owner.
   For a non-action conversational turn, the decision is `not_applicable`,
   never a fabricated success.

**Cleanup (2026-11-18):** The live adaptation path no longer calls the old
weighted-sum / expression strategy selector, which relied on unverified
`expectedProgress` and model-authored success assumptions. A single
constrained non-dominated proposal selector now compares surviving causal
explanations, observed predicate status and projected tool cost. The existing
native dispatch and frozen postcondition/no-regression acceptance gate remain
the only proof of a successful intervention. The archived `adaptation.selection` registry remains readable, but
registration, generation and activation are disabled for the retired kind.
UCB1 automatic exploration has also been removed from other algorithm
execution slots. Owner-activated kernels take precedence over
benchmark-validated alternatives and executable built-in baselines;
old statistics remain audit data, not a live exploration reward.
Redundant `diagnosticEvidence` and `acceptanceDecision` payloads were
removed from the lightweight adaptation checkpoint; full evidence remains
available through native operation receipts and the bounded adaptation log.

The compact acceptance record is stored once at
`execution.lastAdaptation.acceptanceDecision`. The bounded checkpoint retains
only its selection cursor and revision. It contains the native
operation ID, resolved and regressed obligations, and the decision. The ordinary
verifier remains the authority for *final* completion; in-task acceptance is
not a substitute for held-out benchmark evaluation or isolated counterfactual
rollouts.

Sources inspected:
- DeepSeek `packages/core/tool-executor-ptc/src/ptc.ts` and `ptc-calls.ts`
- Google `harness2/core/recursion.py` and `selection.py`
- Airbnb `agent_harness_optimizer/optimizers/prism/loop.py` and
  `framework/acceptance.py`

## 2026 native-provenance memory reranking

The live `memory.rerank` registry now exposes
`evidence-lineage-2026` rather than the historical BM25 kernels. During
actual memory retrieval, it checks the persisted native owner/tool message
references of each candidate within the same session, flags counterfeit or
missing source IDs as unverified, and deprioritizes contradicted memories.
Its order is a constraint-aware evidence tier, then lexical coverage,
exact-phrase evidence, and the existing relevance/importance tie breaker;
this is NOT a new invented lexical scorer. No paid model or background pass
is required. Existing BM25 functions, archived records and direct evaluation
fixtures remain for historical comparisons; old BM25 kernels cannot be newly
registered or reactivated through the public algorithm API.

This implementation was motivated by the 2026 provenance/trace validation
research but has no externally validated hold-out completion gain yet.

## 2026 frozen-acceptance obligation graph

The production `task.decomposition` selection now chooses the bounded,
`acceptance-lineage-dag-2026` executor. It converts only the pre-tool-call
frozen acceptance criteria into obligation nodes with stable provenance IDs,
recognised native predicate types and explicitly declared dependency edges.
No keyword-only text split can become a success claim, and a missing frozen
criterion stays `unverified_owner_request`.

The graph is persisted to the original execution and forwarded to the model
as **pending** requirements, never as completed actions. The prior
`qualityScore` (token/segment coverage) is deliberately null for this
kernel. The kernel's usage is marked unrated until separate native tool and
acceptance receipts decide task completion. A repaired or migrated database
retains old decomposition records read-only; the old `single-step`,
`clause-split` and `numbered-first` implementations cannot be registered
or reactivated through the algorithm API. Historical fixture evaluation code
remains to compare old records rather than falsifying improvements.

This is a project-specific implementation influenced by the 2026 evidence,
provenance and held-out validation work in HarnessFix and the AHO/PRISM
protocol. It does not claim independent paper-level benchmark results or
measured real-model lift.

## Hot-path and restart cleanup

- The failure observer now reuses the existing learned-method workspace
  observation. A failed shell command is no longer mistaken for a pathname,
  and one diagnostic attempt does not perform duplicate directory walks.
- The diagnosis result no longer calculates or records an obsolete Gini
  information-gain scalar. Its auditable receipt contains contingent
  lookahead horizon and worst surviving cause count.
- Algorithm candidates are indexed by kind: selecting a kernel for one tool
  or model step no longer reads every historical algorithm record. The
  user's active choice remains authoritative.
- One-time improvement migrations query only records missing compatibility
  fields and do not rewrite completed legacy upgrades on every restart.
- `algorithm_usage` is indexed by execution ID and fetched only for that run
  at finalization; all historic ratings remain available in the database.
- Approval cancellation looks up only pending approvals. Durable reuse looks
  up only decisions scoped to the same session, task/revision, operation and
  argument digest, then fetches original runs individually to prove that
  the old invocation was interrupted. The compare-and-swap consume gate stays.
- Startup migration fetches only live executions, tool operations and pending
  approvals using existing indexed queries. Terminal history is untouched.
- The routine Rust CI includes focused tests for obsolete algorithm
  rejection, policy experiment gating, indexed approval, migration and
  shared observation. Structural rules flag full-history scan regressions.

## Archived algorithms and API compatibility

- The retired `adaptation.selection` and `selfimprove.search` rows remain visible in
  `GET /api/algorithms` with `retired: true`; new registrations, generated
  candidates and activation are rejected.
- Explicit owner activation takes precedence; validated alternatives and
  active executable baselines are retained. No UCB1 runtime exploration remains.
- Archived RSI campaigns and completed media files remain retrievable
  through read-only API routes; retired mutation/production workers stay off.

### Removed unreachable optimizer loops

The old code-only policy search proposer and optimizer meta-improvement loop
were not called by production API or the running native engine. They have been
deleted rather than retaining another campaign state machine. The existing
benchmark-only `policy_search` endpoint is gated by `DOMINION_BENCHMARK=1`,
and independent powered experiment review still controls policy activation.
Recorded `optimizer_policy`, `search_episode`, and experiment rows remain
readable. Legacy `selfimprove.search` algorithm records are likewise archived,
never seeded/activated/generated. No stored user or experiment rows were deleted.

## Retired subsystems

Native FFmpeg/ComfyUI video composition, narration generation, scene regeneration, YouTube auto upload, RSI campaigns, source-mutation generations, autonomous grading, automatic core adoption and associated external runtime orchestration are retired. Existing media files and job records remain available through read-only archive endpoints. New media creation belongs to explicit MCP integrations or external APIs, not a built-in engine.

## 2026 prompt-cost and context residency improvements (2026-11-18)

DOMINION distinguishes provider-reported prompt tokens, provider-reported cache hits and uncached input from estimated WebSocket wire payloads. Wire delta is **not** evidence of lower billed token cost. The Codex WebSocket adapter now emits `incrementalTransport.rebaseReason` for a fresh chain, changed prefix, leading-history eviction, transport recycling and reordered context. **Leading-window eviction is still a full resend:** silently keeping a remote chain after a user or owner context update could revive stale instructions. This instrumentation enables an exact breakdown of why expensive rebuilds occur without weakening owner revisions, context-epoch boundaries or effect safety.

Enabled skills use progressive disclosure: the dynamic context advertises a deterministic brief index of visible skill IDs, names and descriptions. `skill_view(id,startLine,maxLines)` loads exact stored instructions on demand through the native read-only tool gateway; draft/disabled/out-of-scope skills cannot be fetched. Tool-free/model-only turns retain the previous bounded instruction excerpt so they do not lose their only source of skill guidance. Unadvertised skills do not enter the context dependency watermark. Full SKILL.md bodies remain in SQLite/files and are not deleted by prompt selection.

Room peer context selection pins only the newest 1–2 raw contributions according to task complexity and admits additional contributions through relevance, recency and token budget. Each participant's last self response, owner instructions, durable memories and tool-call/result pairs remain protected. The 2026-11-09 100-turn room benchmark (`benchmark/ROOM-100-TURN-LUNA-2026-11-09.json`) had 18.15% provider-reported cache reuse and 0/9 long-range recall at several checkpoints, but it was a different branch/revision. No claim of improved provider cache hit rate, recall, cost or latency is made until matching live-model trials cover the revised main build.

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
