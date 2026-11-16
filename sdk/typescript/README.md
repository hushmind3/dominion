# @dominion/sdk — TypeScript / JavaScript

This is the external client for the **same native Rust/Axum API used by the React GUI**. It does not embed a second harness, forward prompts to a third-party agent, or expose an incompatible `/api/v1` contract.

## Build and use

```bash
cd sdk/typescript
npm install
npm test
```

```ts
import { Dominion } from '@dominion/sdk';

const dominion = new Dominion({
  baseUrl: 'http://127.0.0.1:4317',
  // token: process.env.DOMINION_ADMIN_TOKEN,
});

const job = await dominion.jobs.create({
  kind: 'prompt',
  input: { prompt: 'Inspect the repository and propose a fix', toolsEnabled: true },
});
const result = await dominion.waitForJob(job.id, { timeoutMs: 120_000 });
console.log(result.status, result.result);
```

All operations use **real** `/api/*` endpoints. The Rust engine performs planning, model/tool calls, logging, approvals, checkpointing, cancellation and recovery. SDK requests do not create fake execution state.

## Important contracts

- `jobs.create({kind:'prompt', input:{prompt:'Fix this project', workspaceDir:'/absolute/project/path'}})` scopes an individual job to that existing project rather than the server startup folder. The Rust engine validates the directory at submission and again at execution. The embeddable Rust SDK uses `submit_in_workspace(prompt,path)`.
- `input.tokenBudget` optionally limits the job's subsequent provider calls based on recorded session usage plus the next call's estimated input tokens, and bounds output allocation; accepted values are 1,024–2,000,000. Final provider-billed usage can differ from the admission estimate. A finished job does not leave the cap on later user chat in the same session.
- The main GUI's **resume** operation can also retry a provider-only failure if the Rust engine finds no persisted tool operations for that attempt. Any possible tool side effect still requires the main UI's owner reconciliation; its prior observation is preserved as `resumeEvidence`, reuses the original durable session and instructs the Rust agent to re-observe before taking further actions. Historical execution receipts are retained in bounded `resumeHistory`; a user attestation is not mislabeled as engine verification.
- `waitForJob` recognizes `needs_review` and `interrupted` as terminal job states. A user intervention or exhausted model-call budget must not make external SDK clients poll forever.
- `jobs.create` returns the Rust-persisted job; `jobs.get` and `waitForJob` read it after an SDK or GUI restart. The SDK does not claim to resume an incomplete task on its own.
- `streamChat(input, onEvent, {signal})` uses the same `POST /api/chat` stream as the GUI. A dropped chat stream is **never automatically replayed**, since replay could duplicate paid model calls, file edits or external actions.
- `events({after, signal, maxReconnects})` listens to the durable `GET /api/events` bus, deduplicating by `seq` and reconnecting with `after` / `Last-Event-ID`. Consumers should persist the last processed `seq` to resume across process restarts; feed replay is bounded by the server's own retention contract.
- `rooms.send(roomId, {input, clientMessageId})` uses the engine's existing idempotent room-submission path. Supply a stable `clientMessageId` if you explicitly retry a room post.
- `video.create({brief, autoStoryboard:true, narrationScript?})` uses the same native Rust video runner as the GUI. AI planning can return scene visuals and voice lines in one provider call; when a custom narration is supplied, it overrides the generated voice script. Actual MP4 captions require FFmpeg; OS TTS is optional and reports its outcome rather than claiming an audio track that was not written.
- `video.upload(videoJobId, {title,privacyStatus:'private'})` calls the existing explicit YouTube upload queue. No automatic public post is triggered by video completion.
- The embeddable Rust SDK also provides `submit_video`, `video_job`, `cancel_video`, and `video_output_path`, all operating on the same durable Rust engine; there is no shadow JS renderer.
- `video.cancel(videoJobId)` stops the exact Rust video job and returns `{job,remoteCancel}`. `remoteCancel.status=requested` means the scoped ComfyUI server acknowledged the cancellation request; `unconfirmed` means remote GPU work may remain active. No global renderer interruption or implicit retry is performed. See [Sub2 cancellation and checkpoint contract](../../docs/SUB2-VIDEO-CANCEL-CHECKPOINT-2026-11-16.md).
- `approvals.resolve`, `executions.cancel`, `jobs.cancel`, `sessions.cancel`, `video.create`, `rsi.campaigns` and `agents.save` call the identical Rust handlers exposed in the UI.
- RSI uses the **native Rust worktree/evaluator**, not an SDK-side improver. `rsi.list()` returns `{version,candidates,campaigns,minPairedCases}`; `rsi.campaigns()` returns `{campaigns:[...]}` (both are envelopes, not arrays). `rsi.start({providerId,model,mode:'continuous',confirmation:'START_RSI_EVOLUTION'})` invokes Rust source/branch/budget validation. `rsi.verify(id)` replays independent measurements; `rsi.choose(id,attempt,'candidate')` requires the current eligible comparison; and `rsi.applyVerified(id,commit)` sends the exact verified-commit confirmation to the Rust adoption gate. No client-side bypass, model training, or automatic deployment is added.
- `evaluation.summary()` is the same native Rust scorecard served to the existing GUI Activity panel. It distinguishes independently verified native task success, failures and unverified cases, total tokens and duration, and configured-rate cost coverage. It does NOT claim per-task invoices.
- Provider pricing is set in the existing Model settings dialog as `pricing: {inputUsdPerMillion, outputUsdPerMillion, cachedInputUsdPerMillion?}`. Missing rates remain unpriced; cache hits are never subtracted from input tokens twice.
- `X-Dominion-Token` is passed on requests only to validated relative `/api/*` paths. Set `DOMINION_ADMIN_TOKEN` in the server environment for write authorization. Do not expose that token in browser code accessible to untrusted users.
- No automatic write retries, remote API keys, hosted vendor lock-in, or hidden token charges are included.

## Testing and scope

`npm test` builds TypeScript and runs mocked-fetch contract tests for endpoint matching, auth, SSE fragmentation, duplicate suppression, interruption and persistence polling. They **do not** establish a live model run, server CI pass, or external production reliability.

This TypeScript package is auxiliary; the embedded native Rust SDK in `src/sdk.rs` owns the engine and is the primary integration layer. `jobs.steer(id, instruction)` appends a binding instruction; `jobs.replaceGoal(id, instruction)` replaces the unfinished goal of the **same persisted job**. Both commit a revision and take effect at the next safe Rust model-turn boundary, not mid-tool-call. A replacement replans and resets old acceptance checks, retaining audit receipts and prior side-effect history. Explicit verification commands and tool permission policy remain in effect. `jobs.create({kind:'prompt', input:{prompt, autonomous:true, maxAutonomousAttempts:3, executionStrategy:'focused'}})` requests the native verifier/replan loop.

The initial SDK surface is explicit and versioned rather than pretending to cover every experimental backend endpoint; new operations should be mapped to their Rust handler and tested before publication.

2026 reference patterns reviewed: [Hermes architecture](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/architecture.md), [OpenAI Agents streaming lifecycle](https://openai.github.io/openai-agents-js/guides/streaming/), [MCP transport cancellation/reconnection](https://github.com/modelcontextprotocol/typescript-sdk/blob/main/packages/client/src/client/streamableHttp.ts). Implementation uses Dominion's own contract.
