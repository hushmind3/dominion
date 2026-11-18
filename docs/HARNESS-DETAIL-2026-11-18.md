# Implementation notes moved out of the README (2026-11-18)

These are the harness, Money and SDK notes that used to sit on the README front page. They are kept
verbatim so the detail stays in the repository while the README leads with what a visitor needs first:
what the product is, how to install it, and what it does and does not claim.

The contract the engine is held to is [HARNESS.md](HARNESS.md); the map of every document is
[INDEX.md](INDEX.md).

---

## Native Rust execution harness (2026)

DOMINION does **not** use a lexical retrieval score as its agent controller.
The [`dominion-core` Rust engine](../crates/dominion-core/src/engine.rs) owns the
bounded **initialize → plan (when needed) → model → tool admission/execution →
verification → completion** loop. The product adapter in
[`src/engine/chat/product.rs`](../src/engine/chat/product.rs) connects that loop
to the actual provider, user instruction, workspace, approvals, SQLite
receipts and frozen acceptance conditions.

| Execution layer | Actual implementation |
| --- | --- |
| **Kernel / safety** | [Shared driver](../crates/dominion-core/src/engine.rs), [native snapshots and tool intents](../crates/dominion-core/src/hosted.rs), [shared budgets](../crates/dominion-core/src/budget.rs); cancellation, outstanding approval or unresolved tool/child effects block false completion. |
| **Planning / control** | [Versioned executable algorithms](../src/engine/algorithms.rs), including `acceptance-lineage-dag-2026` for task decomposition, selection of applicable strategies and budget-aware tool use. Algorithm records select real Rust kernels; old experimental records are not an autonomous self-modifying engine. |
| **Verification / repair** | [Frozen postcondition verifier](../src/engine/verification.rs), [resource-scoped obligation frontier](../src/engine/obligation_repair.rs), [receipt-based recovery](../src/engine/recovery.rs) and [bounded causal diagnostics](../src/engine/adaptation/diagnostics.rs). Completed mutations are not blindly replayed. |
| **Memory / context** | [Evidence-lineage memory reranking](../src/engine/memory.rs), [owner-confirmed room state](../src/engine/room_state.rs), [source-aware room compilation](../src/engine/chat/room_context.rs), and [lazy skill inspection](../src/engine/tools.rs). Lexical search is a candidate retrieval component, **not the harness policy**. |
| **Provider / token reuse** | [Provider-specific request assembly](../src/engine/chat/provider.rs), [Codex incremental WebSocket](../src/engine/chat/codex_ws.rs) and [continuity policy](../src/engine/chat/continuity.rs). History eviction and owner edits may require a safe full rebase; transport byte estimates and provider-billed cache tokens are recorded separately. |

**Verified-checkpoint completion:** after real tool effects satisfy every frozen
postcondition, the kernel can finish without buying an extra model round.
That shortcut requires attributable persisted receipts, no unresolved effects,
a current owner revision and cleared standing procedures. The final response
is published only after its durable message write. This is not a blanket claim
that every successful tool call completes a task.

**Current limitations:** the 2026-11-09 nine-agent / 100-owner-turn benchmark
[`ROOM-100-TURN-LUNA`](../benchmark/ROOM-100-TURN-LUNA-2026-11-09.json)
reported 18.15% provider cache reuse and failed long-range recall on its
older experimental branch. Recent code changes have **not** yet established
a new cache hit rate, recall improvement, cost saving or live-model success
rate. Source-level changes and mocked regressions should never be described
as an end-to-end performance result.

The existing React UI, product SQLite data, OAuth credentials and external
MCP contracts are preserved. Native video/audio creation and perpetual RSI
campaigns have been retired; existing records remain readable, while requested
code changes use ordinary inspect → edit → test → verify tasks.
For execution recovery, provenance and acceptance invariants, see
[the detailed harness contract](HARNESS.md).

### Group-room recall without resending confirmed facts

Room turns use a **proof-carrying append-only working view** in
[`continuity.rs`](../src/engine/chat/continuity.rs). The current question may
select a different subset of existing owner-confirmed facts. When those facts
are still valid in the persisted `room_state` evidence ledger, the runtime
keeps the previous prefix and sends only the newly selected verbatim fact
lines. Adding a genuine new owner fact is also appendable if every previously
visible fact is unchanged. **Corrections, supersession, revoked evidence,
scope changes, modified system instructions, and history-budget rotation**
force a fresh source-scoped projection instead of silently reusing stale text.

