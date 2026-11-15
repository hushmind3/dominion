# Sub3 — Job-scoped Codex WebSocket continuity (2026-11-15)

## Objective and actual product path

Dominion owns agent planning, tool dispatch, verification, authorization and persistent receipts in the independent Rust kernel. No external agent harness or vendor agent SDK is embedded.

```text
NativeAgentPanel / TypeScript SDK
  → POST /api/jobs
  → jobs::run_prompt
  → native_agent::run
  → dominion_sdk::Kernel::{run,resume}
  → ConfiguredModel::complete_with_tools
  → native_model::complete_with_run
  → native_codex_ws::generate (Codex, native /api/jobs only)
  → native_stream::{Assembler,Wire::Codex}
  → native_model::parse_codex → typed SDK Decision
  → scheduler / approvals / Tools::execute
  → host-verified durable Receipt → next model round
```

The existing interactive chat WebSocket in `src/engine/chat/codex_ws.rs` is a separate application route. Native jobs now use their own Rust job-scoped, connection-local Responses chain, not an interactive-session adapter or a second runner.

## Session contract

- The chain key separates **job ID**, provider ID, model, base URL and account. A per-job async lock serializes requests. Live connections are recycled at 50 minutes, with idle expiry and a maximum registry size. `native_agent::run` drops its ephemeral socket when the job exits, including early kernel returns.
- The first turn sends `codex_input(current_prompt, durable_receipts)`. A successfully settled response records its ID, the list of provider-issued function-call IDs, a fingerprint of the system/tool contract and the prefix of exact durable receipt fingerprints.
- A subsequent turn sends only the **new host-confirmed `function_call_output` items** plus the current owner/task-state prompt using `previous_response_id`. Incremental continuation requires every pending provider call to match exactly one new durable receipt and all earlier receipt fingerprints and the model/tool/system contract to match.
- On edited history, changed owner-level system instructions, missing/extra results, provider change, socket age expiration or process restart, the client starts a full **canonical** request without `previous_response_id`. It never treats model-produced text as an executed tool result.
- When the server explicitly returns `previous_response_not_found` for a delta request, it opens a fresh socket and replays canonical full context once. An unconfirmed send, silent socket drop, mid-stream interruption or terminal response without ID is **not** replayed over WebSocket, HTTP or another model. `native.codex_ws.uncertain` records unknown billing and `replayed:false`; the SDK's `model_in_flight` checkpoint retains `needs_review` rather than automatically paying twice.
- Job cancellation is polled during network silence, preserving pending text/tool labels as **non-executable** and the Rust journal as sole execution authority.

## Long-context and cost controls

- Before every native model call, `native_model::fit_native_context` applies the configured provider window and reserves output capacity. It can omit only the oldest complete tool-call/result pairs. The owner instruction, current task, tool schema and durable receipt history remain intact.
- A live WebSocket's retained model context may continue growing despite short wire deltas. The transport watches **provider-reported input/output token usage**, and when the estimated next turn plus output reservation approaches 90% of a configured model context, it starts a new canonical chain. The record `native.codex_ws.context_rebase` explains the decision.
- `native.codex_ws.turn` records delta/full mode, number of new tool receipts, estimated wire-input tokens, canonical-input estimate and provider-reported tokens. A smaller WebSocket payload does **not** establish smaller billable model context; `measuredSavings:false` makes that distinction explicit.
- A response omitting `total_tokens` but reporting `input_tokens` and `output_tokens` is accounted with their sum by the native usage ledger. Failed or ambiguous requests remain unbilled in the local exact-token ledger rather than being misclassified as zero-cost successes.

## Verification scope

Native mock WebSocket fixtures exercise: a genuine two-round function-call/output continuation on one socket; full-context replay after explicit `previous_response_not_found`; no retry after EOF without a delta; missing terminal ID; cancelled stalled socket; receipt-prefix and context-rebase rules. The focused workflow separately runs SDK, native Rust, interactive Codex socket, workflow recovery, TypeScript typecheck and Vite build.

This is loopback protocol integration, **not** a paid ChatGPT-subscription credential test, a measured reduction in provider-billed tokens or a full autonomous benchmark. No full CI or SWE-bench parity is claimed.

## Reference design

- [OpenAI Responses WebSocket mode (2026)](https://developers.openai.com/api/docs/guides/websocket-mode): connection-local `previous_response_id`, 60-minute lifetime, store=false recovery, chain forks.
- [OpenAI conversation state](https://developers.openai.com/api/docs/guides/conversation-state): continued response context and billable input tokens.
- [Codex native model client](https://github.com/openai/codex/blob/main/codex-rs/core/src/client.rs): session/turn-scoped WebSocket ownership and continuation. Dominion independently implements its own job chain and does not invoke this external client.

## Executed focused verification (2026-11-15)

[GitHub Actions run 43690699582](https://github.com/hushmind3/dominion-rust/actions/runs/43690699582), code commit `50768a5f395ecbff0ef70d1b69d6fe27ffb0f2b5`: completed successfully. The log contains 45 Rust SDK cases, 51 filtered native host/transport cases, 15 separately filtered interactive Codex socket cases, and one workflow recovery case: **112 passed, 0 failed**. Frontend TypeScript typecheck and Vite production build also passed. Native WS fixtures include canonical tool-output continuation, explicit missing-previous replay, partial/unknown delivery, job cancellation both before send and during a stalled receive, unknown terminal ID, and context rebase selection. No commercial model token-cost claim or full release CI result is inferred from these fixtures.
