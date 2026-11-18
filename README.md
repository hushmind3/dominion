# DOMINION

<p align="center">
  <img src="src-tauri/icons/icon.png" width="88" alt="DOMINION icon" />
</p>
<p align="center"><strong>Bring your model. Keep control of your agents, tools and data.</strong></p>
<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> ·
  <a href="ENGINE.md">Architecture</a> · <a href="sdk/typescript/README.md">TypeScript SDK</a>
</p>
<p align="center">
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue" /></a>
  <img alt="Engine: Rust" src="https://img.shields.io/badge/engine-Rust-262626" />
  <img alt="UI: React" src="https://img.shields.io/badge/UI-React-149ECA" />
  <a href="https://github.com/hushmind3/dominion-rust/actions/workflows/ci.yml?query=branch%3Adominion-sub2"><img alt="sub2 development CI" src="https://img.shields.io/github/actions/workflow/status/hushmind3/dominion-rust/ci.yml?branch=dominion-sub2&label=sub2%20dev%20tests" /></a>
</p>

**DOMINION** is a local-first, open-source agent workspace. The **native Rust engine** owns model calls, tool execution, approvals, cancellation, durable jobs, verification and recovery. The React UI, HTTP API, embeddable Rust SDK and optional TypeScript SDK are interfaces to that same engine—not separate agents.

