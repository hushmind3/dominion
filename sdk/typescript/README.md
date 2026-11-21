## Shared Rust tool discovery contract (sub2, 2026-11-18)

The SDK calls the same chat/prompt-job Rust turn engine as the GUI. For large available native/MCP tool catalogs, the engine may advertise `dominion_tool_search` alongside a bounded set of relevant real tool schemas. After a successful search receipt, subsequent model rounds can invoke newly promoted tools by their **original** names. The TypeScript SDK does not implement its own search, tool executor or authorization, and no new public SDK endpoint is required. Unadvertised model tool names are rejected before native dispatch. Explicit user-selected core/filtered strategies and single-file shortcuts keep their previous tool inventory. See [Rust code and verification scope](../../docs/SUB2-PROGRESSIVE-NATIVE-TOOLS-2026-11-18.md).

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
- `approvals.resolve`, `executions.cancel`, `jobs.cancel`, `sessions.cancel`, and `agents.save` call the same Rust handlers exposed in the UI.
- `evaluation.summary()` is the same native Rust scorecard served to the existing GUI Activity panel. It distinguishes independently verified native task success, failures and unverified cases, total tokens and duration, and configured-rate cost coverage. It does NOT claim per-task invoices.
- Provider pricing is set in the existing Model settings dialog as `pricing: {inputUsdPerMillion, outputUsdPerMillion, cachedInputUsdPerMillion?}`. Missing rates remain unpriced; cache hits are never subtracted from input tokens twice.
- `X-Dominion-Token` is passed on requests only to validated relative `/api/*` paths. Set `DOMINION_ADMIN_TOKEN` in the server environment for write authorization. Do not expose that token in browser code accessible to untrusted users.
- No automatic write retries, remote API keys, hosted vendor lock-in, or hidden token charges are included.

## Testing and scope

`npm test` builds TypeScript and runs mocked-fetch contract tests for endpoint matching, auth, SSE fragmentation, duplicate suppression, interruption and persistence polling. They **do not** establish a live model run, server CI pass, or external production reliability.

This TypeScript package is auxiliary; the embedded native Rust SDK in `src/sdk.rs` owns the engine and is the primary integration layer. `jobs.steer(id, instruction)` appends a binding instruction; `jobs.replaceGoal(id, instruction)` replaces the unfinished goal of the **same persisted job**. Both commit a revision and take effect at the next safe Rust model-turn boundary, not mid-tool-call. A replacement replans and resets old acceptance checks, retaining audit receipts and prior side-effect history. Explicit verification commands and tool permission policy remain in effect. `jobs.create({kind:'prompt', input:{prompt, autonomous:true, maxAutonomousAttempts:3, executionStrategy:'focused'}})` requests the native verifier/replan loop.

The initial SDK surface is explicit and versioned rather than pretending to cover every experimental backend endpoint; new operations should be mapped to their Rust handler and tested before publication.

2026 reference patterns reviewed: [Hermes architecture](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/architecture.md), [OpenAI Agents streaming lifecycle](https://openai.github.io/openai-agents-js/guides/streaming/), [MCP transport cancellation/reconnection](https://github.com/modelcontextprotocol/typescript-sdk/blob/main/packages/client/src/client/streamableHttp.ts). Implementation uses Dominion's own contract.

## Typed run lifecycle subscription (sub2, 2026-11-20)

`client.runEvents({after:lastSeq})` filters the existing durable SSE feed to versioned `run_state` events originating in actual persisted `execution`, `job` or `room_work_assignment` records. Events include `runId`, `runKind`, `state`, `agentId`, nullable `sourceMessageId` and monotonic `eventSeq===seq`. The same `/api/events` endpoint and reconnect logic are reused. Unknown schema versions, bad state names and mismatched journal cursors are rejected instead of being presented as trustworthy lifecycle updates. This method does not create a new runtime or execute any tools.

## Direct Chat and file attachments (sub2, 2026-11-22)

The SDK supports plain conversation without creating an agent persona. Set
`conversationMode:'chat'` explicitly in `sessions.create` and `streamChat`.
The Rust engine then disables native tools, automatic learning and inherited
session agent identities even if a stale client asks for them. Omit the mode
on older agent/SDK work to preserve its existing execution contract.

```ts
const session = await dominion.sessions.create({
  title:'Document review',conversationMode:'chat',providerId:'my-provider'
});
const file = await dominion.attachments.upload({
  sessionId:session.id,name:'notes.txt',mimeType:'text/plain',
  data:Buffer.from('Hello world','utf8').toString('base64'),
});
await dominion.streamChat({
  sessionId:session.id,conversationMode:'chat',
  input:'Summarize the attached document',attachmentIds:[file.id],
  toolsEnabled:false,toolsExecutionEnabled:false
},event=>console.log(event));
```

Attachment records are scoped to a real session or room; Rust enforces
per-file (8 MiB), per-turn (8 files / 24 MiB) limits and validates image bytes.
Large text files are stored intact. For model input, the current follow-up
question selects bounded excerpts with original line/column locations, plus
the document start and end; a summary or quote from an omitted section is
not treated as verified. Base64 image payloads are accounted for at encoded
size rather than raw-file size.

`sessions.delete(sessionId)` deletes an idle **private Chat** and revokes its
stored attachment bytes and metadata. Rust rejects deletion while a job/run is
active or when the transcript belongs to a Room or an external channel.

`attachments.content(id)` reads bytes with the same authenticated SDK client.
Chat POST requests are never automatically retried.
To stop an active streamed Chat turn, supply a fresh UUID as `requestId` to
`streamChat`, call `sessions.cancelTurn(sessionId, requestId)`, and poll
`sessions.cancelStatus(sessionId)` until `stopped` is `true` before
considering the Rust execution and its durable journal released. This is a
**session-wide Stop**; the UUID also tombstones that exact delayed POST so it
cannot execute after cancellation. The initial cancellation acknowledgement
alone does not prove that model/tool execution has settled. Never automatically
retry a POST whose completion is uncertain.

When Rust emits an in-band `error` SSE event, `streamChat` first delivers the
event to the callback and then rejects with `ChatTurnFailedError`, preserving
the original `.event` for diagnostics. Rust may append `done` after the error;
this terminator never upgrades a failed turn into success. The
`assistant_saved` event records a persisted final answer.


## Optional public web lookup in ordinary Chat

Set `conversationMode:'chat'` and `webSearch:true` to allow a direct
conversation to consult public sources. The browser and SDK keep
`toolsEnabled:false` and `toolsExecutionEnabled:false`; the Rust runtime
grants only `web_search` and `http_fetch`. No file, shell, memory-writing,
MCP or agent execution capabilities are exposed. Search availability depends
on the network and the selected model's native/compatible tool calling.
Omit webSearch for fast no-tool conversations.
