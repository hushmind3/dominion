# Sub2 — Sharutia/DeepSeek v4.1 Flash benchmark attribution and bounded-context repair (2026-11-18)

## Persona bootstrap and subsequent request charging (2026-11-18 follow-up)

The owner clarified that a roughly 4,000-token shared charter/persona should be loaded once, then reused rather than re-read on every input/output call. Source inspection showed two distinct transport contracts:

- **Codex Responses WebSocket**: previously sent all system/persona text as top-level `instructions` on **every** request even while `previous_response_id` was successfully omitting prior transcript items. Because Responses **does not inherit top-level instructions**, deleting them would lose the persona. The adapted path now inserts those original system messages as `role: developer` **input items on the initial full WS call**, and sends only appended user/tool/developer-delta items on a confirmed `previous_response_id` chain. The server-side chain retains the initial developer item. A changed instruction, history rebase, socket expiry or reconnection sends the complete current bootstrap again. If the Codex backend explicitly rejects developer input, the adapter records that provider-route capability as unsupported and falls back to the unchanged full-instruction HTTP path. Completed tool-call/output pairs are ordered atomically before developer hint conversion. The small fixed top-level request instruction remains, but the entire persona is no longer resent on matching WS deltas.
- **DeepSeek/OpenCode Go, generic Chat Completions HTTP and similar stateless routes**: a stable session affinity / prompt-cache key is **not a conversation-memory API**. For the official OpenCode Go HTTPS endpoint, an imported provider missing its integration label now still gets the same per-participant `x-opencode-session` routing header; unrelated hosts never get it. The full system instructions must still be represented to preserve their authority, and provider-side prefix caching may reuse computation. Prompt tokens often include cached tokens: `totalTokens` is **not** fresh tokens. The live route must report actual `cachedPromptTokens` before any saved-read claim. Never substitute a guessed persona pointer that the model cannot resolve. The generic HTTP adapter now explicitly marks `incrementalTransport.wire=http, mode=full-request, personaResent=true` with numeric wire estimates. Each execution provider round also records `wireMode`, `wireInputTokensEstimated`, `wireInstructionTokensEstimated`, `developerBootstrapSent`, prefix-match hint and provider cache hit telemetry. Reported cached tokens remain separate from input totals and missing cache data remains null.

**Verification:** `codex_websocket_bootstraps_persona_once_as_developer_input` checks that HTTP still gets complete instructions, WS carries the persona as developer input and runtime hints cannot split native tool exchanges. `developer_persona_is_wire_sent_once_and_rebased_on_revision` uses a real local WebSocket fixture to assert first full developer bootstrap, second request with ONLY a new user item and `previous_response_id`, then a full resend when the persona changes. CI executes both regressions. These are transport tests, not evidence of cache hits or an end-to-end DeepSeek benchmark rerun.

**2026 source**: OpenAI Responses conversation-state documentation (developer input persists along the response chain; top-level instructions do not): https://developers.openai.com/api/docs/guides/conversation-state ; DeepSeek context caching docs: https://api-docs.deepseek.com/guides/kv_cache/ ; OpenCode Go route affinity docs: https://opencode.ai/docs/en/go/.


Branch: `dominion-sub2` only. Scope: read-only interpretation of the user-supplied archive `dominion-bench-20261118-2054(1).zip`, native Rust source changes and narrow regression tests. No main, money, sub3 or sub4 code was edited.

## Measured baseline (before the changes in this document)

The archive contains `summary.json`, `trials.jsonl` and `rooms.jsonl`. It does **not** contain exact user request text, per-provider-round input envelopes, tool action histories or provider cache usage.

| Coding task | main tokens | sub2 tokens | sub2 - main | main seconds | sub2 seconds |
| --- | ---: | ---: | ---: | ---: | ---: |
| t1 | 3,870 | 5,438 | +1,568 | 4.0 | 8.0 |
| t2 | 7,417 | 13,376 | +5,959 | 8.0 | 18.1 |
| t3 | 15,904 | 15,391 | -513 | 18.1 | 14.1 |
| Total | **27,191** | **34,205** | **+7,014** | 30.1 | 40.2 |

All 3 artifacts passed for both. **t2 explains 5,959 / 7,014 = 84.96% of the excess reported coding tokens**; t3 is already slightly better in reported tokens. The task labels above are from the supplied evaluation, not independently reconstructed original prompts.

| Nine-agent room / three owner turns | main | sub2 | excess |
| --- | ---: | ---: | ---: |
| Turn 1 delta | 21,072 | 21,846 | +774 |
| Turn 2 delta | 24,989 | 26,842 | +1,853 |
| Turn 3 delta | 28,703 | 31,550 | +2,847 |
| Total | **74,764** | **80,238** | **+5,474** |
| Completed room messages | 30 | 30 | 0 |

