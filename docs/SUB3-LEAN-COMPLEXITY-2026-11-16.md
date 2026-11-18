# Sub3 — Lean agent execution architecture (2026-11-16)

Branch: `dominion-sub3` only. The current owner direction **supersedes** the former 100-point RSI/video-focused rubric and prior feature-growth guides.

## Actual removals

- `src/engine/rsi*.rs`: all standalone RSI research, optimizer, campaign, self-patch, generation and automatic acceptance loops removed physically. Ordinary source editing, source-hash protection, acceptance tests and owner-driven repair remain.
- `src/engine/video.rs`, `video_studio.rs`, `video_hub.rs`: built-in media generation, ComfyUI model download/installation, scene production, internal render/autopublish removed.
- `src/api/rsi.rs`, `src/api/video*.rs`, `src/api/benchmark.rs`, `src/api/experiments.rs`, `src/api/improvements.rs`: old feature/product/auto-experiment routes removed rather than remapped to non-working handlers.
- `web/src/pages/Video.tsx`, `SelfImprovement.tsx`, media-only subcomponents and memory growth dashboard are removed from source and navigation. `sdk/typescript` no longer declares RSI/media producer facades.
- RSI-only Rust tools and special automatic-write approval exception removed. CLI runner, periodic experiment scheduler, media smoke and standalone RSI benchmark GitHub workflows removed.
- Unused raster library `resvg` and its transitive dependencies removed from `Cargo.toml` and `Cargo.lock`.

## Retained critical capabilities

- Native Rust SDK and product adapter; existing chat SSE and rooms; files, memory, transcript history, corrections, job receipts, linked MCP/plugin channels, browser, OAuth, tools, local/remote models.
- Rust owner-approved edits, actual tool invocation, file SHA checks, tests, cancellation, step budgets, recovery and replay evidence.
- `jobs::retired_legacy_kind` refuses new old-media/RSI jobs. Recovering persisted old queued/running jobs cancels or marks review without executing side effects. Completed archived runs and user data remain stored.
- SQLite indexed active job fetch instead of scanning historic jobs every 400ms, Tokio Notify for immediate in-process enqueues, 5s durable catch-up for external writers.
- Default chat automatic learning disabled in Rust and UI; 5-minute growth/distillation/learning sweeps and post-chat self-experiment/strategy adoption disabled. Explicit owner-directed memory/procedure work stays available.

## Regression and release gate

`scripts/check-lean-product.mjs` asserts the **absence of removed source/routes/tools** and presence of retained core chat, memory, MCP, SDK, authorization, archived-job safety, indexed workers and nonmedia dependencies.

`.github/workflows/ci.yml`: manually dispatched Windows/macOS release-stage verification, including the TypeScript GUI/SDK and lean architectural gate; Linux is not a target. Ordinary development tests are scoped Rust development-profile tests. `sub3-product-acceptance.yml` and `sub3-rust-sdk-check.yml`: real agent, chat, recover, immutable tool evidence, API and provider regressions. **A green workflow belongs only to the exact HEAD it tested.**

Historical SQLite/job content is not deleted; existing audio/video files in a user's data directory are not removed by upgrading this source branch. They are simply no longer created by the built-in engine.

## Scope of future external media

Media can be done via separately installed and authorized MCP or plugin tools. No paid media provider is hardcoded, no local weight download occurs at startup, and no media-only CPU or GPU worker exists in the Rust product.

## 2026-11-18 — Bounded evidence retrieval without a second model loop

- `crates/dominion-agent-sdk/src/context.rs`: nonempty durable receipt searches inspect JSON scalar leaves and field names with streaming, Unicode-aware literal matching instead of serializing every historical tool result and allocating a second lowercased full-log string. The authoritative receipt journal, pagination, read-by-ID, and capped previews are unchanged.
- `src/engine/money_retrieval.rs`: selected facts still suppress highly similar duplicate headlines in the LLM context, but add up to two bounded provenance references and a count of related observations. `similarityStatus=lexical_overlap_not_independent_confirmation` prevents a title match being represented as verified independent reporting.
- `src/engine/money.rs`: the research planner sees at most one additional related source URL per selected fact, explicitly labeled as a similar source rather than independent confirmation. Full individual original evidence remains in the indexed Money corpus.
- Regression tests added for long nested multilingual receipt search, query boundaries, bounded duplicate provenance and planner-context citation limits. These tests were **added but not executed** in this editing session: `cargo`/`rustc` are unavailable in the current editing container, and full CI was intentionally deferred.
- Expected improvement: lower transient allocations during receipt keyword search; no claims about benchmark speedups, real-model completion rate, or verified CI until measured with the same workload.

