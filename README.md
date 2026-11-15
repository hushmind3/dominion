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
| Independently scored source trials, durable A→B→A evolution, verified Git lineage | /api/improvements, /api/evolutions |
| Scoped memory and explicitly activated skills | /api/memory, /api/skills |
| Read-only provider information and native/MCP tool discovery | /api/providers, /api/tools |
| Main React navigation, user profile and event inspector | web/src/App.tsx, web/src/pages/NativeWorkspace.tsx, NativeAgents.tsx, NativeWorkflows.tsx, NativeSystem.tsx |
| Optional transport SDK, not a parallel harness | sdk/typescript |

Task admission uses requestId idempotency. Native workers persist every tool observation to SQLite; workspace writes require explicit permission, and confirmed writes receive a readback receipt. A bounded model-context projection retains complete tool-call groups and the newest owner instruction. The original transcript and tool receipts are never shortened in storage; task_receipt can retrieve a persisted observation without repeating an external effect. An interrupted task can continue only through a new, linked task with a new explicit owner instruction and permissions. A full-text SQLite index, exact-quote owner checkpoints, explicit later corrections and event-derived recovery state now preserve context beyond the 256-message loaded session tail. Models can search old records without knowing a tool call ID.

Model routing choices are auto, economy or quality; cost accounting records cached/uncached input and output at operator-configured KRW rates, with explicit uncertainty when a provider omits usage. Configure DOMINION_INPUT_KRW_PER_MILLION, DOMINION_CACHED_INPUT_KRW_PER_MILLION and DOMINION_OUTPUT_KRW_PER_MILLION, plus ECONOMY-prefixed equivalents if using a backup. Task options tokenBudget and costBudgetKrw provide bounded execution; a paid remote API needs configured rates before a KRW cap is accepted. These are **rate-based estimates**, not settled provider invoices. Local compute, retries and other billed product services need separate accounting.

## Verified long-context evidence subsystem (feature-level focus)

The first focused native capability is the **evidence-grounded long-context engine**, combining src/engine/context.rs, src/engine/semantic.rs, src/context_memory.rs, src/store.rs and the real model loop in src/engine.rs. It validates exact assistant/multi-tool-result groups, strictly limits outbound encoded JSON bytes, records cryptographic source/projection hashes, and can retrieve the original persisted UTF-8 tool result page by page without repeating side effects. Explicit continuation keeps task lineage in one SQLite transaction and enforces ancestry-scoped receipt permissions. Canonical provider context overflow may retry only after measurable projection shrinkage. The imported main UI displays real source/target/budget bytes and receipt counts in its developer inspector.

Set DOMINION_CONTEXT_MAX_BYTES to configure the total encoded model-request context envelope (default 96000 bytes; includes tool schemas and reserved protocol overhead). This is not a tokenizer-derived context window or measured model-performance advantage.

The implementation, exact tool interfaces, failure modes and test evidence are documented in **[Bounded evidence context](docs/CONTEXT_EVIDENCE.md)**. The GitHub native test workflow has passed 59 Rust tests with a real SQLite and filesystem, mock-provider end-to-end evidence retrieval, and the TypeScript SDK contract. Any further feature is separate from this tested subsystem.

## Model-neutral MCP tool selection (live native path)

The Rust agent no longer sends every configured MCP tool schema in every model
turn. It retains the permission-scoped tool catalog server-side and uses
`src/engine/schema_router.rs` for bounded, intent-weighted tool selection.
When an external capability is not visible, the model can call
`tool_discover` to browse short eligible descriptions, promote fitting full
schemas, and use them on the next step. Tool aliases absent from that step's
actual provider request are rejected **before any external effect can start**.

Routing is wired from the native `/api/tasks` execution loop to the existing
TypeScript event client and the React task UI. The GUI shows the actual
JSON-schema bytes sent versus catalog bytes; provider-reported tokens and
configured KRW cost estimates remain separate measurements. The feature
includes local HTTP mock model + a **46-tool MCP server** end-to-end tests
for discovery, promotion, real invocation and rejected fabricated tool calls.
See **[Native lazy MCP tool routing](docs/TOOL_ROUTING.md)** for exact limits,
source trace, evidence and unresolved retrieval-quality work.

