# DOMINION

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" />
</p>

<h3 align="center">Bring your own model. Run your own agents. Own your workflow.</h3>

<p align="center">
  <strong>An open-source, local-first AI agent workspace powered by a native Rust execution engine.</strong><br/>
  Connect your own cloud or local LLMs. Run tool-using agents, collaborate across multiple agents, connect external media tools through MCP, and complete verified workflows.
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

## Native Rust execution core

The product embeds [`dominion-core`](crates/dominion-core/README.md), adapted from `dominion-sub4`, behind the existing SQLite store, React frontend and provider/tool adapters. Chat, rooms, jobs, workflows and the Home autonomous-work panel use the same product execution path. `/api/sdk/runs` uses that same execution path and SQLite journal. See [the execution contract](docs/HARNESS.md).

Background SDK, job, and Discord turns drain transient events without retaining an additional in-memory copy of tool arguments and results. Durable tool receipts and opt-in evidence collection are unchanged.

Existing product data and authentication remain in SQLite and its credential vault. Tool intent, receipts and uncertain-effect recovery use the native Rust state machine; cancelled sequential tool batches now record aborted-before-dispatch tool receipts rather than silently dropping queued call IDs. Recovery treats opaque MCP actions and completed or failed external mutations conservatively: whole-turn replay is withheld pending review, while recognized read-only operations remain replayable. Execution cancellation persists both the terminal status and phase, and phase updates use conditional SQLite patches instead of overwriting concurrent evidence. Product-specific approval and verification policies remain in the main application. These source-level changes do not establish real-model quality gains or general self-improvement.

### Durable worker checkpoint safety

Queued and running job checkpoints now commit under one SQLite transaction that checks both the live job status and cancellation intent. A worker finishing after cancellation or an earlier terminal result cannot rewrite that record. This does not add a second execution engine or extra DB lookup. The regression test covers completed, failed, cancelled, review-required, and cancellation-requested jobs. CI runs release workspace tests on Windows and macOS only, per current product targets.

## Development vs release validation

Normal Rust source pushes run only scoped, default-profile kernel/job/Money regression tests on macOS via `.github/workflows/ci.yml`. They do **not** trigger frontend builds, `cargo build/check`, release-profile builds, installer packaging, or the Windows/macOS release matrix. The separate `.github/workflows/release.yml` performs those full checks only for version tags or an explicit manual release request. Passing scoped tests does not imply that a distributable desktop release has been validated.

## 💰 Money intelligence

Money cards also allow the owner to explicitly authorize a prototype implementation through the **same native SDK run endpoint** used elsewhere in the product. This binds the selected agent persona, preserves a generic source link across reloads, and exposes native approvals/cancellation/recovery without introducing a separate Money runner. Public sales, purchases, outreach and deployment are outside that MVP authorization.


Money discovery collects public RSS, GitHub and Hugging Face signals with the existing Rust worker queue and stores the evidence in indexed SQLite. It produces source-backed business hypotheses without model tokens and offers opt-in AI inference through the existing durable job kernel. Research never authorizes outreach, spending or implementing a business.

[Money architecture and limitations](docs/MONEY.md). No separate execution kernel or autonomous source-code modification system is introduced.

## What is DOMINION?

**DOMINION is an AI agent tool, not another AI model or a token-reselling service.**

Connect a model you already have access to — including supported cloud APIs and locally hosted models — and use it inside an agent workspace. DOMINION's Rust harness handles tool execution, permissions, durable tasks, long-term memory, and multi-agent coordination. Native media generation and autonomous RSI campaigns have been retired; use external MCP tools for media generation and ordinary verified agent tasks for code changes.

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
| **Messaging channels** | Native Telegram and Discord adapters; currently require bot credentials and manual configuration. |

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
       ├── Verified outcomes / explicit error recovery / durable checkpoints
       ├── User-configured model APIs
       └── Local inference (GGUF / llama.cpp and other configured runtimes)
```

The agent execution engine is written in **Rust**. Node.js is used to build the React frontend rather than to host the agent runtime. Some local-model integrations and MCP servers may require their own processes.

**Native API / SDK:** The TypeScript client in [`packages/sdk-typescript`](packages/sdk-typescript/README.md) uses the same Rust execution path as chat. Its `waitForRun` stops at completion, approval and manual-recovery boundaries; `waitForVerifiedRun` requires `taskOutcome: "succeeded"` with independent verification evidence. SDK run creation persists task limits and the execution owner atomically. A text answer alone is not evidence of an implemented or tested artifact.

**Technical docs:** [Engine](ENGINE.md) · [Documentation index](docs/INDEX.md) · [Agent harness contract](docs/HARNESS.md).

## 🚧 Project status

DOMINION is an **active developer preview**, not a managed SaaS or a fully validated commercial agent product.

- The repository implements chat, tool execution, memory, multi-agent rooms, workflow jobs, and model connections. Real-world results depend on model capabilities and the environment.
- Multi-agent behavior includes mocked-model tests; long-running real-model reliability is still an area to validate.
- Telegram and Discord integration currently require manual bot setup rather than one-click provisioning.
- Cache and context-management code exists, but no fixed token-saving percentage is guaranteed.
- Check the [CI results](https://github.com/hushmind3/dominion-rust/actions) for the status of a particular commit.

## 🤝 Contributing

Contributions are welcome — particularly **reproducible bug reports, test cases, model/provider compatibility fixes, Rust harness improvements, UI work, and documentation**.

- [Join Discussions](https://github.com/hushmind3/dominion-rust/discussions) — ask questions, share your setup, and propose ideas for agent workflows and MCP integrations.
- [Open an issue](https://github.com/hushmind3/dominion-rust/issues) — include your OS, model/provider, reproduction steps, and expected vs. actual behavior.
- [Submit a pull request](https://github.com/hushmind3/dominion-rust/pulls) — explain the change and include appropriate tests when possible.
- Never include API keys, bot tokens, private conversations, or other secrets in reports.

**License:** [Apache 2.0](LICENSE) · **한국어:** [README.ko.md](README.ko.md)
