**2026-11-18 — Money original-source lineage and same-cycle buyer/supplier research (sub2):** RSS ingestion now retains separate archived article URLs even when headlines match, groups suspected copies under a stable story digest, and records actual publisher identity independently of RSS search-channel labels. Google News mirrors and same-story clusters cannot inflate two-source market signals. New observations enter the current cycle's 120-signal research window instead of waiting until the next scan. Existing Money SQLite records preserve original text, decisions, and event identity while missing publisher/feed metadata can be backfilled transactionally. [Code paths, tests, limitations](docs/SUB2-MONEY-SOURCE-LINEAGE-2026-11-18.md). No new model loop, UI, or paid background process.

**2026-11-18 — Progressive native tool disclosure (sub2):** Chat, prompt jobs via the TypeScript SDK, and actionable room turns reuse the same Rust tool executor. Large tool catalogs expose an eight-tool native core, four request-relevant schemas and a small search tool; a successful native search admits up to eight more *real* owner-enabled tool schemas next round. Model calls to unadvertised tool names are blocked before dispatch. No second agent loop, permissions engine or worker is installed. [Execution contract, regression coverage and limits](docs/SUB2-PROGRESSIVE-NATIVE-TOOLS-2026-11-18.md). Real-model success/token gains and Rust test passes remain unverified for this revision.

**2026-11-18 — Single native tool gate / scoped effect history (sub2):** Zero-model exact replacements now reuse the regular permission, tool, receipt and verification gateway. Model-free receipts stay durable without orphan provider chat messages. SQLite composite indexes select only the same resource and exact effect when admitting or inspecting a tool, instead of deserializing the full session operation archive; old pending effects remain visible without a recent-N cutoff. [Source and verification limits](docs/SUB2-SCOPED-NATIVE-EFFECTS-2026-11-18.md). Rust tests and benchmarks have not been run for this revision.

## 🦀 Lean Rust agent core — owner-directed improvements (2026-11-16)

**2026-11-18 — One Rust kernel, optional harnesses:** The native Rust turn controller remains the only DOMINION execution owner. No external agent executable is bundled, installed, started or advertised by default. Operators may register existing CLI agents through a single dynamic tool; approval, budgets, cancellation and native effect receipts stay in the same gateway. Ordinary ProgramIR repairs now use a single proposal by default; contrastive probes and isolated local-file trials are explicit opt-ins. [Architecture and limitations](docs/SUB2-ONE-KERNEL-OPTIONAL-HARNESSES-2026-11-18.md).

**2026-11-18 — Actual isolated ProgramIR candidate rollouts (sub2):** After evidence-based read-only probes, up to three local-only repair revisions can now be executed by the existing native Rust ProgramIR interpreter in separate temporary workspace/SQLite states. Real tool receipts, file modifications and postflight readbacks decide which candidate may be promoted; a failed set cannot mutate the owner workspace. The winner carries original SHA-256 compare-and-write or atomic create-if-absent constraints into the normal approval gateway. Shell, browser, network, MCP and other external effects are explicitly excluded from local candidate trials; preflight-only fallback is not represented as isolated testing. [Implementation and limits](docs/SUB2-CONTRASTIVE-PROGRAM-REPAIR-2026-11-18.md).

**2026-11-18 — Retired live UCB1 kernel exploration (sub2):** The active algorithm dispatch no longer chooses kernels by historical success-rate bonuses or unexplained exploration. Only explicitly activated executable kernels can be selected; validated/generated/disabled records never silently become active. Per-kind indexed SQLite lookup replaces full scans of algorithm records and statistics on every decision. Historical algorithm outcomes remain available for review and controlled evaluation. This is a removal of a legacy live policy, not a claim to have invented a frontier algorithm. See [Rust selection source](src/engine/algorithms.rs) and [DB lookup](src/db.rs). Real-model performance impact remains unmeasured.

