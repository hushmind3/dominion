# DOMINION

**Bring your own model. Run your own agents. Own your workflow.**

<p>
  <img src="src-tauri/icons/icon.png" alt="DOMINION" width="80" />
</p>

An open-source, local-first AI agent workspace — a native Rust execution engine with a Tauri desktop app.
Point it at your own API keys or a local model, then let tool-using agents read and edit code, run commands,
search the web, call MCP servers and work together in multi-agent rooms. Every file write and shell command
waits for your approval, and all state stays in a SQLite database on your machine.

[English](README.md) · [한국어](README.ko.md)

[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
![Engine: Rust](https://img.shields.io/badge/engine-Rust-262626)
![Desktop: Tauri 2](https://img.shields.io/badge/desktop-Tauri%202-24C8DB)
![UI: React](https://img.shields.io/badge/UI-React-149ECA)
![Models: BYOK + local](https://img.shields.io/badge/models-BYOK%20%2B%20local-6855D8)

[Screenshots](#screenshots) · [Features](#what-you-can-do) · [Models](#models) · [Install](#quick-start) ·
[Architecture](#architecture) · [Status](#project-status) · [Discussions](https://github.com/hushmind3/dominion-rust/discussions)

---

## Screenshots

<!-- TODO before publishing: re-capture these from the current build. The first image still shows the
     retired "영상 제작" sidebar entry and no Money entry, so it is older than the current navigation. -->

**Multi-agent room — several agents share one conversation, with per-speaker token accounting**

<p>
  <a href="docs/01-multi-agent-room.webp">
    <img src="docs/01-multi-agent-room.webp" alt="DOMINION multi-agent room with participants, shared conversation and room controls" width="100%" />
  </a>
</p>

| Chat workspace | Agent studio |
| --- | --- |
| [<img src="docs/02-chat-workspace.webp" alt="DOMINION chat workspace with model selector and task input" width="100%" />](docs/02-chat-workspace.webp) | [<img src="docs/03-agent-studio.webp" alt="DOMINION agent studio with agent cards, roles and model settings" width="100%" />](docs/03-agent-studio.webp) |
| Pick a model and give it a task. | Give each agent its own role, instructions and model. |

<sub>Captured from a development build of the Korean UI. Screenshots show the interface, not a claim of
real-model reliability.</sub>

## What you can do

| Feature | What it actually does |
| --- | --- |
| **Your models, your keys** | Over 30 provider presets — OpenAI, Anthropic, Gemini, DeepSeek, Mistral, xAI, OpenRouter, Groq, Cerebras, Together, Perplexity, GitHub Models and more. No DOMINION account, no token resale. |
| **Local models** | Ollama, LM Studio, llama.cpp (`llama-server`, GGUF including split files), vLLM, LiteLLM and MLX on macOS. Model download and install are handled in **Models**. |
| **Subscription logins** | Already paying for ChatGPT or a CLI agent? Codex ChatGPT OAuth and `claude` / `gemini` / `opencode` CLI logins can be used as inference transports. DOMINION still owns the tools, approvals and history — the CLI is only a text generator. |
| **Tool-using agents** | Workspace and absolute-path file tools, line-range edits guarded by SHA-256, shell and terminal execution, web search, HTTP fetch, headless Chrome control, GitHub read/commit, MCP tools and skills. |
| **Approvals and receipts** | Writes, shell commands, browser actions and GitHub commits raise an approval request. Cancelled batches record aborted receipts instead of silently dropping calls; uncertain external effects pause for review. |
| **Durable jobs** | Each turn is a job with checkpoints, cancellation and recovery in SQLite. Once stored tool receipts satisfy the task's postconditions the run can finish without another model round. `POST /api/sdk/runs` accepts an `idempotencyKey` so a retried request returns the original run (and 409 if the arguments changed) instead of executing twice. |
| **Multi-agent rooms** | Up to 32 participants, speaker order, mentions, shared room knowledge and per-speaker prompt/cache token accounting. Agents can also delegate to each other with isolated Git worktrees and role profiles (`general`, `inspection`, `implementation`, `verification`). |
| **Memory** | Local SQLite memory — no embedding service, no external memory server. The default `evidence-lineage-2026` reranker scores candidates by the provenance it can verify, and the model must quote your exact words (`evidenceQuote`) to store a fact, so invented memories are rejected. |
| **Attachments** | Drop images (PNG/JPEG/GIF/WebP) or UTF-8 text files up to 8 MiB into a chat; they are stored locally with the session and served back by the engine. |
| **Messaging channels** | Telegram bot polling and Discord adapters. Both need your own bot token and destination IDs configured by hand. |
| **Money research tool (off by default)** | Collects public RSS, GitHub and Hugging Face signals on a local schedule with zero model tokens, then turns them into source-linked business hypotheses. Enable in **Settings → Money tool**. |

## Models

Anything that speaks the OpenAI chat-completions API works: pick a preset or add a base URL. Anthropic is
supported natively. For local inference, install the runtime once and DOMINION finds the models on disk:

- **Ollama** — start `ollama serve`, add the local preset.
- **LM Studio / vLLM / llama.cpp / LiteLLM** — point at the local server port (LM Studio 1234, llama.cpp 8080, vLLM 8000, LiteLLM 4000).
- **MLX** — macOS only, models downloaded into the app's data directory.

Cloud tokens are billed by the provider you chose; local models use your own hardware. Keys are sealed with
ChaCha20-Poly1305 in a local key vault — the app never shows them again after saving.

## Quick start

DOMINION runs on **macOS 13.3+** and **Windows** (WebView2). Linux is not a supported target yet.

**1. Install the released desktop app.** Download the latest installer from
[Releases](https://github.com/hushmind3/dominion-rust/releases/latest) — currently **v0.4.3**:

| Platform | File |
| --- | --- |
| macOS, Apple Silicon | `DOMINION_0.4.3_aarch64.dmg` |
| macOS, Intel | `DOMINION_0.4.3_x64.dmg` |
| Windows, installer | `DOMINION_0.4.3_x64-setup.exe` |
| Windows, MSI | `DOMINION_0.4.3_x64_en-US.msi` · `DOMINION_0.4.3_x64_ko-KR.msi` |

These builds are unsigned, so macOS Gatekeeper and Windows SmartScreen ask you to confirm on first launch.
The app keeps its SQLite database, keys, logs and task workspace in your OS app-data folder — nothing is
written into the installed bundle.

**2. Build the desktop app yourself (Tauri).** This is the product; the window hosts the Rust engine as a
bundled sidecar.

```
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
node scripts/build-desktop.mjs
```

Requirements: Git, Rust stable with Cargo, Node.js **24.12+**, and your platform's C/C++ build tools
(on Windows, MSVC with the C++ desktop workload and the Windows SDK). The bundle lands in
`src-tauri/target/release/bundle/` — `macos/DOMINION.app` plus a DMG on macOS, MSI and NSIS installers on
Windows. See [docs/DESKTOP.md](docs/DESKTOP.md) for signing, data locations and sidecar/port behaviour.

**3. Browser mode (engine + Vite bundle).** Faster loop while developing:

```
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home . --open
```

Then open <http://127.0.0.1:4317>.

### First five minutes

1. **Models** → add a cloud provider with your key, or a local runtime.
2. **Chat** → choose the model, give it a task, watch tools and approvals appear inline.
3. **Agents** → give an agent a role and instructions; it keeps its own memory and model.
4. **Rooms** → put several agents in one room and let them discuss a task.
5. **Connections** → add a Telegram or Discord bot if you want to talk to agents from your phone.

More detail: [setup guide (Korean)](docs/GETTING-STARTED.md) · [desktop build](docs/DESKTOP.md) · [account connections](docs/ACCOUNT-CONNECTIONS.md).

## Architecture

```
React + Vite UI  ──  Tauri 2 desktop shell (macOS / Windows)
        │                        │
        ▼                        ▼
Rust HTTP API (Axum, 127.0.0.1)  ──►  SQLite (state, memory, jobs, receipts, logs)
        │
        ▼
Native Rust agent engine (crates/dominion-core + product adapters)
        ├── chat, streaming, tool calls, approvals, cancellation, recovery
        ├── memory, experience, rooms, delegated jobs, verification
        ├── skills, MCP (stdio + Streamable HTTP), Telegram, Discord
        └── model adapters: cloud APIs, local runtimes, CLI logins
```

The agent runtime is Rust; Node only builds the interface. Chat, rooms, scheduled jobs and the HTTP SDK all
execute through the same engine and the same SQLite journal, so there is no second runtime to reason about.

### What runs the loop

The agent is not a prompt wrapper around a search score. `crates/dominion-core` owns a bounded
**initialize → plan (when needed) → model → tool admission and execution → verification → completion** loop,
and [`src/engine/chat/product.rs`](src/engine/chat/product.rs) connects it to your provider, workspace,
approvals and SQLite receipts.

| Layer | What is in the repository |
| --- | --- |
| **Kernel and safety** | One shared Rust driver, native snapshots and tool intents, shared token/cost budgets. Cancellation, a pending approval or an unresolved external effect blocks completion. |
| **Planning** | A registry of versioned, executable algorithms — `acceptance-lineage-dag-2026` for task decomposition and strategy selection. Archived experimental records are history, not an autonomous self-modifying engine. |
| **Verification and repair** | Frozen postcondition checks, a per-resource obligation frontier, receipt-based recovery and bounded causal diagnostics. A model that says the work is done does not complete the task. |
| **Memory and context** | `evidence-lineage-2026` reranking over a local SQLite corpus, owner-confirmed room state and source-scoped room context. BM25 remains as one candidate-retrieval kernel, not as the harness policy. |
| **Token reuse** | Provider-specific request assembly, Codex incremental WebSocket transport, and an append-only room projection that keeps a verified prefix and sends only newly selected owner facts. Corrections, revocations and instruction changes force a rebuild. |

Two consequences worth knowing:

- **Verified-checkpoint completion.** When real tool receipts already satisfy every frozen postcondition, the
  kernel can finish without buying another model round. It needs attributable persisted receipts, no
  unresolved effects and a current owner revision.
- **A reusable prefix is not a cache hit.** Prompt, cached-prompt and new-prompt tokens are recorded per
  execution, and only from provider accounting.

**TypeScript client:** [`packages/sdk-typescript`](packages/sdk-typescript/README.md) is a thin HTTP client over
that engine, not a second harness. `waitForRun` stops at completion, approval and manual-recovery boundaries;
`waitForVerifiedRun` only returns success for `taskOutcome: "succeeded"` backed by independent verification evidence.

**Deep dives:** [engine](ENGINE.md) · [harness contract](docs/HARNESS.md) ·
[implementation notes moved off this page](docs/HARNESS-DETAIL-2026-11-18.md) · [docs index](docs/INDEX.md).

## What the agents can do — and what they cannot

Observational tools (list, read, search, status) run immediately. Anything that mutates something asks first:

| Needs approval | Runs without asking |
| --- | --- |
| `shell_exec`, `terminal_exec` | `workspace_list`, `workspace_read`, `workspace_read_range`, `code_search` |
| `workspace_write`, `workspace_replace`, `file_write` | `web_search`, `http_fetch` |
| `browser_control` (open, click, type, screenshot) | `memory_search`, `skill_view`, `task_status`, `task_result` |
| GitHub commits and `github_api` write methods | `workspace_git_status`, `workspace_git_diff`, `github_file_read` |

Delegated child jobs inherit the parent's permission, never widen it, and share one token/cost budget.
A verification child can only run approved Cargo tests inside a sandboxed worktree.

## Measured, not claimed

This repository contains two measurements. Neither is a product-wide benchmark.

**Retrieval inclusion (2026-11-06).** On 21 synthetic memories, a free API model and a local 3B model were asked
3 questions each. With no reranking kernel the correct memory reached the model **0/6** times; with the
then-default BM25 kernel it reached the model **6/6** times. Answer accuracy stayed noisy, and the document says
so. This was run on the experimental `codex/native-memory-harness` branch with the BM25 kernel; the default
reranker today is `evidence-lineage-2026`, and no equivalent measurement has been taken for it. See
[docs/REAL-MODEL-EXPERIMENT-2026-11-06.md](docs/REAL-MODEL-EXPERIMENT-2026-11-06.md).

**Group-room cost and recall (2026-11-09).** The nine-agent, 100-owner-turn run
[`ROOM-100-TURN-LUNA`](benchmark/ROOM-100-TURN-LUNA-2026-11-09.json) reported **18.15 %** provider cache reuse
and failed long-range recall on its experimental branch. The reuse work since then — append-only verified room
state, the Codex incremental transport — has regression tests but no new measured cache-hit rate, recall
improvement or cost saving.

Nothing else here is a benchmark: no published token-savings percentage, no self-improvement claims, and no
quality numbers that were not measured on this machine.

## Optional: Money research tool

Off for new installs (**Settings → Money tool**). It collects public RSS, GitHub and Hugging Face signals into
indexed SQLite without spending model tokens, keeps source versions content-addressed and immutable, and checks
model-proposed quotations against archived page text. Research never authorizes outreach, spending or
implementation; those stay outside its scope. See [docs/MONEY.md](docs/MONEY.md).

## Project status

DOMINION is an **active developer preview**, not a managed service and not a validated commercial agent product.

- **Installers are unsigned.** v0.4.3 ships macOS DMGs and Windows MSI/NSIS installers, but without Apple or
  Windows signing certificates, so first launch needs a manual confirmation. No earlier version was published.
- **macOS and Windows only.** The engine is portable, but CI and installers target those two.
- **Focused CI, not full validation.** Routine pushes run a scoped Rust regression set on macOS, and only when
  engine, crate or a handful of named web files change. The frontend build, release-profile tests and both
  installers run only in `release.yml`, for a version tag or a manual run. A green check on a commit does not
  mean a distributable build was verified. See [Actions](https://github.com/hushmind3/dominion-rust/actions).
- **Real-model reliability is unproven at length.** Room and delegation paths are covered by mocked-model tests;
  long multi-hour real-model runs are not established.
- **Channels need manual setup.** Telegram and Discord require your own bot credentials; there is no one-click bot creation.
- **Retired features.** Built-in video generation and autonomous RSI campaigns are gone; archived records are
  read-only (`GET /api/rsi/*` returns `retired: true`). Use MCP tools for media generation.
- **Approvals are deliberate.** Every mutation asks first; unattended, hours-long autonomy is not the design goal.

## Contributing

The most useful contributions are reproducible bug reports, test cases, model/provider compatibility fixes,
Rust engine improvements, UI work and documentation.

- [Discussions](https://github.com/hushmind3/dominion-rust/discussions) — setups, questions, workflow ideas.
- [Issues](https://github.com/hushmind3/dominion-rust/issues) — include OS, model/provider, reproduction steps, expected vs. actual.
- [Pull requests](https://github.com/hushmind3/dominion-rust/pulls) — explain the change and add tests where it matters.
- Never paste API keys, bot tokens or private conversations into an issue.

## License

[Apache 2.0](LICENSE) · [한국어 README](README.ko.md)
