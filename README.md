# DOMINION Rust

[한국어 안내](README.ko.md) · [Architecture](ARCHITECTURE.md) · [Harness contract](docs/HARNESS.md) · [Rust SDK](sdk/rust/README.md) · [TypeScript SDK](sdk/typescript/README.md) · [Current integration audit](docs/SUB3-INTEGRATION-2026-11-19.md)

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

## Frontend baseline and Dominion Cloud Gateway

The original React workspace was imported from main in November 2026. Sub3 has since independently improved all existing navigation destinations with accessible keyboard menu search, responsive model/connection tabs, mobile Chat history, guided first-run onboarding, an enhanced native Agent console, Task Team control and remote worker inspection. No new top-level navigation items were added and main was not modified. Full implementation and source-level scope: [Sub3 UX and browser audit](docs/SUB3-AGENT-UX-2026-11-21.md).

The full `main/cloud/` Cloudflare Worker implementation, D1 identity/usage/payment schema, operator-owned AI gateway, and typed service contracts have been imported under `cloud/` and `services/dominion-cloud/`. The **staging** Worker is named `dominion-cloud-sub3` and uses a separate, intentionally unprovisioned D1 database. The existing React account card registers it as an ordinary OpenAI-compatible provider once the staging gateway and OAuth origins are configured.