**2026-11-18 — Evidence-driven native repair (sub2):** One Rust execution path now links actual failed-tool receipts, bounded causal provenance, task-specific read-only diagnostic probes, up to three ProgramIR repair candidates and actual pre-/post-execution observations. The code reuses the same tool admission, approvals, cancellation epoch, round/tool budgets, effect receipts and durable checkpoint; it preserves completed external actions and requires verified postflight evidence before reporting a repaired program complete. False/Unknown observations remain unresolved, not successes. See [hypothesis evidence and tool failure recovery](docs/SUB2-EVIDENCE-GUIDED-PROBES-2026-11-18.md) and [contrastive ProgramIR repair and remaining verification](docs/SUB2-CONTRASTIVE-PROGRAM-REPAIR-2026-11-18.md). Rust tests are committed; passing CI and measured task gains are not claimed.

**The current product direction supersedes the previous built-in video/RSI scoring brief.** DOMINION should execute owner-assigned tasks (planning, tools, approval, delegation, budgets, cancellation, durable state, recovery and result verification). Changes to the harness itself are **owner-directed coding tasks**, never an automatically adopted core mutation campaign. Optional video/audio generation is connected through MCP/connected tools rather than a permanent built-in rendering engine.

On `dominion-sub2`, the model-free Rust agent kernel also owns round/tool budget admission and deadline checks; active chat cancellation now uses local notifications with a 1-second persisted-epoch fallback instead of 50ms DB polling. Failed tool events are now retained as decision evidence, explicit uncertain external effects block automatic replay, and repeated identical reads do not count as new progress ([source-level changes and tests](docs/SUB2-FAILURE-EVIDENCE-AND-RETRY-2026-11-17.md); CI and real-model measurement pending). New chats and rooms default `autoLearning=false`; perpetual growth/distillation/experimental-policy loops no longer run; the job scheduler wakes from events with a 5-second recovery backstop and indexed live-job lookup. The built-in video/audio renderer and autonomous RSI research/generation source modules, write APIs, frontend screens, JS scripts and unused SVG-renderer dependencies have been removed. **Archived jobs, SQLite records, previous MP4s, memory and conversations remain readable.** New media generation is delegated through MCP/Connections. [Lean-core source and compatibility audit](docs/SUB2-LEAN-CUTOVER-2026-11-16.md).

# DOMINION

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" />
</p>

<h3 align="center">Bring your own model. Run your own agents. Own your workflow.</h3>

<p align="center">
  <strong>An open-source, local-first AI agent workspace powered by a native Rust execution engine.</strong><br/>
  Connect your own cloud or local LLMs. Run tool-using agents, collaborate across multiple agents, and invoke optional media-generation tools through MCP or Connections.
</p>

<p align="center">
  <a href="README.md"><strong>English</strong></a> · <a href="README.ko.md">한국어</a>
</p>

<p align="center">
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue" /></a>
  <img alt="Rust engine" src="https://img.shields.io/badge/engine-Rust-262626" />
  <img alt="React interface" src="https://img.shields.io/badge/interface-React-149ECA" />
  <img alt="BYOK and local LLMs" src="https://img.shields.io/badge/models-BYOK%20%2B%20local-6855D8" />
  <a href="https://github.com/hushmind3/dominion-rust/actions"><img alt="GitHub Actions" src="https://img.shields.io/github/actions/workflow/status/hushmind3/dominion-rust/ci.yml?branch=main&label=CI" /></a>
</p>

<p align="center">
  <a href="#-screenshots">Screenshots</a> ·
  <a href="#-what-you-can-do">Features</a> ·
  <a href="#-quick-start">Quick start</a> ·
  <a href="#-architecture">Architecture</a> ·
  <a href="https://github.com/hushmind3/dominion-rust/discussions">Discussions</a> ·
  <a href="#-contributing">Contribute</a>
</p>

---

## What is DOMINION?

The built-in media renderer and independent generational RSI engine have been retired on `dominion-sub2`. Their historical records remain readable; the Rust harness now focuses on user-requested tasks, verification and recovery. [Current lean cutover](docs/SUB2-LEAN-CUTOVER-2026-11-17.md) · [historical research index](docs/INDEX.md).


