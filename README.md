# DOMINION — Rust agent workspace (dominion-sub2)

<p align="center"><img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" /></p>
<p align="center"><strong>Your models. Your workspace. Native Rust execution.</strong></p>
<p align="center"><a href="README.ko.md">한국어</a> · English · <a href="ENGINE.md">Architecture</a> · <a href="LICENSE">Apache-2.0</a></p>

This document describes the **experimental `dominion-sub2` branch**, checked against source on **2026-11-19**. The complete React/Vite frontend has been synchronized with main's 2026-11-19 interface (`f86a36e`), while the Rust runtime and SDK retain sub2-specific execution, evidence, cache and recovery work. This is **not** a claim that the sub2 backend is identical to main or that a production desktop release has been verified.

<p align="center">
  <img src="docs/01-multi-agent-room.webp" alt="Group conversation workspace" width="48%" />
  <img src="docs/02-chat-workspace.webp" alt="Local agent chat" width="48%" />
</p>

## What is available

- **Rust-native agent runs:** tool calls, policy-bound approvals, effect receipts, test evidence, cancellation and recovery. The Rust kernel is the execution owner.
- **Multi-agent rooms:** shared SQLite transcript, distinct participant identities, precise self-recall, owner facts, per-agent context windows and observable token accounting.
- **2026 main frontend:** model management/Hugging Face exploration, provider picker, GitHub/connected accounts, conversations and scoped live refresh from the main web tree.
- **Public model discovery without an API key:** read-only provider models via `/api/providers/catalog/:id/models` and verified OpenRouter free entries via `/api/providers/openrouter/free-models`. Actual model inference still follows the selected provider's authentication rules.
- **GitHub CLI connection:** the main UI flow calls native CLI login; the Rust backend validates GitHub account credentials and stores the grant in the encrypted vault.
- **Extensible tools and SDK:** native workspace/file/shell/browser/MCP execution, TypeScript client at `sdk/typescript/`, Rust kernel at `crates/dominion-agent-kernel/`.

The former standalone Money discovery/business-opportunity module **has been removed** from the frontend, HTTP routes, Rust engine, startup workers, DB schema initialization, active tests and feature-specific documents. The application does not silently delete user data from existing SQLite databases.

## Development

Prerequisites: a stable Rust toolchain and Node.js 24.12+ for the synchronized frontend.

```bash
npm ci --prefix web
npm --prefix web run typecheck
node --experimental-strip-types --test web/tests/*.test.mjs
node scripts/check-sub2-contract.mjs
cargo test --lib
cargo run -- --home .
```

Start the UI separately with `npm --prefix web run dev`. A release package and live-model performance must be verified against the target machine/provider; a passing unit test is not a real-model benchmark.

## Execution/source map

| Boundary | Active source |
| --- | --- |
| Main-synchronized frontend, routing and pages | `web/src/App.tsx`, `web/src/pages/`, `web/src/components/` |
| HTTP API, public model catalog and GitHub login | `src/api/mod.rs`, `src/api/providers.rs`, `src/api/cli_oauth.rs` |
| Chat turn execution and tool dispatch | `src/engine/chat/runtime.rs`, `src/engine/chat/runtime/turn_body.rs` |
| Room context and semantic continuation | `src/engine/chat/room_context.rs`, `src/engine/chat/continuity.rs` |
| Room status, owner facts, speaker routing | `src/engine/room_state.rs`, `src/engine/rooms.rs` |
| Native tool contracts, verification | `src/engine/chat/tool_projection.rs`, `src/engine/verification.rs` |
| Durable jobs and SQLite storage | `src/engine/jobs.rs`, `src/db.rs` |
| User SDK and Rust kernel | `sdk/typescript/src/index.ts`, `crates/dominion-agent-kernel/src/lib.rs` |

See [ENGINE.md](ENGINE.md), [documentation index](docs/INDEX.md) and [development CI](.github/workflows/ci.yml). This branch deliberately retains its own Rust design; main's cloud/backend modules have **not** been copied indiscriminately.

## Verification boundary

CI includes Rust regression tests, TypeScript checking, main UI unit tests and an active-source/API route contract gate. Actual browser interaction, desktop packaging, connected-account authorization and full nine-agent token benchmarks require independent live integration checks. Provider prompt-cache hints and context-frame reuse are not evidence of discounted billed tokens without provider usage receipts.
