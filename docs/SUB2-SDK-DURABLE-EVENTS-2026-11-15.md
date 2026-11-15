# Sub2 — Rust Engine, Optional TypeScript Client and Durable Event Recovery (2026-11-15)

## Product principle
The optional TypeScript SDK and the main-branch React GUI use **independent** HTTP/SSE client implementations but the **same** Rust/Axum `/api/*` server contract; no forked Node agent, third-party SDK dependency or invented `/api/v1` runtime. User requests still enter the existing `src/api/*` handlers and are executed by `src/engine/*`.

```text
External Node app ─► sdk/typescript/src/index.ts ─┐
                                                    ├─► Rust /api/* ─► engine ─► tools/model/memory/SQLite
React web UI ─────► web/src/api.ts ────────────────┘
                           │
                  main-branch UI request / streamChat
```

## Entrypoints and data paths

| Operation | SDK / GUI path | Rust handler and persistence |
| --- | --- | --- |
| Work submission | `jobs.create({kind:'prompt',input})` | `POST /api/jobs` → `api/jobs.rs::create` → `engine/jobs.rs::enqueue` → SQLite `job` |
| Long task progress | `jobs.get`, `waitForJob`, `events({after})` | `api/jobs.rs::one`; `api/system.rs::events` → `events.rs::Bus` → SQLite `event_log` |
| Streaming chat | `streamChat(..., signal)` | `POST /api/chat` → `api/chat.rs::chat` → `engine::chat::run` |
| Group collaboration | `rooms.send(id,{input,clientMessageId})` | `POST /api/rooms/:id/messages` → `api/rooms.rs::submit_internal` → persisted message + idempotent job |
| User approvals | `approvals.resolve` | `POST /api/approvals` → `api/approvals.rs::resolve_approval` → engine waiter + persisted decision |
| Video | `video.create`, `video.get`, `video.download` | `/api/video/jobs`, `/content` → `api/video.rs` → `engine/video.rs` |
| RSI | `rsi.campaigns`, `rsi.campaign` | `/api/rsi/campaigns` → `api/rsi.rs` → existing research engine |

## Actual defects addressed
1. **One-page event replay gap**: prior `/api/events` read 500 historical entries then jumped to live events. `system::durable_replay` drains paginated durable history and recovers a lagging live stream. The bounded retention (roughly 2,000 events, pruning periodically) is preserved.
2. **Silent lost history**: if an old cursor predates retained events, `event_gap` is generated with `resyncRequired=true`, the missed sequence count, and the first retained sequence. Clients can refresh canonical job/room/session state instead of assuming the missing events happened.
3. **Concurrent bus ordering**: the old atomic counter alone did not serialize the sequence -> SQLite append -> broadcast pipeline. An `Arc<Mutex<()>>` on `Bus` now guarantees monotonic order among clones.
4. **Two incompatible SDK contracts**: prototype sub4 used `/api/v1` and bearer auth; the integrated Rust service uses `/api` and `X-Dominion-Token`. New SDK matches the actual production server. The React GUI keeps the original main-branch request/auth and SSE implementation, without importing the SDK.
5. **Unsafe chat retry**: chat POST streams are not automatically replayed. A lost stream throws `StreamInterruptedError`; durable room messages accept a stable `clientMessageId` when the caller deliberately retries.

## Evaluation / evidence
- Rust unit cases added to `src/events.rs` (8 concurrent emitters × 80 events) and `src/api/system.rs` (1,100-event paginated replay, expired cursor over 2,600 writes).
- TypeScript tests in `sdk/typescript/tests/sdk.test.mjs` cover routing, auth, denied paths, provider errors, UTF-8 split SSE, no chat retry, cursor resumption, job polling and abort.
- A local preliminary TypeScript test prototype passed 9/9. The exact published head has **not** been exercised by CI or full Rust test run in this session; Rust integration and live-model reliability remain to be established. No benchmark score or savings are asserted.
- CI completion, real-provider E2E, sustained event soak and video output quality measurements remain follow-up checks; they are not simulated by mock tests.

## UI / SDK compatibility
The `web/` subtree is an exact copy of the main branch. Its `web/src/api.ts` retains the original GUI exports (`request`, `get`, `post`, `del`, `streamChat`, `setAuthToken`) and directly calls the Rust API; it does **not** import `@dominion/sdk`. The standalone SDK continues to use the same `/api/*` contract for external clients and can be built independently using `npm run build --prefix sdk/typescript`.

## 2026 implementation references
Patterns were compared with the published [OpenAI Agents JS streaming lifecycle](https://openai.github.io/openai-agents-js/guides/streaming/), [Hermes code/architecture](https://github.com/NousResearch/hermes-agent) and [MCP TypeScript transport](https://github.com/modelcontextprotocol/typescript-sdk). Those projects inform ideas; no foreign agent harness or API policy is imported into DOMINION.

## Native engine evolution — 2026-11-15

The primary public SDK is Rust (`src/lib.rs`, `src/sdk.rs`); TypeScript is an optional /api client. The desktop and Rust consumers boot the same runtime. `src/engine/autonomy.rs` runs bounded evidence-driven repair cycles for ordinary prompt jobs; the specialized detached RSI verifier remains separate. `src/engine/jobs.rs::steer` persists owner instructions for the next model boundary; `src/api/jobs.rs` adapts the same engine method. The unchanged main GUI exposes its existing execution/activity views; step receipts, revisioned steering and usage deltas remain available through the Rust SDK and job API, not additional sub2 GUI controls. See [frontier comparison](FRONTIER-COMPARISON-SUB2-2026-11-15.md).