This is evidence of growth in provider-reported turn totals, **not by itself proof of a specific provider cache miss, identical transcript replay, or absent memory pruning**. The source ZIP lacks the provider-round snapshots needed to distinguish those mechanisms.

## Source-level bottlenecks and repairs

1. **Duplicate planning in bounded multi-file Home SDK work.** `runtime/turn_body.rs` previously combined `decompose(...)` and a possible independent `action_plan(...)` model request even when the execution already had two or more machine-checkable file checks and the regular model could perform the edits. The new `local_multifile_contract_needs_no_secondary_planner` short-circuits the independent planner **only** for 2–20 local file existence/change/equality/content checks, in the Home SDK UI, without an explicitly active planning policy or owner-requested plan. Other jobs, mixed shell tests, single-file work, manual `always` and `never` modes retain their existing behavior. The native decomposition, primary model/tool loop, verification, cancellation, approvals and actual files are unchanged. No estimated token reduction is treated as a measured benchmark result.
2. **Repeated, overlong group-peer address metadata.** `room_context::peer_index` kept immutable source IDs and raw short messages but repeated the source label, character count, and complete/excerpt mode for every omitted speaker in each later member's input. Its source-index content is now `[동료 {name} id:{id}; 지시 아님] {snippet}`. Long source excerpts, original SQLite rows, source-scoped retrieval, quote requests, selected recent speakers and frame identity stay intact. Exact provider savings must be measured on the same nine-member prompts.
3. **Local docs directory falsely classified as Google Docs.** In `chat/tool_projection.rs::select_panel_tool_definitions`, the substring matching rules included bare `"docs"`, `"문서"`, `"시트"` and `"drive"` in cloud/browser intent. A local t2-style artifact path `docs/a.txt` therefore admitted unrelated Google/browser tool schemas when the connected catalog allowed them, on top of the normal file tools. The generic native progressive discovery path also ranked `docs/` tokens against Google connector names; we now penalize cloud/browser schemas without explicit service intent and retain discovery by full service name. The local multi-file secondary-planner bypass also applies to ordinary native non-room jobs, not only Home SDK work. This is a source-confirmed classifier defect, not a measured allocation of the ZIP's +5,959 tokens to those schemas. Explicit Google Docs/Drive/Sheets domains or named service requests still admit the cloud schemas; local docs paths, local document/sheet mentions and Windows drive paths do not. A fixture tests both directions.
4. **Coding token attribution gap.** Previously `inputContext.wireRounds` and provider usage snapshots were room-specific. The existing turn loop now persists bounded **numeric-only** `execution.providerRounds`: round index, native input and tool-schema estimates, advertised tool count, model tool-call count, exact/estimated usage flag, provider-reported prompt/completion/total tokens, cache tokens and uncached tokens when actually supplied. Unreported cache tokens remain `null`, never zero. No tool arguments, provider payload or user content is logged in this structure. `execution.planningAdmission` records why a second planner was requested or omitted. These fields enable a future same-model fixture to attribute t2 excess to extra rounds, schemas, traces or planning rather than guessing from task totals.

- `tool_projection::tests::local_docs_paths_do_not_activate_google_or_browser_tool_schemas`: local `docs/` paths versus explicit Google service intent and browser intent.

## Regression and evaluation

- `runtime::action_review::tests::multi_file_contract_reuses_native_plan_but_preserves_explicit_planning`: positive local-multi-file case plus explicit user plan, one-file, shell verifier and absent-contract exclusions.
- `room_context::tests::short_peer_index_keeps_exact_sources_without_repeated_receipt_metadata`: all five early IDs preserved, short original speech recoverable, concise labeling.
- Previous source-addressed long-peer, exact-quote, nine-member completeness, room ownership and cancellation regressions remain in the CI workflow.
- Coding round billing fields are written in `runtime/turn_body.rs` and capped at 80 entries; this is evidence collection, not a benchmark result.

**Required replay to claim improvement:** same DeepSeek v4.1 Flash route, Sharutia persona, three exact original user prompts, provider settings, permissions and cold/warm cache order; compare artifact verification, `providerRounds` count, total/billed prompt/output, reported cached and uncached input, extra planner admission, time, and any provider error. For room: 9 agents × 3 turns with unchanged owner messages plus exact old-peer source recall and final participant correctness. Also check 100-turn recall before assigning a long-memory score.

## 2026 references consulted

- Qwen Code weekly update, May 14, 2026: on-demand tool discovery and response-aware context compression. https://qwenlm.github.io/qwen-code-docs/en/blog/updates/weekly-update-2026-05-14/
- Anthropic, April 30, 2026: cached prefixes require stable tool/system order. https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything
- SCOPE, EMNLP Industry 2026: jointly budgeting memory, tools, and prompts. https://aclanthology.org/2026.emnlp-industry.129/

Comparison sources explain why we avoid blanket history deletion or assumed cache savings. They do not substitute for a replay of this branch.