## Bounded A→B→A code evolution

A separately evaluated and **adopted** improvement can seed 1–4 automatic generations through `POST /api/improvements/:id/evolution`, the Rust/TypeScript SDK, or the main React RSI lab. Provide `max_generations`, `cost_budget_krw` and optional ordered `target_files` (for example `["src/a.rs","src/b.rs","src/a.rs"]`). Optionally set `candidates_per_generation` to 1–4 and supply a **separate owner-authored numeric** `validation_command`. Every sibling candidate starts from the same adopted Git base and fixed objective/test/scoring contract. The engine retains individual Git-pinned candidates, scores all proposals, runs validation in new detached worktrees against the exact baseline/candidate commits, and chooses **one** independently eligible highest-scoring candidate; losers are not adoptable. Validation commands and outcomes are never shown to the model proposer. Every candidate's result and metered cost (including rejected proposals) is persisted, and the chosen verified gain becomes the next generation's baseline. For sibling candidates sharing the exact Git base SHA, independently measured baseline evidence is reused; each candidate still runs fresh tests and, when configured, fresh validation. This avoids redundant baseline subprocesses without treating stale source revisions as verified.

The lineage record persists candidate IDs, adoption count, budget use and stop conditions. `POST /api/evolutions/:id/stop` (also exposed in SDK and UI) durably requests that the current chain stop before any further generation or candidate adoption. An in-flight call can finish; interrupted work remains `needs_review` and does not replay uncertain paid or destructive effects. The same native workspace lock prevents interference with ordinary agent coding tasks, while the operator controls when an entire chain begins. Cost ceilings use metered provider estimates and can be exceeded inside one remote call; evaluator shells remain operator-approved and are not a hardened sandbox.

**Meta-RSI strategy memory:** `src/improvement/evolution/strategy.rs` now maintains a deterministic, weight-free method-selection policy reconstructed from durable evaluator records. Strategies are `minimal`, `alternative`, `simplify` and `algorithm`; it explores untried methods and then favors historically verified gains, with measured costs as a minor tie influence. A method's reward is normalized against the original score and weighted more strongly when an independent validation pass exists. SHA-256 fingerprints tie learned evidence to the exact repository, file, owner objective and evaluator contracts; related-file experience is transferred at lower weight. All siblings in one generation receive their method assignments **before** the first proposal. The model cannot alter these evaluator-derived records, and its inherited evidence text never includes the holdout command or outputs. See the product UI's per-candidate method and prior evidence fields, the typed TS SDK records and real-Git three-run integration test.

See **[Native RSI evolution architecture and test evidence](docs/RSI_EVOLUTION.md)** for exact API inputs, real Git integration tests, failure behavior and limitations.

Source-integrity acceptance: the native Rust check runner fingerprints project source/configuration bytes before and after each allowed test command. Even process exit code 0 is a failed proof when code changes during checking. The tool receipt stores both SHA-256 values and sourceUnchanged. Old passing results are invalidated. A real Node regression changes its own source while returning exit 0 and must end in needs_review. Build/dependency outputs are excluded; this integrity check is not an OS sandbox. Details: src/engine/source_integrity.rs and docs/CODING_ACCEPTANCE.md.

## Native self-repairing code acceptance (second focused capability)