**DOMINION is an AI agent tool, not another AI model or a token-reselling service.**

Connect a model you already have access to — including supported cloud APIs and locally hosted models — and use it inside an agent workspace. DOMINION's Rust harness handles tool execution, permissions, durable tasks, long-term memory, and multi-agent coordination. Media-generation tools remain optional integrations; independent RSI generation campaigns and the built-in video rendering engine have been retired on this branch.

You choose the models. You control the credentials. You run the workspace.

**Designed for developers, local-LLM enthusiasts, and people who want to experiment with their own autonomous workflows.**

## 📸 Screenshots

**Multi-agent room — agents sharing a conversation, with participant and room controls**

<p align="center">
  <a href="docs/01-multi-agent-room.webp">
    <img src="docs/01-multi-agent-room.webp" alt="DOMINION multi-agent room with multiple participants, conversation history, and room settings" width="100%" />
  </a>
</p>

<table>
  <tr>
    <td align="center" width="50%"><strong>Chat workspace</strong></td>
    <td align="center" width="50%"><strong>Agent studio</strong></td>
  </tr>
  <tr>
    <td><a href="docs/02-chat-workspace.webp"><img src="docs/02-chat-workspace.webp" alt="DOMINION chat workspace with model selector and task input" width="100%" /></a></td>
    <td><a href="docs/03-agent-studio.webp"><img src="docs/03-agent-studio.webp" alt="DOMINION agent studio with individual agent cards, roles, and model settings" width="100%" /></a></td>
  </tr>
  <tr>
    <td align="center">Choose a model and give it a task.</td>
    <td align="center">Configure independent agents and roles.</td>
  </tr>
</table>

<sub>Real development-build screenshots with Korean UI labels. English and Korean UI translation files are included in the project. Screenshots show interfaces, not claims of comprehensive real-model reliability.</sub>

## ✨ What you can do

| Feature | Description |
| --- | --- |
| **Your models, your keys** | Register supported API providers with your own credentials. No DOMINION-hosted token subscription required. |
| **Local LLMs** | Connect local inference with GGUF/llama.cpp or Ollama; a macOS MLX path is also available. |
| **Tool-using agents** | Work with files, shell commands, HTTP/search tools, skills, and MCP integrations, subject to execution policies. |
| **Persistent memory** | Store conversation context, verified experience, corrections, and execution history locally. |
| **Multi-agent rooms** | Give agents separate roles, run shared discussions, and coordinate workflows with multiple participants. |
| **Durable jobs** | Track tasks, approvals, execution evidence, cancellation, and recovery in SQLite. |
| **Media via MCP/Connections** | Invoke external generation services on demand; the Rust executable does not embed video models, FFmpeg rendering or speech synthesis. Historical finished MP4s remain accessible. |
| **Messaging channels** | Native Telegram and Discord adapters; currently require bot credentials and manual configuration. |
| **Owner-directed code improvement** | Edit repositories, execute real tests, inspect failures and seek user approval for adoption. No autonomous core mutation or generational RSI campaigns. |

### Why a separate agent workspace?

- **Model flexibility:** use supported cloud models or run locally without locking the entire workflow to one provider.
- **Execution you can inspect:** tasks have tool calls, outcomes, approvals, and error records, not just chat messages.
- **Local-first state:** SQLite stores agent data and history locally by default.
- **Open code:** inspect and extend a Rust engine with a React UI under the Apache-2.0 license.

## ⚡ Quick start

**Source build for macOS and Windows.** Install Git, Rust stable/Cargo, Node.js **24.12+**, and your OS's C/C++ build prerequisites. On Windows, you will also need the MSVC build tools and Windows SDK.

```bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home .
```

Open **http://127.0.0.1:4317** in your browser.

1. Add your own API provider, or configure a compatible local-model runtime in **Models**.
2. Select a model in **Chat** and try a task.
3. Explore **Agents**, **Multi-Agent Rooms**, **Memory**, **Workflows**, and **MCP / Skills**.
4. To use Telegram or Discord, configure your bot credentials and destination IDs in **Connections**.

