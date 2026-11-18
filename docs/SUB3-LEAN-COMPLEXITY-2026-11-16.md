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
