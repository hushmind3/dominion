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

- `jobs.create` returns the Rust-persisted job; `jobs.get` and `waitForJob` read it after an SDK or GUI restart. The SDK does not claim to resume an incomplete task on its own.
- `streamChat(input, onEvent, {signal})` uses the same `POST /api/chat` stream as the GUI. A dropped chat stream is **never automatically replayed**, since replay could duplicate paid model calls, file edits or external actions.
- `events({after, signal, maxReconnects})` listens to the durable `GET /api/events` bus, deduplicating by `seq` and reconnecting with `after` / `Last-Event-ID`. Consumers should persist the last processed `seq` to resume across process restarts; feed replay is bounded by the server's own retention contract.
- `rooms.send(roomId, {input, clientMessageId})` uses the engine's existing idempotent room-submission path. Supply a stable `clientMessageId` if you explicitly retry a room post.
- `approvals.resolve`, `executions.cancel`, `jobs.cancel`, `sessions.cancel`, `video.create`, `rsi.campaigns` and `agents.save` call the identical Rust handlers exposed in the UI.
- `X-Dominion-Token` is passed on requests only to validated relative `/api/*` paths. Set `DOMINION_ADMIN_TOKEN` in the server environment for write authorization. Do not expose that token in browser code accessible to untrusted users.
- No automatic write retries, remote API keys, hosted vendor lock-in, or hidden token charges are included.

## Testing and scope

`npm test` builds TypeScript and runs mocked-fetch contract tests for endpoint matching, auth, SSE fragmentation, duplicate suppression, interruption and persistence polling. They **do not** establish a live model run, server CI pass, or external production reliability.

The initial SDK surface is explicit and versioned rather than pretending to cover every experimental backend endpoint; new operations should be mapped to their Rust handler and tested before publication.

2026 reference patterns reviewed: [Hermes architecture](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/architecture.md), [OpenAI Agents streaming lifecycle](https://openai.github.io/openai-agents-js/guides/streaming/), [MCP transport cancellation/reconnection](https://github.com/modelcontextprotocol/typescript-sdk/blob/main/packages/client/src/client/streamableHttp.ts). Implementation uses Dominion's own contract.