Cloud-model usage is billed by your provider. Local models use your own hardware. **Prebuilt release downloads are not yet available** — follow the source build above.

More details: [installation guide (Korean)](docs/GETTING-STARTED.md) · [desktop build guide](docs/DESKTOP.md).

## 🧱 Architecture

```text
React UI (Vite)
       │
       ▼
Rust HTTP API (Axum) ──► SQLite (local state, memory, jobs, logs)
       │
       ▼
Native Rust agent harness
       ├── Chat / streaming / tool calls / approvals
       ├── Memory / experience / workflows / multi-agent rooms
       ├── Skills / MCP / Telegram / Discord
       ├── On-demand external media and specialized tools through MCP
       ├── Owner-directed source editing, tests and verifier evidence
       ├── User-configured model APIs
       └── Local inference (GGUF / llama.cpp and other configured runtimes)
```

The agent execution engine is written in **Rust**. Node.js is used to build the React frontend rather than to host the agent runtime. Some local-model integrations and MCP servers may require their own processes.

**Technical docs:** [Engine](ENGINE.md) · [Documentation index](docs/INDEX.md) · [Lean-core migration contract](docs/SUB2-LEAN-CUTOVER-2026-11-16.md).

## 💰 Per-job provider token ceilings

A user may set `input.tokenBudget` (1,024–2,000,000 tokens) when starting an autonomous job. The Rust worker records a job-owned ceiling relative to the session's current usage; the native model runtime checks prior measured use and estimates the next model input before another billable provider call, and reduces the requested output limit to fit the balance. The TypeScript/native Rust SDKs expose this job-specific input. The unchanged main frontend shows provider-reported token usage in the **Home > Autonomous work** panel but does not expose the separate `tokenBudget` input; its own `/api/sdk/runs` policy has a default model-token ceiling. Exhaustion goes to `needs_review` rather than a loop of further paid retries. A provider's final actual usage can exceed the estimate, and separately delegated jobs do not yet share one strict atomic cost reservation ledger: this is a model-call admission budget, not a billing guarantee.

## 🎯 Start, steer and replace a running autonomous task

The **main frontend** starts work through `web/src/pages/Home.tsx` → `web/src/components/NativeAgentPanel.tsx` → `POST /api/sdk/runs`. `src/api/sdk_bridge.rs` delegates to the existing Rust job queue and supplies provider selection, optional verification command, live tool progress, approval, stop, reconciliation and resume. The **Activity** page shows the existing job and execution history; it does not contain an independent task composer or steering buttons.

The native Rust `jobs::steer_with_mode` and `autonomy::execute` also implement append/replace owner goals for `/api/jobs/:id/steer` and `@dominion/sdk` (`jobs.steer()` and `jobs.replaceGoal()`). These are **API/SDK capabilities**, not controls displayed in the unchanged main GUI. On the main GUI's Resume path, zero-tool provider failures can retry; uncertain side effects require an explicit owner observation. The continuation prompt preserves that observation, rather than blindly repeating the original request.

See [2026 source-guided harness comparison](docs/AGENT-HARNESS-2026.md) and [TypeScript SDK contract](sdk/typescript/README.md).

## 🧠 Sub2 harness boundaries and atomic job recovery (2026-11-16)

On `dominion-sub2`, the native Rust harness has dedicated `chat/tool_projection.rs` for room/Home/agent tool-schema selection and actual serialized-size ceilings, and `chat/outcome.rs` for terminal verdicts, idempotent finalization fingerprints and evidence-gated success memory. The owner-controlled execution loop remains in `chat/runtime.rs` and retains its existing GUI/TypeScript/Rust SDK contract. Empty or malformed tools are rejected before they reach the model, and a single oversize schema can no longer exceed the declared tool budget.

`Store::patch_if_all_merge` merges nested checkpoint state under the same SQLite connection lock as the cancellation/status guards. The native job worker checks durable cancellation/terminal state under the same lock; obsolete nested checkpoint records remain compatible for historical jobs. No database migration, model change, new API or UI redesign is required. [Implementation, comparisons and limitations](docs/SUB2-HARNESS-BOUNDARIES-2026-11-16.md).


