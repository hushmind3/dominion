# DOMINION

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" />
</p>

<h3 align="center">Bring your own model. Run your own agents. Own your workflow.</h3>

<p align="center">
  <strong>An open-source, local-first AI agent workspace powered by a native Rust execution engine.</strong><br/>
  Connect your own cloud or local LLMs. Run tool-using agents, collaborate across multiple agents, generate videos, and explore experimental recursive self-improvement.
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

**DOMINION is an AI agent tool, not another AI model or a token-reselling service.**

Connect a model you already have access to — including supported cloud APIs and locally hosted models — and use it inside an agent workspace. DOMINION's Rust harness handles tool execution, permissions, durable tasks, long-term memory, and multi-agent coordination. The workspace also includes AI video generation workflows and experimental recursive self-improvement (RSI).

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
| **AI video creation** | A dedicated video workspace for prompt-based generation jobs with native/ComfyUI backends, output management, and YouTube upload integration. Requires appropriate local backends and account setup. |
| **Messaging channels** | Native Telegram and Discord adapters; currently require bot credentials and manual configuration. |
| **Recursive self-improvement (RSI) research** | Experiment with source-editing campaigns, independent evaluations, candidate selection, evidence records, and controlled adoption. This does **not** claim proven general recursive intelligence gains. |

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
3. Explore **Agents**, **Multi-Agent Rooms**, **Memory**, **Workflows**, **Video**, **Self-Improvement**, and **MCP / Skills**.
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
       ├── Video generation jobs (native / ComfyUI) and YouTube integration
       ├── Experimental self-improvement and evaluation campaigns
       ├── User-configured model APIs
       └── Local inference (GGUF / llama.cpp and other configured runtimes)