This also applies to **tool-heavy group work**: the native-work fallback
carries the room owner-state provenance, and legacy projections without a
verified revision proof are rebuilt once. Original owner messages and tool
receipts remain in SQLite; the optimization changes the model-visible
projection, not durable memory. The
[`owner_fact_selection_reuses_prefix_and_appends_missing_facts_until_real_revision`]
(src/engine/chat/continuity.rs) regression verifies append-only reuse,
deduplication and correction invalidation.

**Measurement boundary:** a reusable prefix is not proof of a provider cache
hit. Per-execution `providerPromptTokens`, `providerCachedPromptTokens`
and `providerNewPromptTokens` are populated only from usable provider
accounting. Real-model nine-speaker / 100-turn memory accuracy and new-token
reduction must be measured separately.

---

## Development vs release validation

Normal Rust source pushes run only scoped, default-profile kernel/job/Money regression tests on macOS via `.github/workflows/ci.yml`. They do **not** trigger frontend builds, `cargo build/check`, release-profile builds, installer packaging, or the Windows/macOS release matrix. The separate `.github/workflows/release.yml` performs those full checks only for version tags or an explicit manual release request. Passing scoped tests does not imply that a distributable desktop release has been validated.

## 💰 Money intelligence

Money cards also allow the owner to explicitly authorize a prototype implementation through a thin Money approval endpoint that atomically binds the candidate to the **same native SDK execution path** used elsewhere in the product. This binds the selected agent persona, preserves a generic source link across reloads, and exposes native approvals/cancellation/recovery without introducing a separate Money runner. Public sales, purchases, outreach and deployment are outside that MVP authorization.


Money discovery collects public RSS, GitHub and Hugging Face signals with the existing Rust worker queue and stores the evidence in indexed SQLite. It produces source-backed business hypotheses without model tokens and offers opt-in AI inference through the existing durable job kernel. Research never authorizes outreach, spending or implementing a business.

[Money architecture and limitations](MONEY.md). No separate execution kernel or autonomous source-code modification system is introduced.

---

## What is DOMINION?

**DOMINION is an AI agent tool, not another AI model or a token-reselling service.**

Connect a model you already have access to — including supported cloud APIs and locally hosted models — and use it inside an agent workspace. DOMINION's Rust harness handles tool execution, permissions, durable tasks, long-term memory, and multi-agent coordination. Native media generation and autonomous RSI campaigns have been retired; use external MCP tools for media generation and ordinary verified agent tasks for code changes.

You choose the models. You control the credentials. You run the workspace.

**Designed for developers, local-LLM enthusiasts, and people who want to experiment with their own autonomous workflows.**

---

## Optional Money tool

The Money research capability can be enabled or paused in **Settings → Money tool**. New installations start with the tool disabled; existing installs with Money data keep their prior on state. Pausing stops its background RSS, catalog and AI jobs and keeps archives and separately approved SDK work. It uses the normal Rust harness and the same SQLite DB; no dedicated Money model runner or DB service is created. Existing source collection now links evidence found in the current ingestion cycle rather than waiting for the next scheduler turn.

Money retains even rejected and deferred opportunities. The collapsed **전체 후보·보류 기록** panel searches all historical candidates with bounded SQLite pages, displays saved source URLs and allows owners to defer or reopen ideas for research without model tokens.

### Retry-safe native SDK jobs

The shared `POST /api/sdk/runs` accepts an optional `idempotencyKey` (up to 128 ASCII letters, digits, hyphens, underscores, dots or colons). A repeated identical request with that key returns the original persisted SDK run rather than starting a second model/tool execution. Using a key with changed arguments returns HTTP 409. The claim, session and run share one SQLite transaction. Older clients without this option retain their behavior.

### Indexed SDK completion and reconciliation evidence

A native SDK run now reads its last execution by session ID using the existing SQLite session index instead of deserializing all prior executions. Operator reconciliation selects only the native frames for the exact execution ID; a dedicated partial SQLite index backs that lookup. Evidence read failures remain explicit and cannot be silently treated as verified completion. Session records, API fields, approval boundaries and model selection are unchanged.

If post-completion SQLite evidence retrieval fails, the SDK marks the run `needs_review` instead of allowing blind model/tool replay. The existing resume endpoint can then **recheck only already-persisted completed execution evidence**; it never runs an extra model round in that state. If evidence is still missing or an effect is uncertain, the run remains paused for inspection.

Money evidence is source-versioned: stored page revisions are content-addressed and immutable, and model-proposed quotations are checked against archived page text without extra model tokens. The Money candidate and archive screens show each quote status and offer on-demand access to previously captured page versions. A quote match is source-text provenance, not validation of payment or buyer intent.

Money can also show an **indicative won-denominated price spread** when two independent archived sources include exact quoted prices, comparable dates/specifications/quantities and complete shipping, duty, platform, returns and labor costs. Currency conversion requires independently archived FX quotations; unknown figures remain null rather than guessed profit.

