# DOMINION — Rust agent execution platform

[한국어 안내](README.ko.md) · [Architecture](docs/HARNESS.md) · [Rust SDK](sdk/rust/README.md) · [TypeScript SDK](sdk/typescript/README.md) · [Source audit (2026-11-18)](docs/SUB3-SOURCE-AUDIT-2026-11-18.md)

DOMINION is a local-first AI workspace and **owner-controlled Rust agent execution system**. It connects configured local or remote models to coding tasks, files, shell commands, web/MCP tools, conversation memory, durable job records, approvals, cancellation, and evidence-based verification.

> **Branch:** This document describes `dominion-sub3`, not a released binary or the current `main` branch. Implemented source, focused test evidence, and verified product performance are different things. A real model/provider must be configured to run agent tasks.

## Try it locally

Prerequisites: Rust stable toolchain, Node.js **24.12+** and npm for the web UI. The desktop packaging targets **macOS and Windows**; local development needs a compatible Rust toolchain and OS dependencies.

```sh
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
git switch dominion-sub3
npm ci --prefix web
npm run build --prefix web
cargo run -- --open
```

By default, the Rust process serves the built React UI at **http://127.0.0.1:4317** on loopback. The frontend build is required for this combined-server launch. Configure a model/provider and the tools you authorize in the UI before starting an agent job.

Optional environment settings: `DOMINION_PORT` (default `4317`), `DOMINION_DATA` (default `runtime`), `DOMINION_WEB` (default `web/dist`), and `DOMINION_WORKSPACE` (default current working directory). Runtime data includes `runtime/dominion.sqlite` and encrypted-credential vault material. **Back up the runtime directory** before experimenting with changes or migrations; never commit it.

For frontend development, use `npm run dev --prefix web` alongside the Rust server; for desktop builds, see `scripts/build-desktop.mjs` and `src-tauri/`. The development launch above is **not** an installer or a signed release.

## What is implemented

| Area | Source-backed scope |
| --- | --- |
| Autonomous jobs | Rust `dominion-agent-sdk::Kernel`, host adapters for model/tool execution, bounded planning, durable receipts, owner approvals, cancellation, verification, and guarded continuation. |
| Interactive chat and rooms | Existing streaming chat/room runtime, provider adapters, history/context selection, independent speaker state, and room collaboration. **This remains a distinct top-level execution path from native jobs.** |
| Providers and costs | Configurable local/remote model routes, native function calls where supported, usage records, selected provider cache hints, and Codex WebSocket incremental history where its connection/prefix survives. |
| Tools | Workspace/file/shell actions, selected browser/web and connected-service tools, MCP manifests, scoped Skills and progressively discovered tool schemas. Tools do not receive authority just because a model mentions their names. |
| Memory and recovery | SQLite-backed sessions, correction/learning records, bounded retrieval of past job receipts, cancellation and review of uncertain prior effects. |
| Money (optional) | Source collection, archived original evidence and revisions, hypothesis ranking, review/defer/revisit, and owner-approved implementation through the existing Rust job queue. It is **research support**, not verified revenue. |
| Client integrations | `sdk/rust` re-exports the kernel and provides an HTTP client; `sdk/typescript` is the primary typed HTTP/SSE client; `packages/sdk-typescript` supports the older `/api/harness/runs` contract. |

### One native job kernel, two product entry paths

```text
React work panel / TypeScript SDK / Rust HTTP client
                  │
             /api/jobs or /api/sdk/runs
                  │
         SQLite job queue + native_agent
                  │
      crates/dominion-agent-sdk::Kernel
       ├─ provider and model adapters
       ├─ approved workspace/MCP/host tools
       ├─ receipts, acceptance, checkpoints
       └─ cancellation, approval, recovery

Interactive chat / rooms → /api/chat and room routes
                         → src/engine/chat/runtime/
                         → its existing streaming turn lifecycle
```

The **new** `/api/harness/runs` compatibility requests are translated into the same native job kernel with the narrower legacy workspace/shell authority. Old standalone `sdk_run`/FileJournal records remain **read-only archives**; their uncertain operations are not silently replayed. See [HARNESS.md](docs/HARNESS.md).

## APIs and SDKs

