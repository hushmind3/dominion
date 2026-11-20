# DOMINION

<p align="center"><img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" /></p>
<p align="center"><strong>Your models. Your workspace. A native Rust agent runtime.</strong></p>
<p align="center"><a href="README.ko.md">한국어</a> · <strong>English</strong> · <a href="LICENSE">MIT</a> · <a href="https://github.com/hushmind3/dominion-rust/releases">Desktop downloads</a></p>

DOMINION is a **local-first AI agent workspace** with a native Rust execution
engine, React/Vite frontend and Tauri 2 desktop apps for **macOS and Windows**.
It connects cloud or local inference providers while keeping approvals,
execution receipts and user-controlled workspace boundaries in the application.

<p align="center">
  <img src="docs/01-multi-agent-room.webp" alt="Multi-agent room" width="48%" />
  <img src="docs/02-chat-workspace.webp" alt="Workspace chat" width="48%" />
</p>

## Features

- **Native agent execution:** structured tool calls, bounded budgets, approval
  decisions, durable effect records, cancellation, restart and repair.
- **Model choice:** bring OpenAI / Anthropic API credentials, or connect supported
  local inference endpoints such as Ollama, llama.cpp and LM Studio.
- **Nine-plus participant rooms:** per-agent personas, stable session history,
  user-confirmed state, bounded context selection and per-call usage evidence.
- **Tools and extensions:** native file/shell/browser operations plus MCP
  integrations. Mutations use the same approval and audit pipeline.
- **Memory:** SQLite-backed sessions, retrieval, evidence, verified changes and
  explicit owner instructions.
- **SDK:** a Rust kernel and a lightweight TypeScript HTTP client with
  progress, approval, cancellation and verification semantics.
- **Desktop:** Tauri application with the Rust engine sidecar; Node.js is
  required only during development and packaging.

## Download and install

The latest installers are on [GitHub Releases](https://github.com/hushmind3/dominion-rust/releases).
The release matrix produces **Apple Silicon DMG, Intel Mac DMG, Windows NSIS
EXE and MSI** installers. Builds are currently unsigned; follow your OS's
normal security confirmation when installing a build you trust.

Release artifacts and their checksums are produced from the **same tagged
source commit**. A successful frontend typecheck is not evidence of a
successful desktop build: Windows and both macOS targets must pass the
native build job before publication.

## Develop

Requirements: **Rust stable**, **Node.js 24.12+**, npm, and your OS's
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/).
Windows needs the MSVC build tools/WebView2, macOS needs Xcode tools.

```bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
npm run typecheck --prefix web
npm run build --prefix web
cargo test --locked --workspace
```

For a local development server, run
`npm run dev:server --prefix web` and `npm run dev --prefix web` in
separate terminals. For native packaging on a supported operating system:

```bash
node scripts/check-release-contract.mjs
node scripts/build-desktop.mjs
```

The release workflow runs full macOS/Windows build and test jobs on version
changes, explicit dispatch or a version tag.

See the [core / algorithm separation contract](docs/CORE-ALGORITHM-BOUNDARY-2026.md) for
the provider-independent strategy port, optional decisions and ownership rules.

## System map

| Layer | Implementation |
| --- | --- |
| Core execution and durable effects | [dominion-core](crates/dominion-core/src/engine.rs), [effect journal](crates/dominion-core/src/journal.rs) |
| Algorithm/host contract | [TurnStrategyPort](crates/dominion-core/src/strategy.rs), [product composition](src/addons/runtime_hooks.rs), [replaceable decisions](src/addons/strategies/turn_policy.rs) |
| Agent orchestration and tool admission | [chat runtime](src/engine/chat/runtime.rs), [native gateway](src/engine/chat/runtime/tool_dispatch.rs) |
| Private-chat context compilation | [Context Engine](src/engine/chat/context_engine.rs), [continuity](src/engine/chat/continuity.rs) |
| Group state, memory and provider continuity | [rooms](src/addons/integrations/rooms.rs), [room context](src/addons/integrations/chat_room_context.rs), [continuity](src/engine/chat/continuity.rs) |
| Local API, durable records | [HTTP router](src/api/mod.rs), [SQLite store](src/db.rs) |
| React interface | [App](web/src/App.tsx), [workspace shell](web/src/components/WorkspaceShell.tsx) |
| Native desktop | [Tauri configuration](src-tauri/tauri.conf.json), [build script](scripts/build-desktop.mjs) |
| TypeScript SDK | [SDK client](packages/sdk-typescript/src/index.ts) |

### Execution, not just conversation