### Role-scoped delegated jobs (2026)

The existing `delegate_task` tool now accepts an optional `profile`: `general` (backward-compatible default), `inspection` (read-only observation), `implementation` (isolated code changes), or `verification` (observation and approved Cargo tests). Task-specific profiles require `isolatedWorktree: true`; child jobs inherit the shared native Rust driver, approvals, cancellation, and hierarchical token/cost budget. Admission is checked again during progressive tool discovery and final tool dispatch, not just in the model-visible catalog.

`task_result` returns up to 20 **most recent successful** tool receipts for the owned child session via an indexed SQLite query, rather than scanning the global journal. Waiting parents subscribe to exact job events with a bounded durable-state fallback. Existing SDK, GUI, task and stored job interfaces remain unchanged; these improvements have Rust regression coverage but not a measured live-model completion-rate uplift.

### Benchmark timeouts and room usage diagnostics

The repository's `dominion-benchmark` runner now explicitly cancels a trial's server-side session when its SSE transport times out or breaks. It confirms the execution has reached a terminal state before resetting fixtures or beginning another arm; inability to confirm fails the experiment closed. The recorded `trial.json` includes the submitted task, cancellation evidence, provider token accounting and prompt-context metadata. This change does **not** alter any separately maintained external ZIP runner.

Room exchange outputs now include `outputs[].modelAccounting` for each speaker, keyed by the persisted native execution ID. It shows provider-reported prompt/output/cache tokens separately from the estimated persona, system, memory, shared-state, history and tool-schema components. Unknown token counts remain null rather than being treated as zero. `/api/sessions/:id/cancel` also uses an indexed running-session query instead of scanning historical executions. These are regression-tested development improvements, not a claimed live-model success-rate uplift.

### Selective 9-agent room context across multiple owner turns

Room work that requires native tools or procedures no longer reverts to the full ordinary-chat transcript just because fixed tool schemas exceed the conversational target budget. The native work system/tool prompt is preserved, while prior owner instructions, committed current responses, personal history and provenance-linked tool results use the already existing source-scoped room context selector. The original transcript and memory remain intact.

Each native work execution persists `inputContext.conversationSelection=room-selective-v1`, selected message IDs, available messages and estimated context components. A CI integration test exercises **9 agents × 3 distinct owner turns = 27 model replies + 3 owner messages** using a mock streaming provider; it verifies all agents respond, no premature completion is claimed, and irrelevant first-turn peer content is not replayed during turn three. Real-model token and task-quality comparisons are pending.

The room compiler now pins at most the latest **two** peer replies for complex owner requests (**one** otherwise), while independently preserving the current owner instruction and each speaker's own previous reply. Up to eight additional current-speaker candidates compete with historical evidence under the configured context budget using lexical relevance, recency and token cost. Only the prompt projection is reduced; every full speaker message stays in SQLite. The `room_context` accounting adds `peerCandidates`, `peerPinned` and `peerSelected` for before/after diagnosis. No measured token reduction or accuracy improvement is claimed yet.

For native task runs that exhaust their model-round allowance, the finalization boundary now checks durable completed tool receipts against the frozen acceptance obligations once more, without another paid model call. Successful terminal verification produces a normal completed run; incomplete verification persists `execution.terminalReview` with remaining obligations and remains a failure instead of upgrading a merely present file to success. These new source-level regression cases have not yet been executed on Windows/macOS release environments.

### Native chat stop and Cloudflare room cache affinity

During an active chat turn the Send icon becomes an accessible **Stop** control. Stop calls the server cancellation API and waits for durable native turn teardown before aborting the browser SSE stream, releasing the composer and allowing another turn in the same session. The same control is used by `/stop`. The server's admission-epoch snapshot prevents stop-before-start races, and native `tokio::select!` drops stalled model/HTTP work on cancellation. A Rust HTTP fixture verifies a tokenless pending SSE request, cancelled execution, no late assistant save and successful next turn; frontend TypeScript/build and UI wiring tests run in CI.

In group rooms, stable global instructions, owner rules and room/roster context now appear **before** the participant-specific persona, so consecutive speakers have a common cacheable prefix. Cloudflare Workers AI requests send `x-session-affinity` scoped to the same room/provider/model/account cohort; participant conversation cursors remain separate. Unsupported Workers AI routes are not sent OpenAI-specific `prompt_cache_key`. Provider cached tokens are reported as `null` when missing, never inferred from prompt length. Prefix reuse and real billed discounts depend on model/provider support and require measured comparisons; these code changes alone do not establish cache hit rates.
