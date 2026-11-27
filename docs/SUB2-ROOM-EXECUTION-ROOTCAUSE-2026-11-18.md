# Sub2: room execution path and provider input-cost evidence (2026-11-18)

Scope: `branch2` only. No model/provider swap, new execution runner, new database table, UI replacement or change to default tool permissions.

## First live benchmark signal

The owner's supplied benchmark report records ordinary agent tasks `3/3` completed (mean `19.4s`), but a 3-person group dialogue was reported `cancelled` after roughly `2s`. The raw benchmark ZIP and its request/response trace are **not available in this development session**, so this work distinguishes source-proven defects from that reported observation rather than attributing an exact user benchmark incident without the raw job row.

## Root cause reproduced by native regression test

`src/engine/rooms.rs::run_locked` attempted to persist a derived group `bound_input` by passing:

```rust
context.checkpoint(json!({"input": bound_input}))?;
```

`src/engine/jobs.rs::JobContext::checkpoint` uses `Store::checkpoint_running_job`, which **explicitly rejects any patch containing `input`**. A room run could therefore fail before reaching `run_room_quiet` and the provider request. A new mocked-provider regression (`engine::rooms::tests::fresh_owner_job_after_stop_uses_new_epoch_and_gets_a_real_reply`) surfaced the native checkpoint rejection, not a synthetic stream failure.

### Repair

- Keep queued `job.input` immutable: preserve the owner's message, selected agents and original permissions/snapshot; do **not** relax the job CAS guard in the DB.
- Write derived session generation, source ID and root task revision to `roomBinding`, a separate, host-owned durable job field. For older compatibility requests that did not include a frozen room snapshot, preserve one in `roomBinding.roomSnapshot`; do not duplicate the original API snapshot when already present.
- Existing `roomState` continues to own pending speaker queue, `inFlight`, outputs, and usage baseline. The resumed run consults immutable `input.roomSnapshot` or the legacy fallback `roomBinding.roomSnapshot`.
- Separate cancellation outcomes: `job_cancel_requested`, `session_epoch_changed` and `session_cancelled`. A failure to persist the session resume at an unchanged generation is a storage error, not a user cancellation. An older stopped job cannot replay under a fresh epoch.
- Keep the existing cancellation supervisor, shared chat runtime, native Rust tool dispatcher, approvals and SQLite journal unchanged.

## Per-speaker token and model/tool usage provenance

`src/engine/chat/room_context.rs::wire_profile` attributes the exact **prepared native input envelope** before each provider call to these components (all **estimates**): `systemAndPersona`, `sharedRoomState`, `ownerState`, `retrievedEvidence`, `toolResults`, `peerAndSelfHistory`, `ownerAndHistory`, `toolSchemas`, and serialization overhead.

`src/engine/chat/runtime/turn_body.rs` persists numeric profiles as `execution.inputContext.wireRounds` (up to 24 rounds). This is the *pre-adapter* prompt, not a claim to know every provider's private chat template. The actual provider prompt/response/cache token counts, with `estimated` and cache reporting flags, remain in `message.metadata.usage` and provider usage records. Auxiliary model calls are reflected in `execution.modelCalls`, while native tool execution is counted in `execution.toolCalls`.

`src/engine/rooms.rs` attaches `executionId`, `usage`, `modelCalls`, `toolCallCount`, and the first `inputContext` round profile to each group's `outputs[]`; full per-round detail stays on the indexed `execution` row and is exposed by the existing `/api/rooms/:id/state` endpoint. No raw system prompts, owner secrets, model tool results or provider credentials are added to the new telemetry.

The profile is an estimator; do **not** sum these estimates and call the result actual billed input tokens. `cachedPromptTokens: null` means the provider did not report a usable cache figure, not zero cached tokens.

## CI scope

The development CI on the sub2 branch runs:

```text
engine::rooms::tests::fresh_owner_job_after_stop_uses_new_epoch_and_gets_a_real_reply
engine::rooms::tests::nine_participants_share_one_owner_message_and_all_speak
engine::chat::room_context::tests::wire_profile_attributes_selected_messages_without_retaining_raw_text
```

The mock provider exercises the actual Rust chat streaming path, frozen job checkpoint, 9-person speaker loop, and the persisted per-speaker output. It does **not** reproduce the real user's 3-person Qwen/Cloudflare cost measurements or prove the revised group success rate in a new live benchmark.

## Next benchmark evidence to preserve

For the same-model 3-person dialogue: source request and benchmark job ID; submitted and resumed cancellation generations; job status and `pauseReason`; each speaker's `executionId`; `execution.modelCalls`, tool operations and receipts; `message.metadata.usage` including cache reporting; `execution.inputContext.wireRounds`; and completion evidence. The runner should include these existing DB/API records in its ZIP, not only a startup `server.log`.

All other branches and the main frontend remain untouched.
