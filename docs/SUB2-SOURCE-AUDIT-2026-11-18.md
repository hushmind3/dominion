# Sub2 source, entrypoint and README audit — 2026-11-18

Branch **\`dominion-sub2\`**. Scope: source-tree inventory, representative runtime-critical path inspection, concrete fixes, focused regressions, and bilingual README consistency. Other branches have not been modified.

## Inventory and audit method

GitHub tree inventory at \`f4ded2d2\` (before this audit): **443 tracked blob paths**, including **199 Rust** and **37 TSX** files. Parsed both root README files, inspected local references, and walked the significant entrypoint paths. The tree inventory covers all tracked paths, but this is **not a claim that every line of 199 Rust modules was manually reviewed** or that all external integrations have been integration-tested.

Actual source files examined include:
- Startup, routing and SDK: \`src/main.rs\`, \`src/bootstrap.rs\`, \`src/config.rs\`, \`src/platform/mod.rs\`, \`src/api/mod.rs\`, \`src/sdk.rs\`, \`src/api/sdk_bridge.rs\`, \`src/api/jobs.rs\`, \`sdk/typescript/src/index.ts\`, \`sdk/typescript/package.json\`.
- Execution and correctness: \`src/engine/jobs.rs\`, \`src/engine/autonomy.rs\`, \`src/engine/verification.rs\`, \`crates/dominion-agent-kernel/src/lib.rs\`, \`crates/dominion-agent-kernel/src/cycle.rs\`, \`src/state.rs\`, \`src/events.rs\`.
- Chat/rooms/cost/cancellation: \`src/engine/chat/runtime/turn_body.rs\`, \`src/engine/chat/runtime.rs\`, \`src/engine/chat/provider.rs\`, \`src/engine/chat/codex_ws.rs\`, \`src/engine/chat/continuity.rs\`, \`src/engine/chat/context.rs\`, \`src/engine/chat/room_context.rs\`, \`src/engine/rooms.rs\`, \`src/engine/room_state.rs\`, \`web/src/pages/Chat.tsx\`.
- Tools, memory and other capability ownership: \`src/engine/tools.rs\`, \`src/engine/memory.rs\`, \`src/engine/models.rs\`, \`src/engine/money/mod.rs\`, \`src/engine/mcp.rs\`, plus the source-tree inventory of \`src/api/money/\`, \`src/db/\`, \`web/\`, tests and documentation.

Checked cross-layer route topology:

\`\`\`text
web/Chat.tsx or web/NativeAgentPanel.tsx
     → /api/chat or /api/sdk/runs → Rust runtime/jobs
     → native kernel → provider stream + approved tools
     → SQLite receipts + job/execution state + event bus
     → UI stream and status
external @dominion/sdk → same /api routes
embedded src/sdk.rs → same Rust engine (no network layer needed)
\`\`\`

Root README links were checked against the current tree; all 20 distinct local relative links in each rewritten language README resolve **after this audit document is committed**. Their main \`# DOMINION\` heading now comes before the feature descriptions and development notes. Historical detailed reports continue to live under \`docs/\` and \`benchmark/\`; they were not deleted.

## Confirmed code defects and fixes

| Scope | Source-level finding | Change | Regression |
| --- | --- | --- | --- |
| Skill loading | \`skill_read\` returned only first 12,000 characters; \`truncated\` could be true, but callers had no way to retrieve the remainder. Thus "full skill" was impossible for long approved skills. | Added optional **Unicode character \`offset\`**, fixed 12,000-character pages, \`nextOffset\`, \`totalChars\`, \`hasMore\`. Out-of-range reads fail. Scope/activation checks remain unchanged. | \`engine::tools::tests::skill_read_requires_enabled_visible_skill_and_returns_original_body\` checks 12,001-character UTF-8 pagination, draft/private/missing/out-of-range refusals. |
| Codex cache budget | The previously introduced larger retained-history horizon was applied whenever the configured route kind was Codex and WS was not disabled—even if prior calls had **fallen back to HTTP**. This could repeatedly enlarge billable full-input replays without confirmed incremental benefit. | \`continuity::has_confirmed_ws_history\` now checks the **committed prior reply for this exact provider route**, requiring \`metadata.continuity.incrementalTransport.wire == "websocket"\`. No previous route reply or prior HTTP fallback keeps the normal history budget. | \`continuity::tests::a_committed_ws_transport_not_provider_kind_alone_admits_longer_history\` exercises confirmed WS, HTTP and absent source. |
| Tool-disabled contexts | Compact index mode admitted 12 skills, but when tools were unavailable each could carry 700 chars of eager instructions, increasing worst-case prompt size compared with the former five-skill bound. | Keep **12** compact index entries when \`skill_read\` is usable, but at most **5** bounded full instruction entries when tools are disabled. | \`context::tests::skill_manifest_is_compact_sorted_and_never_embeds_full_instructions\` plus existing tool-disabled context paths; additional live/token benchmarking remains outstanding. |
| Native SDK idle overhead | \`Dominion::wait\` polled the SQLite job row every **150ms** per waiting Rust caller; 10 concurrent callers could cause ~67 reads/second even with no work. | Subscribe **before** checking durable state; await matching \`job\` Bus events instead of polling unrelated activity, with a **2-second** DB reconciliation timeout for missed/externally written state. Exact timeout, terminal states, and restart reading remain intact. | \`sdk::tests::native_wait_wakes_on_matching_durable_event_and_stops_polling_idle_jobs\` plus existing timeout/SDK tests. |

**Important WebSocket qualification:** A provider socket may fail *during* a call after the prior confirmed WS response, forcing one larger full fallback. The change prevents ongoing enlarged full replays after that fallback; it cannot guarantee that one transient fallback never happens. The original SQLite messages and long-term owner facts remain unchanged.

## Existing audited contracts and remaining operational risks

| Component | Read/code path | Contract and open risk |
| --- | --- | --- |
| Job verification | \`jobs::run_prompt\` → \`autonomy::execute\` → \`verification::check_acceptance\` → native kernel cycle | Owner criteria and receipt chronology govern completion. External-file correctness and \`job.status\` are separate; unknown external effects must not be auto-accepted. Live coding t2 \`needs_review\` remains unproven without original job evidence. |
| Cancellation | \`web/src/pages/Chat.tsx\` → \`/api/sessions/:id/cancel\` → session cancel epoch → provider/tool supervisor → \`/cancel-status\` | Native cancellation and the UI settlement path are present; a remote provider may still compute/charge before observing HTTP disconnect. A tool-side external mutation may require owner reconciliation. |
| Group context | \`rooms.rs\` + \`room_context.rs\` + \`room_state.rs\` + \`continuity.rs\` | User-confirmed facts stay durable and distinct from selectively retrieved peers. The former 2026-11-09 100-turn experiment reported **0/9 late recall**; do not claim the current code has fixed long-run recall without a new run. |
| Provider cache | \`provider.rs\`, \`codex_ws.rs\`, \`continuity.rs\` | Prefix/key affinity helps only where the provider actually supports cache reuse. \`cachedPromptTokens: null\` is unknown, not zero. Cache-hit rate and 9-agent costs require provider measurements. |
| Money | \`engine/money/\`, \`api/money/\`, \`db/money.rs\` | Original source and hypothesis/review should remain inspectable. Evidence discovery is not evidence that an opportunity will be profitable or an authorization to trade/deploy. |
| API/SDK | \`api/mod.rs\`, \`api/sdk_bridge.rs\`, \`src/sdk.rs\`, \`sdk/typescript/\` | Rust native and typed TS client share the backend; HTTP-only \`input.successCriteria\` is not asserted to be an exposed \`Prompt\` field in the Rust SDK. |
| Maintainability | Larger single-responsibility stress points: \`engine/models.rs\` (~127kB), \`memory.rs\` (~119kB), \`tools.rs\` (~111kB), \`chat/provider.rs\` (~105kB), \`chat/runtime/turn_body.rs\` (~102kB), \`db.rs\` (~100kB) | These modules concentrate multiple provider/model/storage policies. Do not split blindly: extract one responsibility at a time with API/receipt/approval regression tests. Risk is documented, not scored as a proved runtime failure. |
| Packaging | \`src/platform/mod.rs\` handles \`--home\` and \`--open\`; \`web/package.json\` requires Node 24.12+ | The README quick start is checked against real entrypoints. Desktop installers and real-model providers were not tested by development CI. |

## README corrective actions

- Replaced chronological changelog before \`# DOMINION\` with short product summary and a proper front page (English and Korean).
- Pointed development workflow badge to **\`dominion-sub2\`**, rather than silently displaying \`main\` test status.
- Checked installation: \`--home\` is handled by \`src/platform/mod.rs::prepare_launch\`; \`Config::from_env\` resolves \`DOMINION_HOME\`, data, web, workspace and port. \`npm ci --prefix web\` requires the source \`web/package-lock.json\`.
- Described the one native engine, correct \`/api/chat\`, \`/api/jobs\`, \`/api/sdk/runs\`, per-request cancellation and embedded/TypeScript SDK entrypoints. Distinguishes API-only fields from Rust \`Prompt\` fields.
- Removed stale self-contradictory claims of "CI not run" that belonged to historical milestones; added an explicit **actual CI versus live provider/release** distinction instead.
- Preserved screenshots and links to historical detailed reports; new READMEs are navigation/onboarding documents, not permanent accumulation of every internal development commit.
- All local README references were checked against the repo tree; none is intentionally dangling.

## Verification and benchmark status

A code-bearing commit of this audit, SHA \`63d63f86\`, was checked by the [GitHub Actions scoped Rust + TypeScript workflow](https://github.com/hushmind3/dominion-rust/actions/runs/44288878083); its **rust-regressions** and **frontend-typecheck** jobs both succeeded. Subsequent README changes are documentation-only and do not alter compiled code. The next README/audit-doc commit should be checked for link validity, not counted as a new compiled-code CI proof.

The source audit itself does **not** run a live model or 100-turn benchmark. Do not infer any improvement in prompt cache hit rate, 9-agent recall quality, completion rate, or billed costs from these fixes alone.

Historical baselines (from repository files):
- [Room 100-turn Luna run (2026-11-09)](../benchmark/ROOM-100-TURN-LUNA-2026-11-09.json): 2,630,782 prompt tokens, 18.15% cached tokens, 621 history-budget rebases, late recall 0/9.
- [Codex WS incremental run (2026-11-08)](../benchmark/CODEX-WEBSOCKET-INCREMENTAL-2026-11-08.json): third-turn wire input reduced 76.9%, cost-effect not stable.
- User-supplied **2026-11-18 18:17** coding benchmark: sub2 produced **3/3** verified files, 71,286 coding tokens, 81,311 9-agent/3-turn room tokens, coding t2 \`needs_review\`. Raw user ZIP was not available to this audit; the observed t2 root cause cannot be assigned to one code path.

## Relevant 2026 public design references

- [Hermes prompt assembly](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/prompt-assembly.md): skill index and stable prompt structure.
- [Hermes context compression/caching](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/context-compression-and-caching.md): usage anchors, on-demand recovery pointers.
- [Anthropic tool context management](https://platform.claude.com/docs/en/agents-and-tools/tool-use/manage-tool-context): defer-loading tools, tool discovery and prompt prefix stability.
- [Anthropic Claude Code cache engineering (2026)](https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything): fixed-first/dynamic-last and cache-safe tool schema changes.

These references inform the review; implementation and verification claims above are grounded in DOMINION's own source and CI, not assumed from external agents.