```

The agent execution engine is written in **Rust**. Node.js is used to build the React frontend rather than to host the agent runtime. Some local-model integrations and MCP servers may require their own processes.

**Technical docs:** [Engine](ENGINE.md) · [Documentation index](docs/INDEX.md) · [Self-improvement execution contract](docs/RSI-MAIN-INTEGRATION-2026-11-12.md).

## 🚧 Project status

DOMINION is an **active developer preview**, not a managed SaaS or a fully validated commercial agent product.

- The repository implements chat, tool execution, memory, multi-agent rooms, workflow jobs, and model connections. Real-world results depend on model capabilities and the environment.
- Multi-agent behavior includes mocked-model tests; long-running real-model reliability is still an area to validate.
- Telegram and Discord integration currently require manual bot setup rather than one-click provisioning.
- Cache and context-management code exists, but no fixed token-saving percentage is guaranteed.
- Video generation requires a configured native engine or compatible ComfyUI environment; YouTube upload requires account authorization.
- Recursive source-improvement mechanisms are **experimental**. The project does not claim demonstrated, indefinitely compounding intelligence gains.
- Check the [CI results](https://github.com/hushmind3/dominion-rust/actions) for the status of a particular commit.

## 🤝 Contributing

Contributions are welcome — particularly **reproducible bug reports, test cases, model/provider compatibility fixes, Rust harness improvements, UI work, and documentation**.

- [Join Discussions](https://github.com/hushmind3/dominion-rust/discussions) — ask questions, share your setup, and propose ideas for agents, video workflows, and RSI research.
- [Open an issue](https://github.com/hushmind3/dominion-rust/issues) — include your OS, model/provider, reproduction steps, and expected vs. actual behavior.
- [Submit a pull request](https://github.com/hushmind3/dominion-rust/pulls) — explain the change and include appropriate tests when possible.
- Never include API keys, bot tokens, private conversations, or other secrets in reports.

**License:** [Apache 2.0](LICENSE) · **한국어:** [README.ko.md](README.ko.md)


## 🧩 SDKs and public Rust API (dominion-sub3)

The native Rust SDK core and companion TypeScript client now target the same **native Dominion Rust execution engine** as the React desktop UI:

- [Rust SDK](sdk/rust/README.md) — `dominion-sdk` primary independent native Rust model/tool execution kernel with parallel read operations, owner steering, deterministic evidence and optional HTTP client.
- [TypeScript SDK](sdk/typescript/README.md) — `@dominion-ai/sdk` reusable package with typed resources, reconnection, abort support and durable job waiting.
- [SDK contract and 2026 architecture comparison](docs/SDK-ARCHITECTURE-2026-11-15.md).

Both clients use the actual `/api/*` routes from `src/api/mod.rs`, optional `X-Dominion-Token`, and Rust-owned model, tool, memory and checkpoint logic. They do not embed an external provider's agent harness. The SDKs are source packages; they have not yet been published to crates.io/npm. Rust and native-host compilation passed the focused GitHub verification run; live-provider integration remains to be verified.


### Main frontend adopted by sub3 (2026-11-15)

The dashboard and execution-monitor design come from `main`. The autonomous-work panel retains the main UI while calling sub3's existing `/api/jobs` and `/api/approvals` APIs and the native Rust SDK kernel. No separate `/api/sdk/runs` loop is introduced. A new job is created for retries after uncertain or failed effects; the UI does not claim the old process was resumed in-place. See [SDK architecture](docs/SDK-ARCHITECTURE-2026-11-15.md).


### 2026-11-15 Rust-first agent execution improvements (sub3)

The independent native Rust kernel is in `crates/dominion-agent-sdk`. The `dominion-sdk` Rust package re-exports this actual kernel; `sdk/typescript` remains a secondary typed HTTP/SSE client, not an agent runtime. The React frontend retains the `main` user interface, with its autonomous-work panel adapted to the native Rust `/api/jobs` and `/api/approvals` routes.

Recent sub3 commits add a dependency-aware Rust tool scheduler (bounded read concurrency and strictly ordered effects), failure-triggered replanning, structured MCP error checking and clean-checkpoint `Kernel::resume`. Previously completed tool effects cannot be replayed automatically after a crash, and unknown outcomes go to owner review. Deterministic kernel scheduling and recovery integration tests are committed in `crates/dominion-agent-sdk/tests`.

**Validation state:** remote Rust SDK and host tests plus frontend TypeScript and production build passed in GitHub Actions run 43684016242. Live-provider and browser runtime remain unverified; full CI and SWE-bench stay deferred.


### Rust-native autonomous coding verification and workflows (sub3)

The main UI now sends required command postconditions to the real Rust kernel. A completed command must match the owner's requested verification command and occur after the latest workspace edit. Workflow stages execute as durable native Rust SDK child jobs rather than a separate chat-turn harness. See [Architecture](docs/SDK-ARCHITECTURE-2026-11-15.md). This source passed the focused native Rust host and frontend build checks; live end-to-end browser interaction remains unverified.


### Rust-owned coding completion gate (2026-11-15)

DOMINION now includes the native Rust SDK-side automatic acceptance/repair controller for required command checks. The model supplies candidate edits; the Rust engine performs missing exact verification commands through the standard approval and durable job operation path, feeds failure diagnostics into replanning, invalidates stale success after subsequent effects and declines unverified completion. The main GUI uses the same `/api/jobs` contract. See [the focused coding completion design](docs/RUST-CODING-COMPLETION-2026-11-15.md). Focused tests have now actually executed and passed on GitHub's Rust runner (run 43684016242).


### Verified Rust coding completion: 58 targeted Rust tests + GUI build

## Executed verification — 2026-11-15

**[GitHub Actions run 43684016242](https://github.com/hushmind3/dominion-rust/actions/runs/43684016242)** on commit `58c37744763506268a369674e3f828ae56b2a699` **concluded SUCCESS**:

- `cargo test --manifest-path crates/dominion-agent-sdk/Cargo.toml -- --nocapture`: **35 passed, 0 failed** (12 unit + 6 kernel integration + 15 replanning + 2 real-file/shell tests)
- The two real-file tests run actual filesystem writes, `sh -c` commands, a real nonzero exit code, repair, re-verification and disk checkpoint restore. A scripted model supplies deterministic decisions; this is **not** a claim that a live frontier model solved a fresh task.
- `cargo test --manifest-path Cargo.toml --bin dominion-rust native_ -- --nocapture`: **22 passed, 0 failed** (filtered Rust host tests; 724 others not run)
- `cargo test --manifest-path Cargo.toml --bin dominion-rust recover_new_workflow_reuses_durable_rust_sdk_child -- --nocapture`: **1 passed, 0 failed** (745 other host tests not run)
- `npm ci --prefix web`, `npm run typecheck --prefix web`, `npm run build --prefix web`: **all succeeded**. Vite reported a successful production build.

**Total executed Rust cases in this narrow gate: 58 passed / 0 failed.** Full product CI, live-model coding success, live-browser UI, paid provider budget, and SWE-bench remain independently unverified. The regular full CI workflow was not invoked by this gate; verification is a dedicated `sub3-rust-sdk-check.yml` workflow and is scoped to core/host/UI validation. This narrows one proven execution capability, not a declaration that all of Dominion or its coding intelligence is frontier-equivalent.



### Durable model context + historical evidence retrieval (sub3)

The Rust agent kernel now separates persisted source/tool receipts from the bounded model-visible context. A live planner can query `dominion_receipt_search` and `dominion_receipt_read` to recover an older compiler failure or file observation even after hundreds of later tools, without rerunning the original effect. Source provenance and limitations: [Durable context design](docs/SUB3-DURABLE-CONTEXT-2026-11-15.md). The existing `/api/jobs` endpoint and main-based GUI use this same Rust kernel.


### Native provider function calls in the Rust SDK (sub3, 2026-11-15)

The default `/api/jobs` native agent path now supports provider-native typed tool calls for OpenAI-compatible chat completions, Anthropic Messages and Codex Responses. `Model::complete_with_tools` returns typed `Action` IDs into the independent Rust kernel, preserving ordinary owner approval, permission, checkpoint and validation controls; compatible local/text-only models keep the prior JSON decision mode. The real provider/request adapters are implemented in `src/engine/native_model.rs`, not embedded vendor agent runners. See [source-level design](docs/SUB3-NATIVE-FUNCTION-CALLS-2026-11-15.md). Live third-party credential and performance benchmarks are independent future validation.


### Native incremental model streaming and bounded progress (sub3, 2026-11-15)

The same Rust `/api/jobs` native agent now consumes OpenAI-compatible Chat Completions, Anthropic Messages and Codex Responses SSE as incremental structured events. `src/engine/native_stream.rs` assembles split UTF-8/tool-call JSON and only releases typed actions after a complete provider terminal event; partial, failed or interrupted calls never reach the tool dispatcher. Progress is recorded in `job.nativeStream`, sent over existing job events, and shown in the autonomous-work panel as **pending, not executed** actions. Owner cancellation interrupts stalled streams without replaying paid model calls. See [native streaming architecture](docs/SUB3-NATIVE-STREAM-2026-11-15.md).


### Sub3 incremental native SSE verification — 2026-11-15

Focused GitHub Actions verification on commit `a9193735d51105b7f4f311a0e83bbd2376e3c45e`: [run 43687745047](https://github.com/hushmind3/dominion-rust/actions/runs/43687745047) completed successfully. The log records 44 SDK test cases (16+2+6+15+3+2), 41 filtered native host test cases, and one workflow recovery test: **86 passed, 0 failed**. Frontend TypeScript check and Vite production build also passed. Real loopback HTTP streaming/stalled-cancellation fixtures are included in those tests; no commercial model API call was made.


### Codex WebSocket replay safety and bounded long-run evidence (sub3, 2026-11-15)

The interactive Rust Codex transport differentiates explicit chain rejection (safe full-context reconnection) from **uncertain request delivery** (no automatic WebSocket/HTTP/model replay, durable unknown-billing audit). Ordered multimodal content is fingerprinted rather than retaining inline media payloads in chain state. This improves safety under mid-turn disconnection without claiming that smaller wire payloads equal lower billed tokens. The independent Rust agent SDK now clips large recent tool receipts to the actual remaining model-view budget instead of dropping all recent observations when the newest entry is oversized. The full receipt history remains searchable and persistent. See [Codex transport notes](docs/SUB3-NATIVE-STREAM-2026-11-15.md) and [durable context](docs/SUB3-DURABLE-CONTEXT-2026-11-15.md). Focused GitHub Actions verification on commit `4407aa19df91538a0469ad8727de06b77a995de7` [run 43688832106](https://github.com/hushmind3/dominion-rust/actions/runs/43688832106) passed: 45 Rust SDK cases, 41 native provider/host cases, 15 Codex WebSocket cases, 1 workflow recovery case (**102/102**), plus frontend typecheck and Vite build. Commercial provider usage, true billed-token savings and long-horizon task completion remain unmeasured.


### Provider-aware context admission for native autonomous jobs (sub3)

`src/engine/native_model.rs::fit_native_context` now fits each configured provider's model context allowance **before** the Rust kernel's native tool-calling request. It reserves output tokens and trims only oldest *complete* native tool call/output pairs, retaining the owner/system instructions, current task and tool schema. Full tool evidence stays in the durable kernel receipt log for exact retrieval; the engine refuses a request when non-droppable input alone exceeds its configured context. Trimming count and estimated model-input size are audited. Follow-up [focused CI run 43689095869](https://github.com/hushmind3/dominion-rust/actions/runs/43689095869) on `0791e09572dea72f95639f1f4a5da1a497a124e5` passed 45 Rust SDK tests, 43 native host tests, 15 Codex WebSocket tests and 1 workflow recovery (**104/104**), plus frontend typecheck/build. See [durable context](docs/SUB3-DURABLE-CONTEXT-2026-11-15.md) for the admission and verification boundaries.


### Autonomous Codex job WebSocket continuity (sub3, 2026-11-15)

The production `/api/jobs` Rust kernel now owns a separate **job-scoped Codex Responses WebSocket** in `src/engine/native_codex_ws.rs`. After a complete response, the next model step continues through `previous_response_id` with only new **host-verified** tool outputs and current task-state input. Its per-job registry is isolated from interactive chat and cleaned up at task exit. Missing or changed receipts, model/owner contract changes, expired sockets and explicit missing-previous-response events start a canonical full-context chain. In-flight unknown delivery is never retried on HTTP or another model; an audit record preserves possible unknown billing. On configured context limits, provider-reported usage triggers a proactive canonical context rebase near the capacity reserve. Interactive chat, TypeScript SDK, GUI, the native approval gate and persistent Rust tool journal remain on their existing shared execution paths. See [the native Codex WebSocket design and limits](docs/SUB3-NATIVE-CODEX-WS-2026-11-15.md).

**Native Codex job continuation verification:** [focused Actions run 43690699582](https://github.com/hushmind3/dominion-rust/actions/runs/43690699582) succeeded on code commit `50768a5f`: **112/112** Rust SDK/native host/chat WebSocket/recovery tests, plus TypeScript typecheck and Vite build. This is a loopback/native integration gate, not a paid Codex account test or frontier benchmark.

### Provider-reported autonomous job cost telemetry (sub3, 2026-11-15)

The native Rust model adapter now keeps a **durable per-job measured-token ledger** (`job.nativeModelUsage`) including input/output, cache read/write, reported/unreported provider calls and last-provider identity. It handles the important protocol difference: Anthropic's separately billed cache-read/cache-create input contributes to total SDK token budget, whereas OpenAI Chat/Responses cache reads are already included in total input. Streaming Chat/Anthropic cache counters survive the incremental event assembler; incomplete provider usage remains **unknown**, never free. The native-work GUI shows measured totals and an explicit missing-usage warning, while `@dominion-ai/sdk` exports the same typed job data. Monetary charges are deliberately unestimated without an account's actual provider price. [Architecture and tests](docs/SUB3-NATIVE-METERING-2026-11-15.md). Focused [Actions run #43691823640](https://github.com/hushmind3/dominion-rust/actions/runs/43691823640) succeeded: **115/115 Rust tests**, **7/7 standalone TypeScript SDK tests**, React TypeScript and Vite build. The Codex native job WebSocket also now omits the HTTP-only `stream` property from `response.create`, following the 2026 Responses WebSocket protocol.

The native-work GUI also exposes its existing Rust SDK **token budget** setting as a simple advanced control. It sends `input.tokenBudget` through the live `/api/jobs` API, shows the kernel's `chargedTokens` against the configured budget, and displays measured provider tokens separately. The restart flow retains the prior task's budget.

Latest UI-budget verification: [Actions #43692151683](https://github.com/hushmind3/dominion-rust/actions/runs/43692151683) — **115/115 Rust**, **7/7 TypeScript SDK** cases, frontend typecheck/build successful.
