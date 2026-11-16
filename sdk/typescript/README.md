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
- Retired embedded media: `video.list()`, `video.get(id)`, `video.download(id)` and `video.outputs()` are historical read-only operations. Use MCP or connected providers to generate new media.
- `approvals.resolve`, `executions.cancel`, `jobs.cancel`, `sessions.cancel`, `rsi.campaigns` (historical read-only), and `agents.save` call the identical Rust handlers exposed in the UI.
- `evaluation.summary()` is the same native Rust scorecard served to the existing GUI Activity panel. It distinguishes independently verified native task success, failures and unverified cases, total tokens and duration, and configured-rate cost coverage. It does NOT claim per-task invoices.
- Provider pricing is set in the existing Model settings dialog as `pricing: {inputUsdPerMillion, outputUsdPerMillion, cachedInputUsdPerMillion?}`. Missing rates remain unpriced; cache hits are never subtracted from input tokens twice.
- `X-Dominion-Token` is passed on requests only to validated relative `/api/*` paths. Set `DOMINION_ADMIN_TOKEN` in the server environment for write authorization. Do not expose that token in browser code accessible to untrusted users.
- No automatic write retries, remote API keys, hosted vendor lock-in, or hidden token charges are included.

## Testing and scope

`npm test` builds TypeScript and runs mocked-fetch contract tests for endpoint matching, auth, SSE fragmentation, duplicate suppression, interruption and persistence polling. They **do not** establish a live model run, server CI pass, or external production reliability.

This TypeScript package is auxiliary; the embedded native Rust SDK in `src/sdk.rs` owns the engine and is the primary integration layer. `jobs.steer(id, instruction)` appends a binding instruction; `jobs.replaceGoal(id, instruction)` replaces the unfinished goal of the **same persisted job**. Both commit a revision and take effect at the next safe Rust model-turn boundary, not mid-tool-call. A replacement replans and resets old acceptance checks, retaining audit receipts and prior side-effect history. Explicit verification commands and tool permission policy remain in effect. `jobs.create({kind:'prompt', input:{prompt, autonomous:true, maxAutonomousAttempts:3, executionStrategy:'focused'}})` requests the native verifier/replan loop.

The initial SDK surface is explicit and versioned rather than pretending to cover every experimental backend endpoint; new operations should be mapped to their Rust handler and tested before publication.

2026 reference patterns reviewed: [Hermes architecture](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/architecture.md), [OpenAI Agents streaming lifecycle](https://openai.github.io/openai-agents-js/guides/streaming/), [MCP transport cancellation/reconnection](https://github.com/modelcontextprotocol/typescript-sdk/blob/main/packages/client/src/client/streamableHttp.ts). Implementation uses Dominion's own contract.
