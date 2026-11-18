# Sub2 native chat Stop, safe settlement and Cloudflare group-prefix caching

Date: 2026-11-18. Scope: **`dominion-sub2` only**.

## User-visible symptoms

- Previous Chat composer replaced Send with a spinning disabled button. The preexisting `/stop` command and `POST /api/sessions/:id/cancel` were not wired to a button.
- `AbortController.abort()` alone only closes the browser's SSE connection. `POST /api/chat` starts a detached Rust task; terminating its SSE reader does **not** guarantee the model future was dropped.
- The owner's later 9-agent, 3-owner-turn benchmark reported sub2 at 73,001 input tokens; cache-hit metrics were absent. 9 separate personas historically appeared before shared room instructions, reducing the identical prefix between participants.

## Chat Stop contract

`web/src/pages/Chat.tsx` now swaps Send for Stop while a turn is active, including during a silent/long-thinking model request. A per-turn `crypto.randomUUID()` accompanies `POST /api/chat`. Stop calls `POST /api/sessions/:id/cancel` **without reusing the stream's AbortSignal**, including the same `requestId`.

The cancel API records a durable `chat_cancel_request` tombstone (UUID, immutable session owner, time, cancelled), then invokes the native `AppState::cancel_session` epoch cancellation. Late `POST /api/chat` checks this marker before it claims the session mutex, after claiming it and again after the normal reset. This fences Stop-before-POST races that a session-wide cancellation epoch alone would miss.

The current `supervise_tool_cancellation` selects local Notify and durable cancellation generation against an outstanding provider send / response stream. On cancellation with no live tools the HTTP future is dropped. If an external tool is still running, the supervisor drains its result for up to its existing limit and records any unresolved operation as `effect_unknown` / `needs_review` rather than pretending a remote mutation was undone.

The ordinary chat runtime also checks the epoch and request tombstone **before saving its final assistant message**, and checks the epoch after the supervised model returns before marking success. SSE updates associated with a stopped browser request are ignored, preventing stale UI updates.

The Stop button waits on the read-only `GET /api/sessions/:id/cancel-status` endpoint until the process-local session lock and durable `execution.status=running` journal are both cleared. While this is not true, the UI remains in *Stopping…* and disables Send; a time-bounded settlement failure leaves Stop available to retry, not a false completed state.

This stops the **client-side Rust HTTP future**. A remote model provider may have already accepted the request and may still charge for tokens computed before it observes the dropped connection; local source cannot promise remote GPU cancellation.

Existing background job, room, CLI and Discord callers use `request_id:None` and retain their historical semantics. `/stop` remains supported.

## Cache-friendly room hierarchy

`src/engine/chat/room_context.rs` now builds the system message with a shared immutable prefix first:

1. DOMINION base identity and owner global instructions
2. Shared room charter, participant roster and topic, tool permission policy
3. Current agent's name, ID, private persona and optional run-specific guidance
4. The existing selected immutable owner facts, room state, memories, peer excerpts and current messages (separate selected history frames)

Room memory and tool output remain grounded, selection budgeted, and original conversation rows remain in SQLite. Moving common rules before agent persona creates a byte-identical shared prefix for every participant in the same room. Prompt cache reuse is an **opportunity**, not a reduction in raw input token accounting.

## Cloudflare Workers AI affinity

For **first-party Cloudflare Workers AI** REST/compatible routes only, `continuity::prepare` derives an opaque, deterministic per-room/per-model affinity from session ID, provider ID, base URL and model. Every room participant requests the same backend instance with `x-session-affinity`; their `participant_conversation`, `transportSessionId`, and persona-specific continuation/cache keys remain disjoint. The header is not sent to arbitrary custom gateways, OpenAI, localhost or other Cloudflare products.

The Cloudflare Workers AI transport omits generic JSON `prompt_cache_key` in this mode, relying instead on the documented header. Other providers retain negotiated hint behavior.

- [Cloudflare Workers AI Prompt Caching](https://developers.cloudflare.com/workers-ai/features/prompt-caching/)
- [Cloudflare Agents conversation state / frozen system prompt](https://developers.cloudflare.com/agents/concepts/conversation-state-and-memory/)
- [GLM-4.7-Flash model page](https://developers.cloudflare.com/workers-ai/models/glm-4.7-flash/)

Cloudflare enables prefix caching for **select** models, not all models; its GLM-4.7-Flash public page currently lists standard input/output rates rather than a cached-token tier. The backend must not assume a cache hit because the affinity header was accepted. Native usage parsing now understands `prompt_tokens_details.cached_tokens`, `input_tokens_details.cached_tokens`, `prompt_cache_hit_tokens`, `cached_input_tokens`, and `cached_tokens`, and preserves a missing value as **null**, not zero.

## Regression evidence / next live run

Development CI was expanded to include frontend TypeScript checking and Rust tests for:
- Stop recorded before a late chat POST: no accidental user/assistant turn stored.
- Silent model HTTP request interrupted before first token; resulting execution is `cancelled`; same session succeeds on its next request.
- Session-specific tombstone and durable cancellation supervisor.
- Stable common room prefix and separate agent personas.
- Cloudflare Workers AI route allowlist, same room affinity, unrelated provider exclusion.
- Existing 9-agent, 3-turn group completion and input-context accounting.

These tests use mocked local Provider HTTP or native Rust fixtures. They do **not** measure cache efficiency on live Cloudflare GLM-4.7-Flash or infer paid cache hits from token totals. A meaningful follow-up reruns the same 9×3 benchmark, compares both answer quality and raw/uncached/cached input token counts, TTFT, full completion rate and cost.

The working frontend TypeScript check has passed in GitHub Actions. Rust regression result should be read from the latest successful code run; do not interpret an in-progress or superseded run as a pass.