## 2026-11-18 — One native harness, independent runtime adapters

Design boundary (compared with OpenClaw ACP Agents / acpx and Hermes toolsets; no external harness has been installed by this change):

1. **One authoritative host kernel:** `dominion-agent-sdk::Kernel` remains responsible for the task budget, proposed action admission, durable receipt journal, evidence, owner corrections and final status. No second planner, duplicate retry loop, external workflow executor, or self-promoting policy has been added.
2. **Evolving plan drives only discovery:** `tool_discovery::select` now gives up to three short-lived visible-schema slots to the latest three bounded model plan steps. The catalog is still the exact host-allowed set, `dominion_tool_search` remains the explicit progressive-discovery path, and `dominion_tool_call` still resolves through normal pinned host permission and dispatch. An explicit owner correction bypasses the old plan. This alters *visibility*, never *authority*.
3. **External harness is a distinct future adapter, not merged code:** Codex, Hermes, or ACPX may later run as independently versioned, opt-in runtimes. A future adapter will report its native session handle, environment binding, capabilities, progress, cancellation acknowledgment, final result and evidence to Dominion's existing job/session/receipt contracts. It must not claim an external effect has been committed solely because an RPC or subprocess returned. Uncertain external run termination requires review rather than blind restart.
4. **Domain modules remain removable:** Money, MCP, Skills, RAG and media via external plugins may provide capabilities, but must not introduce new authoritative execution loops or bypass the host's user scope, tool permissions and budgets.
5. **No premature integration:** current `src/engine/native_agent.rs` binds the MCP manifest before dispatch; `src/engine/chat/runtime/` still has an older separate interactive path. This change does not claim those paths have been merged or that an ACP backend is running.

Added a source-level regression for adaptive tool visibility and stale-plan suppression. It was committed without local Cargo execution; the corresponding Rust test and full CI remain **not run** in this session. External runtime adapters and full chat/kernel unification are **not implemented** by this patch.

## 2026-11-18 — Capability research and progressive Skill disclosure

