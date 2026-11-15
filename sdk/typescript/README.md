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
