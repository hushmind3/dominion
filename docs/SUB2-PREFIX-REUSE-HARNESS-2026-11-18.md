# Sub2 — 2026 per-conversation prompt prefix and cache evidence

Date: 2026-11-18. Scope: `branch2` only; Rust server/agent SDK. No frontend changes, no `main` or other branch edits, no release build.

## User requirement

A session's approximately 4,000-token common charter and persona should be paid or loaded **once** where the backend can retain them, not reprocessed on each subsequent owner/tool turn. The metric must distinguish bytes sent to an API, tokenizer-reported total prompt tokens, *provider-confirmed cached tokens*, and true server-side stateful continuation.

## Backend contracts verified in November 2026

- **OpenAI Codex Responses WebSocket**: `previous_response_id` on a live socket preserves earlier input items, including initial `developer` persona. The branch's earlier WS change sends the initial persona as developer input, then only appended items on later successful chain calls; full rebase occurs on persona change, dropped connection or unsupported role. Top-level `instructions` are not inherited, so they must not be silently omitted from stateless HTTP.
- **DeepSeek direct Chat Completions and current DeepSeek Responses API**: the latter currently does not implement `previous_response_id` or `store=true`; HTTP cannot silently skip the persona. DeepSeek context prefix caching may avoid *recomputing* identical input. Provider-reported `prompt_cache_hit_tokens` or `prompt_tokens_details.cached_tokens` are the evidence, while reported `prompt_tokens` often include cached reads.
- **OpenCode Go (DeepSeek V4.1 Flash)**: the official service requests a stable per-conversation `x-opencode-session` and documents both the older `https://opencode.ai/zen/go/v1` and newer `https://opencode.ai/inference/go/v1`/`openai/v1` Chat endpoints. Public November 2026 reports include provider-side cache misses when effort changes and other intermittent upstream cache misses. An HTTP client cannot repair the serving provider's KV state. We retain correct headers and log exact cache evidence instead of inventing savings.
- **2026 Qwen Code and DeepSeek Harness**: Qwen's published `global-tool-schema-stable-sort` makes same-tool sets byte-stable regardless of MCP registration timing. DeepSeek Harness' reconstructable-request architecture keeps a fixed prompt header and appends runtime context rather than rewriting its prefix. Existing DOMINION already separates volatile retrieved context from the stable system prompt; this patch integrates the *additional* deterministic schema order and per-session affinity natively in Rust, without embedding an incompatible Node/TypeScript execution engine.

## Actual execution-path repairs

1. `src/engine/chat/continuity.rs`: `participant-cache-v3` scopes model cache routing by **session and participant** in addition to provider route and identity version. Previously the same agent in another room shared one `agent-cache-v2` key despite different conversations; this could thrash the provider's worker cache. Intentional one-time migration to a new cache namespace.
2. `src/engine/chat/tool_projection.rs` + `runtime/turn_body.rs` + `provider.rs`: deterministic stable tool ordering with pinned discovery gateway then essential workspace read/edit/test capabilities and alphabetically sorted optionals. Both the tool selection/round budgets and the provider's actual transport preparation use the same order. A capability's permissions, approval and audit paths do not change.
3. `src/engine/gateway.rs`, `engine/llm.rs`, `engine/compact.rs`: exact-match official OpenCode Go path detection; a stable, opaque Go session header on **auxiliary** planner/reviewer/compactor HTTP requests as well as normal chat. Helper headers are scoped separately from the user's agent conversation by session + helper static system digest, so a summarizer's different system prompt cannot collide with the main conversation cache. Even no-session helper calls get a distinct stable hash namespace. A mock request-builder regression confirms the header is actually present; other hosts and general Zen routes receive none.
4. `src/engine/chat/continuity.rs` + `runtime/turn_body.rs`: `cachePrefixParts` hashes the actual static system content, tool schemas (with names retained), and the generation control options separately. `prefixChange` classifies `initial`, `prefix_stable`, `system_or_persona_changed`, `tool_set_changed`, `tool_schema_changed`, `model_options_changed`, and migration. Each provider round logs cache-read permille **only with actual provider usage**, plus estimated wire tokens, tool counts and a `benchmarkCacheDisabled` flag when the benchmark gateway explicitly switches caching off. Cache key stability is not conflated with successful provider KV reuse.

## Verification

Scoped Rust CI adds/retains these tests:
- `tool_projection::tests::canonical_tools_preserve_cache_prefix_across_mcp_registration_order`.
- `continuity::tests::same_persona_and_canonical_tools_keep_prefix_diagnostics_stable`.
- `continuity::tests::continuity_survives_restart_and_separates_bots_routes_and_prefix_revisions`.
- `continuity::tests::opencode_go_affinity_is_auto_detected_only_for_the_real_go_endpoint`.
- `gateway::tests::helper_go_requests_have_stable_isolated_session_affinity` (including HTTP builder header assertion).
- Existing `codex_ws::tests::developer_persona_is_wire_sent_once_and_rebased_on_revision` and atomic cursor tests.

The owner-provided pre-patch Sharutia + DeepSeek V4.1 Flash benchmark has coding `sub2=34,205 tokens / 3-of-3 success / 40.2 seconds`, versus `main=27,191 / 3-of-3 / 30.1 seconds`. Group 9-agent 3-turn totals were `sub2=80,238`, `main=74,764`. These are historical baselines, not new results. Re-run exactly the same prompts and route after CI, **without changing provider tools or credentials**, and compare `providerRounds` and the actual artifacts. Total provider input tokens and cached-token usage are separate columns; never subtract cached tokens from the provider's reported total use.

## External evidence (2026)

- OpenCode Go official: https://dev.opencode.ai/docs/go/ and https://dev.opencode.ai/v2/docs/console/go/
- DeepSeek Responses API capabilities: https://api-docs.deepseek.com/guides/responses_api/
- DeepSeek context caching: https://api-docs.deepseek.com/guides/kv_cache/
- Qwen Code canonical tool schema order: https://github.com/QwenLM/qwen-code/blob/main/docs/design/prompt-cache/global-tool-schema-stable-sort.md
- DeepSeek Harness reconstructable requests: https://github.com/deepseek-ai/deepseek-harness/blob/master/.agents/notes/implemented/architecture/2026-07-05-reconstructable-requests.md
- OpenCode Go cache / effort diagnostics: https://github.com/anomalyco/opencode/issues/63935

No third-party harness binary is bundled because existing Rust kernel, gateway and tool executor already support these algorithmic seams, and a different harness cannot manufacture stateful continuation for a stateless remote API.