## 🎯 Sub2 verified-cost routing and native task scorecard

Auto model routing now learns a cautious lower confidence bound **only from independently verified, completed native Rust work** and refuses to substitute fabricated model benchmarks or self-graded answers. Existing explicitly selected providers, capabilities, fallback and the default router remain intact until every candidate has at least 8 comparable verified cases. A local pricing panel accepts owner-entered input/output/cache-read model tariffs. `model.usage` records configured-rate estimates and unpriced coverage, never provider invoices. The native evaluation view is linked end-to-end from `engine/evaluation.rs` to `GET /api/evaluation/summary`, TypeScript SDK `evaluation.summary()` and the existing Activity screen; it reports verified success, missing evidence, tokens and latency. Use `node scripts/sub2-compare.mjs baseline.json candidate.json` on matching exported cohorts, with tool-schema and budget metadata, for reproducible comparisons.

Parallel mutating tools now use Rust resource-scoped locks across agent sessions: shell, terminal and patches serialize on the same workspace; safe concurrent reads and independent workspaces retain parallelism. The previously built-in video QA module was removed with the media renderer. User-selected external MCP media providers own their generation and quality evaluation, while old MP4 files remain retrievable. [Judge-ready contracts, caveats and test map](docs/SUB2-COST-ROUTING-EVALUATION-2026-11-16.md).

## ⚙️ Parallel Rust harness budgets

Independent agent branches reserve non-overlapping model-round, tool-call and proposal budgets as well as durable operation identifiers. Workspace mutations and shell commands serialize by resource lock while read-only tasks can run concurrently. These are operation budgets rather than proof of billed cost.

## 🦀 Native Rust SDK and autonomous execution

**Rust is Dominion's primary engine and SDK.** The desktop, HTTP API and external Rust consumers use the same native lifecycle in `src/sdk.rs` (`Dominion::open / start / submit / steer / wait`). This keeps model selection, tools, durable jobs, memory and permissions under Dominion's control, independently of any vendor's agent SDK.

**Proof-carrying code edits:** native workspace tools record whether bytes actually changed and their post-write SHA-256. The Rust verifier rehashes the current file, rejects no-op/fabricated writes, and requires a successful post-edit test when the owner supplied no explicit file or command check. Failing checks are fed back into bounded replanning, never converted into a false green job. See [verified patch implementation](docs/SUB2-VERIFIED-PATCH-2026-11-15.md).

Persisted parallel tool replies are now reconstructed by original model call ID in the Rust context compiler. Out-of-order completion rows no longer disappear from the next model turn; partial and duplicate tool rows remain excluded from provider wire history. Tests reproduce both states. [Execution history contract](docs/SUB2-TOOL-BARRIERS-2026-11-15.md).

**Native multi-file code editing:** `workspace_apply_patch` accepts Codex/DeepSeek-compatible Add/Update/Delete/Move patch sections for up to 32 files. It preflights every operation, stages output bytes, rechecks source versions, acquires the existing canonical file locks, and reports exact completed files and uncertain partial failures. The native tool passes through owner approval, operation journaling, frozen SHA-256 acceptance and autonomous replanning. [Implementation and tests](docs/SUB2-MULTIFILE-PATCH-2026-11-15.md).

Native crash/cancel recovery now checks for in-flight operation receipts before requeueing an interrupted job; a cancelled turn attempts to drain started native tools, persists completed receipts and marks timed-out unknown effects for owner review. DB read errors no longer imply an empty live-operation set. This is exercised by cancellation, timeout and restart regression tests. [Detailed contract](docs/SUB2-TOOL-BARRIERS-2026-11-15.md).

Native tool execution now preserves the original model-call order across exclusive write/shell/MCP barriers while running adjacent independent reads in bounded parallel groups (up to four calls). A real mock-provider regression asserts that reads after a write observe new bytes, not stale pre-write content. [Source-grounded scheduler design](docs/SUB2-TOOL-BARRIERS-2026-11-15.md).