[Cloud staging integration and setup](docs/SUB3-CLOUD-GATEWAY-2026-11-19.md) · [Cloud gateway test results](https://github.com/hushmind3/dominion-rust/actions/runs/44503096540). This import does not deploy a worker or authorize real purchases. Local models and BYOK work without a cloud account.


## What is implemented

| Area | Source-backed scope |
| --- | --- |
| Autonomous jobs | Rust `dominion-agent-sdk::Kernel`, host adapters for model/tool execution, bounded planning, durable receipts, owner approvals, cancellation, verification, and guarded continuation. |
| Interactive chat and rooms | Existing streaming chat/room runtime, provider adapters, history/context selection, independent speaker state, and room collaboration. **This remains a distinct top-level execution path from native jobs.** |
| Providers and costs | Configurable local/remote model routes, native function calls where supported, usage records, selected provider cache hints, and Codex WebSocket incremental history where its connection/prefix survives. |
| Tools | Workspace/file/shell actions, selected browser/web and connected-service tools, MCP manifests, scoped Skills and progressively discovered tool schemas. Tools do not receive authority just because a model mentions their names. |
| Memory and recovery | SQLite-backed sessions, correction/learning records, bounded retrieval of past job receipts, cancellation and review of uncertain prior effects. |
| Client integrations | `sdk/rust` re-exports the kernel and provides an HTTP client; `sdk/typescript` is the single supported TypeScript HTTP/SSE client. The older `/api/harness/runs` endpoint remains a narrow compatibility adapter to the same native Rust jobs; no second client package is maintained. |

### Source + UX full-tree audit (2026-11-21)

A follow-up audit inventoried the full 438-file branch source tree and traced the real owner UI, native SDK, Chat/Room, Task Team, remote worker and Cloud paths. It repaired a browser DNS-rebinding Host check gap on **local owner API reads** and exposed an explicit **Refuse this action** button tied to the exact native tool approval ID; both have Rust and Chromium regressions. The latest executable CI run (commit 86274c0) passed Rust, frontend and actual Chromium browser jobs: [Actions](https://github.com/hushmind3/dominion-rust/actions/runs/44881868925). That Chromium suite uses API fixtures; it does not verify paid provider execution, actual Cloud deployment or a unified Chat/Agent loop. [Full audit and prioritized gaps](docs/SUB3-FULL-SOURCE-UX-AUDIT-2026-11-21.md).

### Crash-consistent paid model turns (2026-11-21)

The native Rust kernel now keeps its durable `model_in_flight` replay fence until reported/estimated usage and the validated decision (or an explicit parse failure) are saved together. If the provider response was received but the Journal commit fails, recovery requires review instead of silently repeating a potentially paid generation. No UI or API shape changed. Regression coverage is included in the sub3 SDK CI. [Read the source-level settlement contract](docs/SUB3-PAID-MODEL-SETTLEMENT-2026-11-21.md).

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
                         → src/conversation/chat/runtime/
                         → its existing streaming turn lifecycle
```

The **new** `/api/harness/runs` compatibility requests are translated into the same native job kernel with the narrower legacy workspace/shell authority. Old standalone `sdk_run`/FileJournal records remain **read-only archives**; their uncertain operations are not silently replayed. See [HARNESS.md](docs/HARNESS.md).

## APIs and SDKs

- **Jobs:** `GET/POST /api/jobs`, `GET /api/jobs/:id`, `POST /api/jobs/:id/cancel`, `POST /api/jobs/:id/directive`. Jobs can carry explicit acceptance checks and approved tool limits.
- **Native work panel:** `GET/POST /api/sdk/runs` and `/api/sdk/runs/:id/{approve,cancel,resume,reconcile,steer}`. Read-only evidence tools: `GET /api/sdk/runs/:id/replay`, `POST /api/sdk/runs/:id/compare` and `POST /api/sdk/runs/:id/adjudicate`.
- **Chat and data:** `POST /api/chat`, `/api/sessions`, `/api/providers`, `/api/rooms`, `/api/memory/overview`, `/api/mcp/catalog`, `/api/skill-catalog`.
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

**Local 2026 KV-cache backend support (sub3):** the DOMINION-managed `llama.cpp` runner is recognized from its persisted provider record, and requests explicitly set `cache_prompt: true` without also probing the unsupported `prompt_cache_key` field. A third-party llama.cpp endpoint can opt in using provider fields `"cacheBackend":"llama.cpp","kvCacheEnabled":true`; a vLLM endpoint uses `"cacheBackend":"vllm","kvCacheEnabled":true`. For vLLM, `cache_salt` isolates rooms but lets the nine speakers reuse the common charter's KV blocks before their personas diverge. Confirmed KV-enabled local routes use a bounded four-selection cached working-view epoch rather than a per-turn selector-sized epoch; unknown local endpoints keep the small safe default. **This is server-side prefill reuse, not a claim that stateless OpenAI-compatible requests omit the system prompt from the HTTP body**. Do not pin undocumented `id_slot` values for nine speakers: model/server slot capacity, eviction and concurrency must be provisioned separately. Exact provider cache-read and uncached-token telemetry remains authoritative; host estimates are not billed usage. [Group memory, wire and test audit](docs/SUB3-ROOM-CONTEXT-DELTA-2026-11-18.md).


**Persona-once Codex WebSocket (2026-11-18):** the host keeps the original system prompt in SQLite and the committed per-speaker context projection. For an active Codex Responses WebSocket chain, the initial request moves the fixed room charter and individual persona into the first trusted `developer` input item. Later requests send only new `input` items with `previous_response_id`; they omit the complete 4k-token charter/persona from the transport. A reconnect, expired chain, changed persona or changed owner policy reboots the full chain. The existing HTTP fallback and generic stateless Chat Completions/local model APIs still transmit a complete prompt unless their server provides real stateful context/KV reuse; neither a client cache key nor a WebSocket delta guarantees free input tokens. Even server-side chained context can remain billable, so check reported uncached input tokens. A mock-WebSocket regression asserts persona is sent once, never in the second incremental request.


**Nine-speaker group optimization (sub3, 2026-11-18):** derived shared room status is now content-addressed instead of bound to the changing owner message ID. When nine agents have the same completed-response/work state across consecutive owner turns, their per-speaker working views retain an identical prefix rather than forcing a room-snapshot rebase. A new owner message, verified owner decision/correction, prior self response, relevant peer evidence, and their original SQLite provenance remain available. Different speaker/work outcomes still invalidate the stale snapshot; context capacity guards remain intact. **The deeper source pass also appends newly verified owner facts without retransmitting existing facts, forces rebases on corrections or delayed earlier owner turns, separates per-turn selection from cache-epoch budgets, stabilizes relevant-tool schema order, and exposes measured provider-uncached input per call. Local models without proven cache reuse default to smaller epochs.** The added nine-speaker/100-owner-turn Rust regression is not a provider-token benchmark; authored tests have not been executed in this editing environment. **This reduces unnecessary cache-prefix churn, not proven provider-billed tokens.** [Design, regression cases and measurement plan](docs/SUB3-ROOM-CONTEXT-DELTA-2026-11-18.md).

- The native agent provides bounded, task-relevant Skill disclosure; ordinary chat now reuses the same scoped Skill selection. Full instructions/resources can be retrieved through approved `skill_read`/`skill_resource_read` calls.
- Long jobs retain original receipts in the durable journal and expose paginated read-only lookup. The model sees a bounded view without erasing the authoritative source.
- WebSocket incremental send and stable cache prefixes **do not themselves prove provider cache hits or reduced billing**. Host-side reused/new history estimates and provider-reported prompt/cache usage must be compared separately.
- An older nine-agent 100-turn run measured **18.15%** cached prompt tokens and **621 history-budget rebases** on **2026-11-09**. It is a historical baseline, **not a current-branch benchmark**. Details: [context audit](docs/SUB3-CACHE-CONTEXT-2026-11-18.md).

2026 references: [Claude prompt caching](https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything) and [deferred tool loading](https://claude.com/blog/reducing-cost-and-improving-performance-with-claude-platform). These describe upstream mechanisms, not DOMINION performance measurements.

## Product boundaries and data preservation

The standalone RSI generation/campaign/core-auto-promotion engine and built-in video/audio production pipeline have been retired on this branch. External media tools may be connected through user-approved MCP/plugins. **Owner-evidenced manual memory review, unapproved Skill drafts, agent-growth records and archived improvement history remain**. Retired learner-to-strategy/tool-policy/harness-proposal generation has also been physically removed; an old model response cannot automatically adopt execution policies. Historical SQLite records and already generated user files are not deleted.

Connections to GitHub/Google may use a configured OAuth application or an authorized official CLI flow. Provider consent/configuration is still required; credentials are not bundled. See [connected accounts](docs/CONNECTED-ACCOUNTS-2026-11-17.md).

## Schema-aware progressive tool discovery (sub3, 2026-11-20)

- Native Rust SDK jobs and interactive chat now share a bounded **two-view lexical ranker**: authorized tool names/descriptions and JSON Schema parameter-field vocabulary. This helps discover generic MCP endpoints whose action is described by fields such as `repository_name` and `issue_number`; the field view is capped at four nested levels and 96 terms.
- A native `dominion_tool_search` receipt is now bound to the exact authorized JSON Schema, read-only/effect classification and displayed description before `dominion_tool_call` may invoke it. Catalog contract drift requires a new discovery; existing host authorization, argument validation, approvals and durable execution remain in charge.
- Regression cases cover opaque MCP names, native/chat ranking parity, schema/effect drift and authorization boundaries. The change adds no menu, new indexing service, paid discovery call or claim of measured real-model accuracy gains. Automated branch CI and real task evaluation are tracked separately.

## Sub3 input-token optimization (2026-11-20)

- Checkpoint summarization budgets its input from the configured model context window (output reserved) and caps the transcript at 48,000 characters instead of forwarding up to 110,000.
- Older successful tool observations are represented by bounded head/tail excerpts; exact repeated results are marked once, while recent observations, failures, and owner messages retain higher priority. The original SQLite conversation is never rewritten by checkpoint projection.
- Chat context editing for **local models without confirmed server KV caching** now triggers earlier (40% of estimated input capacity) so stale read results are not repeatedly paid for as fresh prefill. Explicit cache-capable local backends and remote routes retain the existing cache-friendly 75% threshold. Writes, errors, latest three results, and tool-call identities remain intact.
- The retired-feature regression scanner also examines SQL, HTML, CSS, Python, XML, SVG, text/config and dotenv files in addition to Rust/TypeScript/Markdown. This guard prevents dormant feature fragments from reappearing outside application source.
- New regressions test replay fidelity, error tails, repeated tool outputs, Unicode safety and cache-sensitive thresholds. All savings are **projected prompt characters**, not claimed provider-billed token reductions. Verify with real model usage/cache telemetry before asserting cost savings.

## Optional integrations outside the engine (2026-11-20)

MCP sessions, messaging transports, browser CDP control, local Skills and third-party account connectors (including the GitHub/Google tool catalog) are now owned by `src/integrations/` rather than `src/engine/`. The main process wires adapter startup through `bootstrap.rs`, while the Rust SDK kernel keeps planning, approval and durable effect verification. Existing `/api/mcp/*`, channel/account API paths and React navigation remain unchanged. Legacy `engine::mcp`/channel/account aliases were retired; source callers import the new modules directly. The `AppState::integrations` field owns pooled MCP sessions. The functionality is still compiled into the host executable; this refactor does **not** claim a dynamically loaded plugin ABI or fully independent Cargo crates. [Boundary and follow-up architecture](docs/INTEGRATIONS-BOUNDARY-2026-11-20.md).

## Minimal language-to-action core (2026-11-20)

The execution infrastructure is now independently compiled as `crates/dominion-agent-runtime/`, with budgets, shared action/receipt contracts, dependency-aware scheduling and fail-closed argument-schema admission. Model-facing intelligence is owned by `crates/dominion-agent-algorithms/`: planning instructions, intent signals, tool ranking/discovery, context projection and failure guidance. The public `crates/dominion-agent-sdk/` is now a **13-line compatibility facade**: the actual `ReActLoop` implementation, bounded model → tool → result cycle, and offline replay belong to `crates/dominion-agent-algorithms/agent_loop.rs` and `replay.rs`. The runtime owns the `AgentLoop` and `AgentPorts` interfaces, action/receipt contracts, tool dispatch barriers, permission-limited admission, and durable checkpoints. The host calls the `AgentLoop` trait. Existing `Kernel::run()` / `resume()` and algorithm injection methods remain accessible through the SDK re-exports. The host now allows `algorithmId: "standard-v1" | "focused-v1"` per prompt job, pins the profile revision in the job record, and saves the algorithm ID in the SDK checkpoint. Resume requires the exact same policy; the TypeScript SDK exposes the typed choice with no additional UI. Neither runtime nor algorithms links to SQLite, `AppState`, credentials, channels or the web server.

**SQLite and its migrations live in `src/storage/`** (alongside the encrypted vault); product-state ownership and the DB-backed job/approval/effect implementation live under `src/host/`. The old `src/engine/` directory has been removed. `src/main.rs` supplies a temporary `crate::engine` alias to `host::execution` for existing internal Rust imports, preserving network APIs and stored records. Chat lives under `src/conversation/`, learning/group/model-platform features under `src/features/`, and MCP/CLI/web/local tool transports under `src/integrations/`.

This is **module ownership, independent runtime/intelligence crates, and portable-kernel separation**, not a separately deployable plugin system or complete host-independent app binary. Existing app features still access the host's SQLite through its own `AppState`; the SDK kernel has no such dependency. [Detailed architecture and contract checks](docs/CORE-BOUNDARY-2026-11-20.md) · [Algorithm injection and runtime dependency boundaries](docs/ALGORITHM-RUNTIME-2026-11-20.md).

## Pre-dispatch cancellation and owner corrections (sub3, 2026-11-20)

The version-pinned Rust agent loop rechecks **cancellation and owner directive changes immediately after durable write-ahead intent persistence** and before calling a paid model, an authorized tool, or an owner-required verification command. If a correction arrives at that fence before execution begins, the pending intent is cleared in the journal; cancelled runs stop, while corrected tasks replan from the updated owner instruction. Tool calls that have already begun still require their normal durable receipt or explicit review if their outcome is uncertain. Regression tests in `crates/dominion-agent-sdk/tests/kernel_replanning.rs` cover model/tool/acceptance cancellation and tool-dispatch correction. This does not claim instant cancellation of already-running provider or tool calls.

## Room delivery and evidence continuity (sub3, 2026-11-21)

The Room read cursor now bootstraps at the **current owner message** rather than at the last peer message visible in the first compilation. This prevents a newly participating agent from acknowledging peer messages it did not receive as required unread input. Subsequent turns drain only the remaining committed messages in contiguous, bounded windows; the cursor is still persisted atomically with the answer, and historical content remains available to indexed recall. The Room workboard also retains the latest **receipt-backed** contribution per agent when a newer reply has no scoped operations, and excludes an operation explicitly attributed to a different agent. Added regressions cover joining-agent delivery, empty newer replies, and cross-agent receipt contamination. No React navigation or extra runtime loop is introduced. Focused CI tests: `features::collaboration::room_cursor::tests` and `features::collaboration::workboard::tests`.

## Development verification

Routine changes: run *focused Rust development-profile tests*, not a blanket release build.

```sh
cargo test -p dominion-agent-runtime --lib
cargo test -p dominion-agent-algorithms --lib
cargo test -p dominion-agent-sdk --test algorithm_policy_injection
cargo test -p dominion-agent-sdk --lib
cargo test -p dominion-sdk --lib
cargo test --bin dominion-rust features::collaboration::rooms::tests
cargo test --bin dominion-rust api::providers::tests
cargo test --bin dominion-rust api::cli_oauth::tests
cargo test --bin dominion-rust conversation::chat::context::tests
cargo test --bin dominion-rust integrations::skills::discovery::tests
cargo test --bin dominion-rust api::sdk::tests
```

Each sub3 product CI push runs frontend TypeScript checks, HTTP contracts, Rust product tests and a real Chromium responsive/UI interaction suite against the Vite dev server. The test exercises 1440/768/390/320px layouts, all nine destinations, modal focus, keyboard search, Agent and Team actions, and connection onboarding. Browser screenshots are saved as an Actions artifact. No production frontend release build is performed on push; release packaging remains manual.

```sh
npm run typecheck --prefix web
node --experimental-strip-types --test web/tests/*.test.mjs
npm run build --prefix web
node scripts/check-lean-product.mjs
```

**Latest verified code before the documentation cleanup:** `4e9f5bdf` passed [Sub3 product Rust + frontend + TypeScript SDK CI](https://github.com/hushmind3/dominion-rust/actions/runs/44665179631) and [Sub3 Rust SDK CI](https://github.com/hushmind3/dominion-rust/actions/runs/44665179763) on 2026-11-20. The standalone media/RSI paths, upload-only OAuth grant, obsolete policy promotion and duplicate client SDK are retired; prior SQLite jobs/evidence and manually reviewed learning records remain. Check the Actions run attached to later commits separately. `.github/workflows/ci.yml` is manual release-stage verification and was not run. These are mock-provider and compiler checks, not a measured 27B success rate or provider-billed token reduction. See [current runtime documentation](docs/INDEX.md).

Current limitations: interactive chat and the native work kernel are not yet a single lifecycle; independent commercial-grade long-horizon task success, full release validation, and reproducible provider-billed cache gains are still unmeasured. Avoid treating source existence or scripted-model fixtures as competitive benchmark wins.

For a file-by-file boundary and documentation discrepancy register, read [the 2026-11-18 source audit](docs/SUB3-SOURCE-AUDIT-2026-11-18.md). Historical detailed research remains in `docs/`; verify its date and scope before applying it to the current branch.

Apache-2.0 — see [LICENSE](LICENSE).

### Independent extensions (sub3)

Service connections → Plugins searches public package registries and manages local Docker images separately from MCP and skills. Start Docker Desktop/Engine, select a versioned OCI image or digest, confirm the permissions, install, then explicitly start. Running state and logs come from Docker. Containers are scoped to the current data directory and port, have bounded CPU/memory/processes/logs, read-only root, a named `/data` volume, and no network unless enabled. Network-enabled images can access the network; only explicitly selected service ports are published to loopback. Removal requires a stopped container and preserves the image and named volume. The retained-data card allows reinstalling with the same volume and an explicitly selected version. It is not an atomic upgrade or data migration/rollback guarantee. Do not treat arbitrary registry results as compatible plugins. The host-pull SDK bridge supports compatible running apps with one local published port. Apps expose GET /dominion/v1/request (204 or {requestId: UUID, instruction}) and POST /dominion/v1/result (acknowledge the same requestId and answerDigest). The owner pulls and approves a fixed conversation-only or workspace read/write scope, the canonical SDK job executes once, and the owner reviews and delivers the answer. No app receives the owner API token; shell, MCP and arbitrary tool grants are excluded. An explicit, memory-only 30-minute/20-turn conversation grant enables automatic answers for the exact running container; it disables tools, shared-memory/skill injection and fallback, uses bounded model calls, and pauses on uncertain delivery. Grants vanish when DOMINION restarts. Workspace actions remain manually reviewed. Apps must deduplicate result delivery by requestId and answerDigest. Stopped plugins support explicit version changes: pull the target first, persist the previous immutable image and settings, replace the stopped container, and allow owner-triggered rollback. A failed replacement leaves a durable recovery state; rollback recreates the previous local image without executing it. /data is retained, not rolled back. Automatic background upgrades, data snapshots, volume purge, native non-container runtimes, native mobile apps and SIP/PSTN calls remain unimplemented. Python/PyPI, Node/npm and Rust/crates.io executable packages can be built in independent containers using exact versions and explicit executable names. Dockerfile generation uses exec-form JSON and no owner files/keys. PyPI requires wheels; npm disables install scripts; package runtime compatibility is not guaranteed. Package data can be retained and reinstalled with the same plugin ID; atomic package-recipe upgrades remain separate from registry image rollback. A bundled, separate DOMINION Voice container is available from the plugin screen. Browser speech recognition/TTS and a Pipecat 1.17.0 LiveKit STT→DOMINION→TTS adapter use the owner-approved task protocol. The latter requires private LiveKit room credentials, Deepgram and Cartesia settings. Browser availability, provider compatibility, phone client and actual Docker audio execution require end-to-end validation. The voice bridge token is generated per plugin and stored in the existing encrypted vault; it is not the owner token.

Voice mobile/SIP extension: `plugins/dominion-voice/mobile` is an independent Expo Android/iOS client. The local voice control page mints room-scoped microphone-only tokens for a random room; paste the mobile pairing into the native client. For outbound PSTN, configure a LiveKit SIP outbound trunk and approve one E.164 destination. For inbound PSTN, configure a dedicated inbound trunk/dispatch rule with an allowlisted caller and a fixed `dominion-inbound-...` room, then explicitly start waiting in the voice control page. No automatic trunk provisioning, carrier credentials, or live call verification is included. Calls are bounded to ten minutes and outbound uses a unique room, with no automatic redial. Server keys are not sent to the phone. Actual native build, carrier setup, Pipecat audio and end-to-end call testing are outstanding. See https://docs.livekit.io/reference/telephony/sip-api/ and https://docs.livekit.io/transport/sdk-platforms/expo/ .