The actual \`src/engine.rs\` model/tool loop can now **read source → edit one or several files → execute owner-approved tests → read real compiler/test errors → repair → re-run tests → complete**. This is native Rust behavior, not a TypeScript agent wrapper or a prompt-only promise.

- \`src/engine/verification.rs\` owns check selection, an exact executable+argv allowlist, source revision invalidation and the deterministic completion gate. \`acceptanceChecks\` accepts up to 4 precise owner-specified checks when \`allowShell=true\`. With no explicit checks, the engine conservatively detects \`cargo test --quiet\` for Rust or \`npm test --silent\` for a Node package with a test script. No shell interpolation is performed. Failed tests retain stderr, stdout, actual exit code, Rust tool events and the next repair instruction.
- **Passing results from a prior source revision do not count**: every attempted write/edit, uncertain external MCP side effect or source-format action invalidates earlier check proofs. An unchanged failing revision is not re-tested endlessly. With \`requireEvidence=true\`, a coding job that has shell permission but no runnable check enters \`needs_review\` instead of claiming verified success.
- \`src/engine/files.rs\` supports bounded numbered reads and staged, rollback-aware single-file writes, exact replacements and multi-file exact replacements. It pre-validates 1–8 independent changes, refuses stale file SHA-256 and ambiguous spans, preserves original permissions, confirms readback hashes and attempts rollback on a failed adoption. A crash in the middle is reported as an uncertain effect; backups must be inspected before retry.
- A shared native workspace lease prevents overlapping writer/shell/MCP jobs from interleaving file changes with another agent's acceptance check. Read-only tasks continue independently. For shell/test commands, the owner can opt into strict OS isolation (`shellIsolation: "isolated"`) via Bubblewrap on supported Linux hosts or a preinstalled Docker/Podman image. This does not isolate direct file tools, MCP, RSI evaluator processes or separate worktrees.
- The final task result carries its independently recorded \`acceptance\` evidence (passing check commands and source revision), alongside file-write readback evidence. An assistant's prose alone never sets these fields.

Example Rust/HTTP task input (same JSON contract is supported in the TypeScript SDK and the main React task form):

~~~json
{
  "requestId": "fix-calculator-2026",
  "prompt": "Fix calc.js and run the exact acceptance tests",
  "allowWrite": true,
  "allowShell": true,
  "shellIsolation": "isolated",
  "requireEvidence": true,
  "acceptanceChecks": [
    {"program": "node", "args": ["--test", "calc.test.js"]}
  ],
  "maxSteps": 8,
  "tokenBudget": 15000
}
~~~

**Isolated shell execution:** the same `shellIsolation` setting is part of native Rust `TaskInput`, the public TypeScript SDK, `POST /api/tasks` and `/resume`, and the GUI task form. `GET /api/sandbox` probes real readiness. Model-proposed `workspace_check` calls and the independent acceptance verifier use `src/engine/sandbox.rs` with no host fallback. OS confinement is combined with the existing before/after source-SHA integrity gate; a green exit code alone is not success after source mutation. Connection settings and execution evidence show the actual backend. The default `host` mode keeps legacy behavior. See [native shell sandbox](docs/SHELL_SANDBOX.md) for setup, tested guarantees and limits.

The real Rust acceptance suite uses a mock LLM for reproducible model choices while invoking **actual Node test processes and actual filesystem edits**. It proves that one wrong edit fails, its diagnostic is included in the next model request, a corrected edit passes, an unchanged failing source is not retested, stale earlier green evidence is rejected, concurrent writers serialize, and a staged two-file change passes a joint test. See \`src/engine/acceptance_tests.rs\`, \`src/engine/verification.rs\` and [coding acceptance architecture](docs/CODING_ACCEPTANCE.md).

**Boundary:** these tests establish end-to-end functional integration, not an SWE-bench Verified success rate or equivalence to Claude Code/Codex on hidden problems. More work is needed on isolated branch adoption, general dependency management, browser validation and real-provider evaluation.

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

Tests are present in source for native execution, cost, persistence, context, workflow, video and migration. **GitHub Actions executes Rust compilation/tests and Node/TypeScript/frontend checks on the champion branch**, while real-provider benchmarking and full interactive browser smoke testing remain separate work. These tests do not establish external frontier performance or actual API invoices. Historical advanced pages remain in web/src as migration references rather than active broken screens; full desktop packaging, model-management UI, complex workflow branching, multi-user service hardening, frontier-grade autonomous success-rate measurement and generative cinematic quality remain open product work.

See docs/ARCHITECTURE.md and sdk/typescript/README.md. Benchmark only after all source paths and tests are verified.
