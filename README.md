# DOMINION — Native Rust Autonomous Workspace

The first-party **Rust SDK** owns scheduling, session evidence, models, tools, verification, cancellation and completion. Its HTTP API and TypeScript transport client call the same engine. The **main branch's React/Vite frontend** is preserved and used as the champion workspace.

## Run

Install stable Rust, Node.js 22.12+ (24 LTS recommended), and a compatible local or remote LLM provider. From the repository root:

~~~sh
npm ci --prefix web
npm run build --prefix web
export DOMINION_WORKSPACE=.
export DOMINION_MODEL_URL=http://127.0.0.1:11434/v1/chat/completions
export DOMINION_MODEL=qwen3
cargo run
~~~

Open http://127.0.0.1:4317. The server binds to loopback and serves web/dist with an SPA fallback. DOMINION_WEB_DIR overrides the production build location. For UI development run cargo run, then npm run dev --prefix web to open Vite with its /api proxy.

The configured model must already exist. A remote provider can be configured with DOMINION_MODEL_URL, DOMINION_MODEL and DOMINION_API_KEY. A smaller backup model can use DOMINION_ECONOMY_MODEL_URL, DOMINION_ECONOMY_MODEL and its associated key environment settings.

## Real product surfaces

| Native Rust capability | HTTP / GUI |
| --- | --- |
| Durable tasks, event replay, interventions and explicit continuation | /api/tasks, /api/tasks/:id/events, /api/tasks/:id/resume |
| Native persona presets that affect actual model prompts | /api/agents |
| Sequential durable workflows using the same Rust task engine | /api/workflows, /api/workflows/:id/runs, /api/workflow-runs/:id |
| Multi-participant model discussions with distinct participant economy settings | /api/groups |
| Video storyboard, manual scenes, image-driven motion, local MP4 and optional paid Veo | /api/videos and per-scene edit/image/render paths |
| Isolated source improvement trials, independent measurements, explicit adoption and descendants | /api/improvements |
| Scoped memory and explicitly activated skills | /api/memory, /api/skills |
| Read-only provider information and native/MCP tool discovery | /api/providers, /api/tools |
| Main React navigation, user profile and event inspector | web/src/App.tsx, web/src/pages/NativeWorkspace.tsx, NativeAgents.tsx, NativeWorkflows.tsx, NativeSystem.tsx |
| Optional transport SDK, not a parallel harness | sdk/typescript |

Task admission uses requestId idempotency. Native workers persist every tool observation to SQLite; workspace writes require explicit permission, and confirmed writes receive a readback receipt. A bounded model-context projection retains complete tool-call groups and the newest owner instruction. The original transcript and tool receipts are never shortened in storage; task_receipt can retrieve a persisted observation without repeating an external effect. An interrupted task can continue only through a new, linked task with a new explicit owner instruction and permissions.

Model routing choices are auto, economy or quality; cost accounting records cached/uncached input and output at operator-configured KRW rates, with explicit uncertainty when a provider omits usage. Configure DOMINION_INPUT_KRW_PER_MILLION, DOMINION_CACHED_INPUT_KRW_PER_MILLION and DOMINION_OUTPUT_KRW_PER_MILLION, plus ECONOMY-prefixed equivalents if using a backup. Task options tokenBudget and costBudgetKrw provide bounded execution; a paid remote API needs configured rates before a KRW cap is accepted. These are **rate-based estimates**, not settled provider invoices. Local compute, retries and other billed product services need separate accounting.

## Verified long-context evidence subsystem (feature-level focus)

The first completed frontier-relevant native component is the **bounded evidence-preserving context engine**, in src/engine/context.rs with the real model execution path in src/engine.rs. It validates exact assistant/multi-tool-result groups, strictly limits outbound encoded JSON bytes, records cryptographic source/projection hashes, and can retrieve the original persisted UTF-8 tool result page by page without repeating side effects. Explicit continuation keeps task lineage in one SQLite transaction and enforces ancestry-scoped receipt permissions. Canonical provider context overflow may retry only after measurable projection shrinkage. The imported main UI displays real source/target/budget bytes and receipt counts in its developer inspector.

Set DOMINION_CONTEXT_MAX_BYTES to configure the total encoded model-request context envelope (default 96000 bytes; includes tool schemas and reserved protocol overhead). This is not a tokenizer-derived context window or measured model-performance advantage.

The implementation, exact tool interfaces, failure modes and test evidence are documented in **[Bounded evidence context](docs/CONTEXT_EVIDENCE.md)**. The GitHub native test workflow has passed 59 Rust tests with a real SQLite and filesystem, mock-provider end-to-end evidence retrieval, and the TypeScript SDK contract. Any further feature is separate from this tested subsystem.

## Low-cost video

Local video rendering requires FFmpeg. Add image assets to individual scenes through the UI; the engine creates animated zoom/pan video segments entirely on CPU, combines clips into a downloadable MP4, and invalidates only edited scenes. Text-only scenes still render. Optional speech uses local Piper neural speech (DOMINION_PIPER_MODEL and optionally DOMINION_PIPER_BIN); when unavailable, eSpeak is the fallback, and silent video is supported.

Optional Veo generation needs DOMINION_VEO_API_KEY, a positive DOMINION_VEO_KRW_PER_SECOND estimated rate, and an explicit per-video budget_krw. It will not issue the next billable scene call if the projected cost exceeds that ceiling. A local FFmpeg slideshow is not claimed to be generative cinematic video.

## Historical main-branch data import

An operator may explicitly import an existing main-branch SQLite database by starting the engine with DOMINION_IMPORT_LEGACY_DB=/path/to/original.sqlite. The importer opens the original read-only, checks that source and destination differ, uses a transaction and imports each source location only once. Original historical JSON records are retained in a non-executing archive; valid user-authored memories, agent personas, disabled-by-default skills and chat messages are projected into native tables with stable IDs. **Stored credentials and old executable jobs are not activated or run.** Keep a secure backup of the original database and separately reconnect provider secrets.

## Tests and current boundaries

~~~sh
node --test scripts/source-contract.test.mjs
npm ci --prefix web
npm run typecheck --prefix web
npm run build --prefix web
npm test --prefix sdk/typescript
cargo test
~~~

Tests are present in source for native execution, cost, persistence, context, workflow, video and migration. **The current GitHub connector execution environment did not run Rust compilation or a full browser smoke test.** Those must pass before performance and cost claims. Historical advanced pages remain in web/src as migration references rather than active broken screens; full desktop packaging, model-management UI, complex workflow branching, multi-user service hardening, frontier-grade autonomous success-rate measurement and generative cinematic quality remain open product work.

See docs/ARCHITECTURE.md and sdk/typescript/README.md. Benchmark only after all source paths and tests are verified.
