## 🦀 Lean Rust agent core — owner-directed improvements (2026-11-16)

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