Ordinary prompt jobs now enter a Rust-owned verification/replan cycle. The engine captures acceptance conditions before work, gathers actual tool evidence, continues when measurable checks fail, and stops uncertain external side effects rather than replaying them. The user can add a new instruction to an active job; the next model turn sees the persisted revision. The default cycle is bounded to 3 attempts and can be configured to 1–6.

```rust
use dominion_rust::sdk::{Dominion, EngineOptions, Prompt};
let sdk = Dominion::open(EngineOptions::new(".", "./runtime"))?;
sdk.start()?; // in a Tokio runtime
let job = sdk.submit(Prompt::new("Inspect, fix and verify the code"))?;
```

The [TypeScript SDK](sdk/typescript/README.md) is an **optional auxiliary client**, not a harness. The sub2 `web/` frontend keeps the shared API contract and its `web/src/api.ts` client without the retired video-studio or RSI screens; the optional TypeScript SDK is for external consumers. Both clients communicate with the same Rust `/api/*` runtime and persisted results. See [2026 frontier source comparison](docs/FRONTIER-COMPARISON-SUB2-2026-11-15.md) and [execution contract](docs/SUB2-SDK-DURABLE-EVENTS-2026-11-15.md). Frontier-equivalent performance is not yet benchmarked. The main GUI's `/api/sdk/runs` requests now bridge into the same Rust job worker via `src/api/sdk_bridge.rs`, including real approval resolution, cancel, bounded live operation progress and an owner-reconciled resume (or safe retry when no tool effects were recorded). Verification commands require actual successful tool evidence after the last file edit. The engine additionally supports bounded read-only parallel subagent investigation (`delegate_parallel`). Archived media design notes remain historical only; no built-in ComfyUI renderer runs.

## 🚧 Project status

DOMINION is an **active developer preview**, not a managed SaaS or a fully validated commercial agent product.