One owner request enters the Rust API, is compiled into a scoped task,
passes through provider selection and a bounded model/tool loop, then
persists results, approvals, tool receipts and verification status in SQLite.
A model-generated success message **does not** replace independent completion
evidence. Mutating tools and deferred/discovered MCP tools pass the same
permission, schema, cancellation and effect journal gates.

### Context and token accounting

Ordinary **1:1 chat** uses a separate [private Context Engine](src/engine/chat/context_engine.rs).
The [SQLite store](src/db.rs) retains the original conversation and evidence;
only model-visible history, contextual retrieval and tool contracts are selected.
Full owner instructions and persona are preserved.

- **Stable stateful prefix:** An unchanged contextual snapshot is anchored once
  before the transcript; edits or revocations trigger an explicit rebase.
  Codex WebSocket `previous_response_id` is reused only for a compatible chain.
- **Small social turns:** An unmistakable greeting does not replay a previous
  coding task, enumerate MCP servers, or inject unrelated skill/growth data.
  Date metadata is emitted only for a date-relevant question.
- **Capacity-aware input:** History is bounded by model context, output reserve,
  static instructions, selected retrieval and tool-schema costs, in addition
  to the private 32KB history / 12,000-token working-view bounds. The newest
  owner input is preserved even if large.
- **Provider accounting:** Every completed private or group call records
  prompt/output tokens, optional provider-reported cache reads/writes, and
  whether the values were estimated. Session totals include auxiliary calls;
  unknown cache telemetry is **unknown**, never an invented zero. Configured
  cache input rates can be used for cost settlement.

Real-process tests through the Rust HTTP/SSE runtime and a mock inference
provider measured cumulative **API request JSON bytes** against a conservative
unbounded history replay baseline: **4.9% less over 10 turns**, **83.2% less
over 100 turns**. These are wire-traffic results, **not** an API-provider
billed-token or price-reduction claim. See the
[reproducible benchmark](benchmark/PRIVATE-CHAT-WIRE-2026-11-20.md).

### Optional backend extensions, independent of the native kernel

The native Rust kernel owns session history, execution/checkpoint journals,
model calls, approval and verification. MCP servers, long-term memory,
skills and messaging channels are integrated only through the
[backend extension host](src/addons/integrations/extensions/mod.rs), with fresh tool-call
admission after discovery. Disabling memory never deletes session transcripts;
disabling channels does not disable normal chat. No frontend navigation changes.

See the [extension boundary and API contract](docs/EXTENSION-HOST-2026-11-20.md)
for scoped policy, `/api/extensions/runtime`, session controls and tests.

### Native execution contracts and lean model rounds

The native Codex WebSocket chain now invalidates `previous_response_id`
when tools, reasoning settings or other provider request controls change,
preventing a stale policy/persona from leaking across turns. After a real
file/tool action has independently satisfied its acceptance checks, the
agent can persist a verified answer without a redundant final model call.
Program nodes labeled `parallel` run concurrently **only for independent
read-only calls**; shell commands, mutations and dependent branches are
serialized, and their checkpoint records the effective mode from the start.
Focused GitHub CI exercises these paths on the real API and Rust engine.

A group room preserves historical messages on disk but sends a bounded,
source-aware working view to each model. Cache routing and optional provider
cache hints improve prefix reuse where the provider supports them. They do not
turn independent model requests into one persistent reasoning process.
Cache-hit tokens are reported only when the provider actually supplies them;
estimated history reuse is not counted as a billed cache hit.

## Product boundaries

DOMINION runs one native Rust agent/SDK execution kernel. Optional media
and research capabilities are integrated through user-connected MCP tools,
not through parallel product-specific execution engines. User records are
not deleted as a side effect of upgrading the application.

See [getting started](docs/GETTING-STARTED.md),
[desktop setup](docs/DESKTOP.md), [engine architecture](ENGINE.md),
[harness semantics](docs/HARNESS.md) and
[TypeScript SDK](packages/sdk-typescript/README.md).

## Verification and release

- [Development checks](.github/workflows/ci.yml): focused Rust and frontend
  checks plus native Windows validation.
- [Native release](.github/workflows/release.yml): native test/build matrix for
  macOS ARM64, macOS x64 and Windows x64. Artifacts are published only when
  required jobs succeed.
- [Runtime structure check](scripts/check-runtime-architecture.mjs) and
  [version/sidecar check](scripts/check-release-contract.mjs).

**License:** MIT. The [main branch](https://github.com/hushmind3/dominion-rust/tree/main)
is the canonical development line.