**This README describes the experimental \`dominion-sub2\` branch as of 2026-11-18**, not the default \`main\` branch or a verified production release. Connect your own model/provider; DOMINION does not resell model tokens.

[Quick start](#quick-start) · [Features](#features) · [Architecture](#architecture-and-source-map) · [SDKs and API](#sdks-and-api) · [Honest limits](#verification-and-limitations) · [한국어](README.ko.md)

## Screenshots

**Multi-agent room**

![Nine-agent conversation workspace and room controls](docs/01-multi-agent-room.webp)

| Chat | Agent configuration |
| --- | --- |
| ![Chat workspace](docs/02-chat-workspace.webp) | ![Agent studio](docs/03-agent-studio.webp) |

*Development-build screenshots; an interface screenshot is not proof of live-model benchmark success.*

## Features

| Capability | What is implemented | Where |
| --- | --- | --- |
| **Local & bring-your-own-model** | Configured local inference (GGUF / llama.cpp, Ollama, compatible runtimes) and external API providers; model features depend on the chosen backend | \`src/engine/models.rs\`, \`src/engine/chat/provider.rs\` |
| **Agent chat** | Streaming messages, native tool calls, approval gates, per-request Stop and per-session persisted history | \`web/src/pages/Chat.tsx\`, \`src/api/chat.rs\`, \`src/engine/chat/\` |
| **Autonomous jobs** | User goals, bounded execution, explicit verification, checkpoints, review states, cancel/retry and owner steering | \`src/engine/jobs.rs\`, \`src/engine/autonomy.rs\`, \`src/engine/verification.rs\` |
| **Multi-agent rooms** | Individual personas, room-wide owner facts, selected peer context and independent participant continuations | \`src/engine/rooms.rs\`, \`src/engine/chat/room_context.rs\` |
| **Context efficiency** | Stable shared room prefix, bounded context selection, cached-token telemetry, Codex WebSocket incremental continuation where available | \`src/engine/chat/context.rs\`, \`continuity.rs\`, \`codex_ws.rs\` |
| **Skills / MCP / discovery** | Permission-scoped native tools, progressive tool schemas, enabled-skill index and on-demand \`skill_read\` with pagination | \`src/engine/tools.rs\`, \`src/engine/chat/capability.rs\` |
| **Money research** | Optional evidence gathering, article provenance, candidate comparison, hypotheses and explicit owner review; no automatic business purchase/deployment | \`src/engine/money/\`, \`src/api/money/\` |
| **External integrations** | Configured connections such as Telegram, Discord, GitHub and MCP; credentials/permissions required | \`src/engine/telegram.rs\`, \`discord.rs\`, \`mcp.rs\` |
| **SDKs** | Embeddable Rust lifecycle and typed TypeScript HTTP client | \`src/sdk.rs\`, \`sdk/typescript/\` |

**Product scope:** The built-in video/audio rendering pipeline and autonomous generational RSI campaigns were retired **on this branch**. Media generation can be invoked through user-configured MCP/Connections. Historical SQLite rows, chat, archived jobs and existing media files remain readable; there is no background self-modifying-core campaign.

## Quick start

**Source checkout** for macOS or Windows. Install Rust stable/Cargo, Git, Node.js **24.12+**, and native C/C++ toolchain prerequisites (MSVC Build Tools + Windows SDK on Windows).

\`\`\`bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
git switch dominion-sub2
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home .
\`\`\`

Open **http://127.0.0.1:4317**. Your first step is to configure a model/provider in **Models**; then open **Chat** or **Home → Autonomous work**. Browser UI assets are built from \`web/\`; Node.js is **not** the production agent runtime.

- Development Rust tests: \`cargo test --lib engine::chat::continuity::tests\`, \`cargo test --lib engine::tools::tests\`, \`cargo test --lib sdk::tests\`, \`cargo test -p dominion-agent-kernel --lib\`.
- Optional frontend source validation: \`npm --prefix web run typecheck\`.
- Desktop packaging is a separate release workflow; a passing development test job does not verify macOS/Windows installers. See [installation](docs/GETTING-STARTED.md) and [desktop packaging](docs/DESKTOP.md).

By default the server binds to loopback. Data, model downloads and keys are stored under the configured local data location (\`DOMINION_DATA\`, normally \`runtime/\` for this checkout). Override \`DOMINION_HOME\`, \`DOMINION_WORKSPACE\`, \`DOMINION_WEB\` and \`DOMINION_PORT\` when needed. Back up SQLite, the secret key and referenced workspace files together. Never commit secrets or \`runtime/\`.

## Architecture and source map

\`\`\`text
 React / Vite UI ────────────┐
 TypeScript @dominion/sdk ───┼──► Axum /api/* ───────────────┐
 External API callers ───────┘                               │
 Embedded Rust callers ─────────────► src/sdk.rs ────────────┤
                                                              ▼
                                      Native Rust job/chat/room engine
                                      ├── dominion-agent-kernel (limits/decisions)
                                      ├── tool admission → approvals → effects
                                      ├── verifier → receipt → job state
                                      ├── context / memory / skills / MCP
                                      └── cancellation / recovery / event bus
                                             │             │
                                             ▼             ▼
                                        SQLite state   Provider APIs /
                                                       local inference
\`\`\`

| Layer | Entry point | State / contract |
| --- | --- | --- |
| HTTP routes | \`src/api/mod.rs\` | Axum route composition; same job engine, no duplicate SDK worker |
| Native SDK & bootstrap | \`src/sdk.rs\`, \`src/bootstrap.rs\` | \`Dominion::open/start/submit/steer/wait\`; Tokio runtime required for \`start\` |
| Chat, provider stream | \`src/engine/chat/runtime.rs\`, \`runtime/turn_body.rs\`, \`provider.rs\` | Durable user/assistant messages, model usage and tool receipts |
| Jobs & harness | \`src/engine/jobs.rs\`, \`autonomy.rs\`, \`crates/dominion-agent-kernel/\` | Per-job state and review-safe verification; one owner for execution |
| Source-of-truth / recovery | \`src/db.rs\`, \`src/state.rs\`, \`src/engine/effect.rs\` | SQLite persistence, owner/epoch fencing, uncertain-effect reconciliation |
| Room context | \`src/engine/rooms.rs\`, \`chat/room_context.rs\`, \`room_state.rs\` | Shared owner decisions + persona-specific context, source pointers retained |
| User interfaces | \`web/src/pages/\`, \`web/src/components/NativeAgentPanel.tsx\` | Chat, models, jobs, agent configuration, money and integrations |

The repository contains historical documentation under \`docs/\` and \`benchmark/\`. Old experiments are retained as evidence, not active runtime features.

## SDKs and API

The **Rust SDK is the native engine interface**:

\`\`\`rust
use dominion_rust::sdk::{Dominion, EngineOptions, Prompt};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), String> {
    let sdk = Dominion::open(EngineOptions::new(".", "./runtime"))?;
    sdk.start()?;
    let job = sdk.submit(Prompt::new("Inspect and verify a repository change"))?;
    let result = sdk.wait(job["id"].as_str().unwrap(), Duration::from_secs(120)).await?;
    println!("{}", result["status"]);
    Ok(())
}
\`\`\`

The independent **[TypeScript SDK](sdk/typescript/README.md)** (\`sdk/typescript/\`, package name \`@dominion/sdk\`) is a typed HTTP client, not another execution runtime. It requires a running DOMINION server. A minimal external client creates a prompt job with \`jobs.create()\` and awaits \`waitForJob()\`.

Useful native routes:

| Action | API |
| --- | --- |
| Stream chat | \`POST /api/chat\` |
| Stop the current chat request | \`POST /api/sessions/:id/cancel\` with \`{ "requestId": "<UUID>" }\` |
| Confirm stop settlement | \`GET /api/sessions/:id/cancel-status\` |
| Create / inspect job | \`POST /api/jobs\`, \`GET /api/jobs/:id\` |
| Cancel / steer job | \`POST /api/jobs/:id/cancel\`, \`POST /api/jobs/:id/steer\` |
| Home autonomous-work interface | \`/api/sdk/runs\` and its ID-specific approval, cancel, resume and reconcile endpoints |
| Durable execution evidence | \`/api/executions/:id/operations\` |

\`POST /api/jobs\` supports an optional \`input.verifyCommand\`, \`input.tokenBudget\` (1,024–2,000,000, *admission estimate*, not a strict billing guarantee) and explicit validated \`input.successCriteria\`. The Rust \`Prompt\` struct has its documented separate fields; **do not assume every HTTP field is available as a Rust SDK struct field**. A file's existence does not by itself justify marking a risky external side effect as verified.

**Stop semantics:** A chat Stop records a request cancellation on the server, cancels the Rust execution epoch, and waits until the session and execution settle before re-enabling Send. When a remote provider has already accepted an HTTP call, it may still compute/charge work before observing disconnect. External effects that cannot be proved settled require review; a browser AbortSignal alone is not proof of remote cancellation. [Cancellation and cache details](docs/SUB2-CHAT-STOP-CACHE-2026-11-18.md).

## Context, cache and cost measurements

DOMINION distinguishes three values:

1. **Total provider input tokens:** what the provider reports, including any cached input.
2. **Cached input tokens:** reported only when that provider supplies cache-hit data; absence is **unknown**, not zero.
3. **Uncached input tokens:** total minus cached **only when both are available**.

For Codex **WebSocket**, incremental transcript reuse depends on a live provider-side response chain; only **confirmed** previous WS use permits a larger, finite retained-history horizon. A provider change, edited source, revoked evidence, restart, or HTTP fallback can require a full rebase. **Cloudflare, other HTTP providers and stateless local backends do not inherit an expanded WS-only history budget.**

Skills send a compact index when native tools are enabled; approved details are fetched on demand via \`skill_read({skillId, offset?})\`. Long skills are paginated with \`nextOffset\`, not silently cut off. Tool-disabled conversations retain bounded instructions instead of pointing to an unavailable tool.

**Historical, not current proof:** [100-owner-turn room benchmark, 2026-11-09](benchmark/ROOM-100-TURN-LUNA-2026-11-09.json) recorded 2,630,782 prompt tokens, 18.15% cache hit rate, 621 history-budget rebases and **0/9 late recall** at its listed checkpoints. The [2026-11-08 WS experiment](benchmark/CODEX-WEBSOCKET-INCREMENTAL-2026-11-08.json) reported a 76.9% third-turn **transcript wire reduction**, not a reliable billed-cost reduction. These measurements predate current sub2 code. **No new 100-turn recall or live cache-efficiency result has been established by source changes alone.**

## Verification and limitations

- **Development checks:** the \`dominion-sub2\` [Rust development test workflow](.github/workflows/ci.yml) runs scoped Rust regressions; it also has a separate frontend TypeScript typecheck job. This is **not** full release CI or a live-model acceptance benchmark.
- **Quality:** job \`status=completed\` and a user's verified output are distinct. A correct file can still have an unresolved verification gap (\`needs_review\`); records preserve source receipts and outstanding conditions.
- **Cost:** provider prices, cache behavior, throttling and model-specific tool support vary. Usage estimates are not invoices. Native approval and concurrency controls do not roll back remote effects.
- **Operations:** no bundled cloud service, no automatic hosted inference and no guaranteed prebuilt installer. Telegram/Discord/MCP and local model backends need their own configuration.

For the detailed scope and known gaps, consult the [2026-11-18 source audit](docs/SUB2-SOURCE-AUDIT-2026-11-18.md), [harness verification repair](docs/SUB2-HARNESS-DECISION-REPAIR-2026-11-18.md), [context/caching](docs/SUB2-HARNESS-CONTEXT-CACHE-2026-11-18.md) and [Money provenance](docs/SUB2-MONEY-SOURCE-LINEAGE-2026-11-18.md).

## Development and contribution

- Technical architecture: [ENGINE.md](ENGINE.md) · [documentation index](docs/INDEX.md)
- Frontend: \`web/\` · Rust native SDK: \`src/sdk.rs\` · TypeScript SDK: \`sdk/typescript/\`
- Tests: \`src/**\` unit tests, \`crates/dominion-agent-kernel/\`, \`tests/e2e/\` (run separately for release/acceptance)
- File a reproducible issue with the **branch SHA, model/provider, actual job status, masked logs, code path and evidence**. Never include API tokens, OAuth data, private prompts or personal records.

Licensed under [Apache-2.0](LICENSE). Contributions, focused tests and evidence-backed bug reports are welcome.