- The repository implements chat, tool execution, memory, multi-agent rooms, workflow jobs, and model connections. Real-world results depend on model capabilities and the environment.
- Multi-agent behavior includes mocked-model tests; long-running real-model reliability is still an area to validate.
- Telegram and Discord integration currently require manual bot setup rather than one-click provisioning.
- Cache and context-management code exists, but no fixed token-saving percentage is guaranteed.
- New media generation and publishing require an explicitly connected external provider or MCP tool; retired built-in renderer and uploader jobs cannot be started.
- Independent generational RSI campaigns and automatic source adoption are retired. User-requested code edits and verifiable task improvements remain supported.
- Check the [CI results](https://github.com/hushmind3/dominion-rust/actions) for the status of a particular commit.

## 🤝 Contributing

Contributions are welcome — particularly **reproducible bug reports, test cases, model/provider compatibility fixes, Rust harness improvements, UI work, and documentation**.

- [Join Discussions](https://github.com/hushmind3/dominion-rust/discussions) — ask questions, share your setup, and propose ideas for agents, external MCP workflows, and task verification.
- [Open an issue](https://github.com/hushmind3/dominion-rust/issues) — include your OS, model/provider, reproduction steps, and expected vs. actual behavior.
- [Submit a pull request](https://github.com/hushmind3/dominion-rust/pulls) — explain the change and include appropriate tests when possible.
- Never include API keys, bot tokens, private conversations, or other secrets in reports.

**License:** [Apache 2.0](LICENSE) · **한국어:** [README.ko.md](README.ko.md)

### sub2: event-driven queue and indexed recovery (2026-11-17)

- Shared Rust dispatcher scans the durable active queue on enqueue, worker completion or 10-second fallback, admitting at most `DOMINION_JOB_WORKERS` tasks. Same job IDs, persistence, permissions and cancellation semantics.
- Crash recovery loads active jobs/running executions only and resolves uncertain tool effects with a set-based SQLite join. Unproven external effects remain `needs_review`, not replayed.
- Database read failures are audited rather than presented as an empty queue. Money stays independently removable and shares this host's existing DB/runner.
- Analytical idle queue scans at default 4 workers: old 4×12=48/minute, new 1×6=6/minute. This is not an empirical CPU or token measurement.

### Sub2: provider-wire tool-history budgeting (2026-11-17)

The native Rust chat harness now compacts **completed, oversized historical tool-call/result pairs only in the provider-wire copy**, prioritizing newer frames. Huge `workspace_write` arguments and repeated tool output are no longer re-sent verbatim on later model turns; original evidence, tool arguments, receipts, user text, settings and SQLite messages remain unchanged. Normal small tool pairs preserve the exact OpenAI-compatible tool protocol. Incomplete/duplicate tool pairs are left for existing recovery/error handling. The budgeting logic lives in `src/engine/chat/trace_budget.rs`, shared by ordinary chat, SDK jobs and room fallback through the normal Rust model path. The byte limits are deterministic safeguards, not measured tokens. Release builds and frontend validation remain out of scope during development.

### Sub2: truthful Money inference usage (2026-11-17)

The Money `/api/money/find` response now obtains completed/active model call counts, input/output tokens, cached input and estimated-call coverage from the **same per-session Rust audit ledger** as other job clients, not the research draft's hard-coded `modelCalls:0`. The UI shows those observed token totals while inference runs. Provider pricing/FX cannot be inferred from token counts, so the Money panel explicitly reports *KRW cost not measured* instead of inventing a charge. DB-only research remains 0 model calls. Rust regression tests confirm per-job isolation.

The shared SQLite usage counter now uses `idx_audit_usage_session`, a filtered session expression index, so Money's active usage polling and ordinary SDK budget admission no longer require a full historical audit scan. Malformed legacy entries are not deleted or rewritten. The changed Rust DB test verifies session isolation and the query plan; no runtime benchmark is implied.

### Sub2: independent Rust SDK verifier chronology (2026-11-18)

The model-independent dominion-agent-kernel verification module now checks the actual chronological order of committed tool receipts. An earlier successful verifier is invalidated by a later failed/partially applied file mutation or failed retest, and only the newest independent test following an observed edit can certify a free-form local code change. The host normalizes execution evidence once; chat, SDK jobs and Money implementation use the same kernel, without a second runtime or changing SQLite data. Related Rust debug-profile regression tests only; release builds are deferred.

### Sub2: independent SDK tool ordering and bounded batch admission (2026-11-18)

The Rust `dominion-agent-kernel::scheduling` module now owns the safe parallel-read allowlist, model-order exclusive barriers and a strictly capacity-bounded valid-JSON prefix. Previously the active chat scheduler's `parallel_prefix` ignored the upper limit (apart from 0–1), admitting *all* contiguous reads even though the rolling pool executed only four concurrently. The existing `src/engine/chat/tool_schedule.rs` now reuses this independent kernel rather than duplicating policy; it still owns Tokio draining, and the host retains approval, cancellation, budgets and durable receipts. Read/write barriers and malformed argument semantics are preserved. Tests are focused Rust debug unit tests; release builds and real-model benchmarks remain deferred.

### Sub2 Money — durable owner review/archived opportunities (2026-11-18)

The optional Money feature now has the native SQLite-backed `GET /api/money/candidates?status=archived&q=...`, `GET /api/money/candidates/:id/history` and unchanged `POST /api/money/candidates/:id`. Candidate decisions are append-only `money_decision` records tied atomically to the latest candidate revision through `Store::mutate_record`. The old status/notes/economics POST contract remains supported, while optional `expectedRevision` returns 409 on stale updates. Archived hypotheses are searchable and can be restored to research without deleting source citations. Pending/approved implementation receipts cannot be accidentally hidden by a stale reject/hold update. No background model calls or separate storage engine were introduced. Focused Rust tests cover concurrent review conflicts and atomic history.

The Money screen adds a reversible **Hold** action, searchable held/rejected candidate archive, immutable review-history disclosure and a one-click return-to-research action. The TypeScript SDK now exposes `money.archive`, `money.review`, `money.history` and `money.candidateDetails`, preserving existing SDK entry points. No paid model call is needed to search, hold or restore a candidate.

New candidate drafts carry `evidence[].sourceId`, a SHA-256 `sourceRevision` pinned to the archived headline/description/page-excerpt snapshot, and an explicit `quoteMatchStatus` (exact title, title prefix, or unmatched). These are **local archive-attribution checks**, not independent verification of the original website or buyer payment. A later enriched article changes the revision but leaves historical review evidence intact. Held candidates are excluded from fresh discovery rotation without losing their archive searchability.

### Sub2 — Money dashboard indexed aggregation (2026-11-18)

Money's `GET /api/money/stats` now runs a bounded SQL projection through `Store::money_dashboard_stats`, instead of deserializing **every** historical job, candidate, ingestion record and autopilot. SQLite groups per-model Money discovery job outcomes; the latest captured ingestion uses an expression index; an active autopilot is selected by a filtered updated-at index. The JSON payload field names and frontend contract remain unchanged. SQL read errors return HTTP 500 rather than fake empty/zero counters. Rust development regression tests assert legacy field equivalence and the autopilot query plan. No CPU benchmark or release build has been performed.

### Sub2 — durable worker-attempt fencing (2026-11-18)

The Rust job harness now binds each running `JobContext` to its dispatcher-claimed `attempts` generation. `Store::checkpoint_running_job` commits under one SQLite IMMEDIATE transaction and accepts progress/terminal state only while that *same attempt* is running and the owner has not cancelled it. `Store::patch_if` is now an IMMEDIATE compare-and-write across **separate SQLite connections**, closing a two-process race where concurrent workers could both claim a queued job or a stale steer/cancel response could win. Legacy `videoState` nested merging is preserved. The existing deferred-work path can requeue its attempt without deleting persisted evidence. Cancellation is committed before signalling the live session, and a failed compare returns conflict rather than falsely claiming cancellation. Focused Rust development tests exercise cross-connection claims, stale worker checkpoints, owner cancellation, and deferred transitions. Existing native message persistence still has its own channel/session ownership; this patch fences job records and does not claim a fully transactionally fenced assistant transcript.

Deferral can refund the user-visible retry counter, so the worker owner additionally carries a monotonic `runEpoch` incremented by **both** the normal dispatcher and immediate delegated start. The running checkpoint checks `attempts` and `runEpoch` atomically; even a refunded 1→0→1 retry cannot reauthorize a previous worker. Legacy jobs without the new field default to epoch 0 until their next claim. Regression tests cover repeated attempt counters and stale workers.

A worker now constructs `JobContext` directly from its dispatcher-claimed record (not a later DB reread), verifies that the claim is still live before dispatch, and discards stale work without launching tools. Startup recovery uses a four-field `status/attempts/runEpoch/cancelRequested` guarded merge in an SQLite IMMEDIATE transaction, so a recovering process cannot overwrite a newer worker or a user cancellation. Regression tests cover outdated dispatch snapshots and recovery across independent SQLite connections.

### Sub2 — evidence-directed recovery and UCB1 credit (2026-11-18)

The existing agent UCB1 selector no longer interprets one task-level success or failure as proof for **every** strategy kernel selected during a multi-algorithm execution. Only a direct independent domain signal can credit a specific algorithm when other algorithms were involved. Non-attributable runs remain `unrated` with a durable explanation, and the optional `rebuild_stats_from_usage` uses the same rule; no historical data are deleted or silently recomputed. A single observed algorithm may still receive the weaker task-outcome proxy for backward compatibility.

The independent Rust `Cycle` now classifies observed compile, test, missing evidence, authorization, provider and verification failures, steering the *existing bounded* next attempt toward a relevant investigation rather than generic repeats. No extra LLM call, background loop or permission is granted. Each autonomous attempt records `failureClass` in its persisted receipt. See `docs/SUB2-ATTRIBUTION-REPAIR-2026-11-18.md`.