External source-code/design comparison (research is advisory, no external runtime is bundled or merged):
- Pi ([extension registration](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/src/core/extensions/types.ts)): a small agent-core boundary with externally registered tools is preferable to duplicating Dominion's kernel.
- mini-SWE-agent ([repository](https://github.com/SWE-agent/mini-swe-agent)): the Bash-centered minimal loop is a useful cost/complexity contrast, not a reason to replace typed native dispatch for smaller local models.
- Goose ([official extensions](https://goose-docs.ai/docs/getting-started/using-extensions/)): built-in Skills discovery/load and MCP-hosted tools can be enabled without new agent loops.
- OpenHands ([official SDK architecture](https://github.com/OpenHands/docs/blob/main/sdk/arch/overview.mdx)): agent code, workspace executors, tools and server should retain separable contracts and ownership.
- DeepSeek Harness ([official architecture](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md)): reversible plugin lifecycles are worth comparing with Dominion's owner-enabled modules; its whole agent loop is **not** imported.
- Microsoft Agent Framework ([2026-08-10 Skills](https://learn.microsoft.com/en-us/agent-framework/agents/skills)): Skills have metadata → instructions → references → optional scripts, progressively exposed. Dominion implements only read-only discovery/instruction/reference stages. Script execution stays under existing owner-approved tools.

Implemented on sub3 (without a new planner/engine):
1. `src/engine/skill_discovery.rs`: metadata-only bounded relevance selection of owner-enabled Skills; no blind first-three insertion; stable tie-breaking; no model call; zero instructions for unmatched Skills.
2. `src/engine/tools.rs`: `skill_search`, `skill_read`, `skill_resource_read` through the existing Rust host and tool journal, with separate reads for full instructions/references. Skill resources reject absolute paths, traversal and canonical symlink escape, require UTF-8, cap filesystem bytes and returned text. Both instruction and reference receipts include the SHA-256 of their full source content so truncation does not destroy evidence lineage. There is **no** script auto-execution, MCP enablement, permission escalation or source promotion.
3. `src/engine/native_agent.rs`: at most two task-relevant enabled Skill excerpts in the original prompt. `src/engine/chat/runtime/selection.rs` surfaces the same read-only tools for relevant chat requests. `crates/dominion-agent-sdk/src/runtime.rs` classifies them as read-only, preserving existing scheduler admission.
4. Tests cover a relevant Skill beyond the first three, disabled/draft/session/partition filtering, external-channel opt-in, deterministic ranking, live store lookup and revocation, bounded tool/prompt output, path traversal and symlink escape.

Status: code and source-level tests committed; Rust compiler, tests and full CI not run in this environment. No measured task success/latency/token advantage is claimed. This is one substantial **capability-broker** improvement, not adoption of Pi, Goose, OpenHands, DeepSeek, Codex or Hermes as a product runtime.

## 2026-11-18 — Legacy harness API cutover without copying runtimes

**Observed pre-change duplication:** `/api/harness/runs` started `dominion_harness::AgentSdk::run` with its own `sdk_run` record, `sdk-runs/*.json` FileJournal, approval and resume flow. `/api/sdk/runs` already used `jobs::create` → `engine::native_agent::run` → `dominion_agent_sdk::Kernel`, with the host-owned durable SQLite job/operation journal. The old `crates/harness` binary and the interactive `chat/runtime/turn.rs` loop are separate paths.

**Now:** A NEW POST to `/api/harness/runs` forwards to `sdk_runs::create` and the existing native job kernel. Its `harnessCompat=true` marker is persisted only to keep the two public API history views separate. `sdk::list/one/approve/cancel/resume/reconcile` dispatch to native handlers for this marker and retain old handlers for preexisting `sdk_run` checkpoints. The old `AgentSdk::run` is not invoked for new requests to this HTTP API. Existing legacy files, historical entries and in-doubt effect state are **not** migrated, reinterpreted, or discarded. The standalone `crates/harness` CLI is not removed, and chat remains a separate loop.

**No permission expansion:** New compatibility jobs pin only the previous workspace list/read/write/replace operations and approved shell, with the former 160-call cap. They cannot silently discover or execute browser, external account, web, arbitrary-host-file or MCP tools. The native tool host rejects a model-provided nonempty `cwd` for legacy-surface shell invocations because the old harness used the fixed workspace root. Native jobs continue using the existing approval, validation, effect journal and verification gates. Default native SDK jobs remain unchanged.

**Code and source-level regression coverage:**
- `src/api/sdk.rs`: new run is a native job with `harnessCompat=true`, max rounds and model preserved, restricted tool set and no new `sdk_run`; public panel histories are isolated; previous `sdk_run` records remain readable.
- `src/engine/native_agent.rs`: legacy native shell must not select a different work directory.
- `src/api/sdk_runs.rs` + `src/api/jobs.rs`: marker is passed only as UI/history scope, no second worker or authority.

**Status:** committed, with GitHub file/head verification planned. Local Cargo/rustc is not installed here, so changed Rust tests and CI **were not executed**. No measured latency, token or completion-rate gain is claimed. This is a safe **new-HTTP-run cutover**, not total one-loop completion; retiring the legacy binary and unifying chat, external ACP runtimes or historical checkpoint formats requires separate verified migration work.

## 2026-11-18 — Completed retirement of the redundant standalone Rust harness

The earlier cutover section above documents the transition **before** the final cleanup; the following supersedes its references to preserving the old standalone runner.

- Removed the entire `crates/harness/` package (independent `AgentSdk::run` loop, blocking OpenAI driver, tool registry, and CLI binary) from `dominion-sub3`. Root `Cargo.toml` and `Cargo.lock` no longer list it.
- Replaced `src/api/sdk.rs` (formerly 474 lines) with a 260-line compatibility adapter. **New** `/api/harness/runs` calls route to the existing `dominion-agent-sdk::Kernel` worker. **Old** `sdk_run` and bounded FileJournal snapshots remain read-only; no legacy resume/approval/reconciliation attempts can redispatch potentially uncertain side effects.
- Corrected the previously failing tool-discovery directive regression (`Inspect code` was not exposing a code-reading tool) and added architecture guards that reject restoration of the retired parallel crate/legacy API executor.
- Targeted GitHub Actions runs at commits `74a3280` (Sub3 product Rust tests) and `0f643f7` (Sub3 Rust SDK tests) both completed successfully. They do not establish full release CI or verify later documentation-only commits.
- **Remaining active design work:** streaming chat `src/engine/chat/runtime/turn.rs` has a separate interactive turn loop, and the optional `src/engine/chat/runtime/harness.rs` procedure runner executes owner-enabled `harness_assisted` strategies. These are active product semantics, **not** proven-dead code; deleting them without migrating chat continuity, owner policies and approval receipts would be an unverified behavior regression. Neither is claimed unified or removed here.
- The stand-alone CLI instructions in the old HARNESS document were replaced with accurate live/archived API instructions.

Verification guarantees are limited to the exact workflow/test scopes above. No release build, cost/latency comparison, external harness installation or main-branch modification is claimed.
