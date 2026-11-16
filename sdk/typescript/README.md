# DOMINION TypeScript SDK

Standalone, typed client for DOMINION's actual native Rust engine and REST/SSE API. Install using the checked-out package:

    npm install ./sdk/typescript

Or run its independent tests:

    cd sdk/typescript
    npm install
    npm test

Usage:

    import {Dominion} from '@dominion-ai/sdk';
    const sdk=new Dominion({baseUrl:'http://127.0.0.1:4317'});
    const job=await sdk.jobs.create({kind:'prompt',input:{prompt:'Analyze my repository'}});
    console.log((await sdk.waitJob(job.id,{timeoutMs:120000})).status);

Durable events (same event bus as React):

    const stop=new AbortController();
    for await(const event of sdk.watchEvents({after:0,signal:stop.signal})) {
      console.log(event.seq,event.type);
      // Persist event.seq to resume on process restart.
    }

The SDK never replaces the Dominion engine or implements a second agent harness. The Rust engine manages model routing, tool policies, session memory, workflow execution, checkpoints, cancellation and RSI.

Contract: /api/* (NOT /api/v1/*), X-Dominion-Token optional admin header (NOT Bearer), POST /api/chat as SSE, GET /api/events?after=N with durable replay. Browser access inherits the server's same-origin policy. API routes are grounded in src/api/mod.rs, src/api/jobs.rs, src/api/system.rs and src/api/video.rs. Tests cover auth, path mapping, SSE fragmentation, reconnection, chat interruption and job completion.

An SDK client needs a running local DOMINION server; it does not bundle a model, and it never holds vendor tokens. HTTP tests mock the server; a real-provider integration test requires explicitly configured credentials.

Mid-run owner steering is `sdk.jobs.steer(jobId, instruction)`, which invokes the Rust kernel via `POST /api/jobs/:id/directive`. The JS client never executes native tools or owns task states.

## Native job token and prompt-cache usage (2026-11-15)

`Dominion.jobs.get(id)` returns the Rust-owned `Job.nativeModelUsage?: NativeModelUsage` field when the native provider has settled calls. `inputTokens`/`outputTokens` and `cacheReadTokens`/`cacheWriteTokens` are measured provider-reported totals; `reportedCalls` and `unreportedCalls` distinguish known and unknown bills. `nativeState.chargedTokens` remains the independent Rust SDK planner/budget metric and may estimate usage when the provider omits it. The TypeScript client does not retry an unknown-billing Codex request or run its own agent. The actual Rust REST/GUI integration and [2026 focused tests](https://github.com/hushmind3/dominion-rust/actions/runs/43691823640) passed with 115 Rust cases, 7 standalone SDK cases, frontend typecheck/build. See [provider metering architecture](../../docs/SUB3-NATIVE-METERING-2026-11-15.md).

## Native owner-controlled quality-cost routing

A `jobs.create({kind:'prompt',input:{prompt,routingMode:'quality_cost',minQuality:0.8,maxQualityDrop:0.1}})` opt-in goes through the Rust owner API; do not set `providerId` if you want automatic selection. The real Rust job includes `nativeRoute?:NativeRouteSelection` with the pinned model, selection reason, estimated per-turn owner-configured KRW cost and a nonsecret model-contract fingerprint. `nativeModelUsage.configuredCostKRW` is provider-reported usage multiplied by pinned **owner-entered** rates; `unpricedCalls` identifies missing measurements/tariffs. It is not an invoice. Register a provider's `nativeRouting: { model, quality, pricesKRWPerMillion: { input,output,cachedInput?,cacheWrite? } }` first via `providers.save`. Full design: [native Rust quality-cost routing](../../docs/SUB3-NATIVE-QUALITY-COST-ROUTING-2026-11-15.md).

## Native autonomous run controls

The TypeScript SDK and the React autonomous-work panel use the **same** `/api/sdk/runs` routes, backed by the persisted Rust job and tool-approval engine:

```ts
const run = await sdk.nativeRuns.start({
  instruction: 'Inspect and repair the repository',
  verifyCommand: 'cargo test',
  maxRounds: 24,
});
const current = await sdk.nativeRuns.get(run.id);
if (current.status === 'awaiting_approval' && current.checkpoint?.pendingCall) {
  // Present the tool name and arguments for human review before approval:
  // await sdk.nativeRuns.approve(run.id, current.checkpoint.pendingCall.id);
}
```

The API also exposes `list()`, `cancel(id)`, `resume(id)`, and
`reconcile(id,{applied,details})` for uncertain effects requiring an
operator's recorded observation. Server responses are authoritative:
the SDK does not fabricate progress, retry ambiguous side effects or
run a client-side agent loop.

## Video project lifecycle (2026-11-16)

The first-party SDK and production GUI call the same Rust-owned project routes. The SDK does **not** run a parallel video workflow:

```ts
const project = await sdk.video.createProject({
  brief: 'A 30-second guide', engine: 'native', autoRender: false
});
const current = await sdk.video.project(project.id);
// Once current.status === 'planned':
await sdk.video.editScenes(project.id, current.revision, current.scenes);
await sdk.video.renderProject(project.id, current.revision + 1);
```

Use `sdk.video.projects()` for durable project history and `sdk.video.retryPlan(id,revision)` to explicitly retry failed planning. When a paid planning request ended with an unknown result, the server requires `sdk.video.retryPlan(id,revision,true)` as the owner's acknowledgement of possible duplicate cost. Similarly, `sdk.video.renderProject(id,revision,true)` is required to request a fresh render after an uncertain remote video job. Verified completed video clips from earlier failed runs are reused by the Rust kernel. Read the returned project revision before each mutation; HTTP 409 signals a concurrent edit.

These methods wrap `/api/video/projects*`; they do not bypass Rust validation, persistence, approval, or billing boundaries.