- **Jobs:** `GET/POST /api/jobs`, `GET /api/jobs/:id`, `POST /api/jobs/:id/cancel`, `POST /api/jobs/:id/directive`. Jobs can carry explicit acceptance checks and approved tool limits.
- **Native work panel:** `GET/POST /api/sdk/runs` and `/api/sdk/runs/:id/{approve,cancel,resume,reconcile,steer}`. Read-only evidence tools: `GET /api/sdk/runs/:id/replay`, `POST /api/sdk/runs/:id/compare` and `POST /api/sdk/runs/:id/adjudicate`.
- **Chat and data:** `POST /api/chat`, `/api/sessions`, `/api/providers`, `/api/rooms`, `/api/memory/overview`, `/api/mcp/catalog`, `/api/skill-catalog`.
- **Money:** `GET/POST /api/money/find`, `GET /api/money/archive`, `GET /api/money/source`, `POST /api/money/candidates/:id/review` and `POST /api/money/candidates/:id/implement`. Implementation requires explicit authorization.
- **Compatibility:** `/api/harness/runs` supports new native-backed jobs and archive-only legacy reads, with a restricted capability contract.

```ts
// sdk/typescript/src/index.ts — transport client, not an agent loop
import { Dominion } from './sdk/typescript/src/index'
const api = new Dominion({ baseUrl: 'http://127.0.0.1:4317' })
const run = await api.nativeRuns.start({
  instruction: 'Inspect this workspace and report test failures',
  verifyCommand: 'cargo test',
})
const status = await api.nativeRuns.get(run.id)
// Check status, permissions, and any pending approval before continuation.
```

Rust kernel implementers can import `dominion_sdk::harness::{Kernel, RunRequest, Model, Tools, Evidence, Journal}` via the workspace `sdk/rust` crate. The host supplies the actual model, tool authorization, evidence checks and durable journal; the Rust kernel owns the bounded plan/execute/verify cycle. See [Rust SDK](sdk/rust/README.md).

## Context, Skills, and token accounting

- The native agent provides bounded, task-relevant Skill disclosure; ordinary chat now reuses the same scoped Skill selection. Full instructions/resources can be retrieved through approved `skill_read`/`skill_resource_read` calls.
- Long jobs retain original receipts in the durable journal and expose paginated read-only lookup. The model sees a bounded view without erasing the authoritative source.
- WebSocket incremental send and stable cache prefixes **do not themselves prove provider cache hits or reduced billing**. Host-side reused/new history estimates and provider-reported prompt/cache usage must be compared separately.
- An older nine-agent 100-turn run measured **18.15%** cached prompt tokens and **621 history-budget rebases** on **2026-11-09**. It is a historical baseline, **not a current-branch benchmark**. Details: [context audit](docs/SUB3-CACHE-CONTEXT-2026-11-18.md).

2026 references: [Claude prompt caching](https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything) and [deferred tool loading](https://claude.com/blog/reducing-cost-and-improving-performance-with-claude-platform). These describe upstream mechanisms, not DOMINION performance measurements.

## Product boundaries and data preservation

The standalone RSI generation/campaign/core-auto-promotion engine and built-in video/audio production pipeline have been retired on this branch. External media tools may be connected through user-approved MCP/plugins. **Manual memory review, agent-growth records and stored improvement history remain**; they are not standalone RSI execution engines. Historical SQLite records and already generated user files are not deleted.

Connections to GitHub/Google may use a configured OAuth application or an authorized official CLI flow. Provider consent/configuration is still required; credentials are not bundled. See [connected accounts](docs/CONNECTED-ACCOUNTS-2026-11-17.md).

## Development verification

Routine changes: run *focused Rust development-profile tests*, not a blanket release build.

```sh
cargo test -p dominion-agent-sdk --lib
cargo test --bin dominion-rust engine::chat::context::tests
cargo test --bin dominion-rust engine::skill_discovery::tests
cargo test --bin dominion-rust api::sdk::tests
```

Frontend/API checks and full Windows/macOS builds are **release-stage** checks:

```sh
npm run typecheck --prefix web
npm run build --prefix web
node scripts/check-lean-product.mjs
```

`.github/workflows/ci.yml` is manually dispatched for the full gate; the scoped `sub3-*` workflows run targeted Rust tests. The workflows and historical green checks are **not proof that the newest commit passed**. At the time of this documentation update, the latest README/context changes have **not** been compiled or exercised with a paid/live model in this environment.

Current limitations: interactive chat and the native work kernel are not yet a single lifecycle; independent commercial-grade long-horizon task success, full release validation, and reproducible provider-billed cache gains are still unmeasured. Avoid treating source existence or scripted-model fixtures as competitive benchmark wins.

For a file-by-file boundary and documentation discrepancy register, read [the 2026-11-18 source audit](docs/SUB3-SOURCE-AUDIT-2026-11-18.md). Historical detailed research remains in `docs/`; verify its date and scope before applying it to the current branch.

Apache-2.0 — see [LICENSE](LICENSE).
