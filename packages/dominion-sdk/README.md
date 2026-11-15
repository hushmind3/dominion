# @dominion-ai/sdk

TypeScript client for the **DOMINION-owned Rust engine**, not a wrapper around an external agent runtime. Runs on Node.js 20+ or a browser that supports fetch and readable streams.

## Build and check

```bash
cd packages/dominion-sdk
npm install
npm test
```

## Use

```ts
import { DominionClient } from '@dominion-ai/sdk'

const client = new DominionClient({
  baseUrl: 'http://127.0.0.1:4317',
  // token: process.env.DOMINION_ADMIN_TOKEN,
})

const { goal, job } = await client.createGoal({
  goal: 'Create report.md and verify the result',
  executor: { agentId: 'my-agent' },
  maxIterations: 6,
  successCriteria: [
    { type: 'file_exists', path: 'report.md' },
    { type: 'file_contains', path: 'report.md', contains: 'Results' },
  ],
})
console.log(goal.id, job.id) // queued, not completed

// Resume progress after disconnect using an explicit event sequence cursor.
for await (const event of client.events({ after: 0 })) {
  if (event.type === 'job_event' && event.jobId === job.id) {
    console.log(event)
  }
}
```

The Rust server owns all planning, tool execution, permissions, job checkpoints, evaluation and state transitions. The SDK only transports requests and structured events. Creating a goal returns **HTTP 202** with a queued job; inspect `client.job(job.id)` and `client.goal(goal.id)` to learn the actual outcome. `client.chat(input, signal)` is an async iterable of Rust SSE events.

The SDK does not create a session implicitly, retry non-idempotent mutations, grant tool permissions, or claim that a streamed draft is complete. It supports `AbortSignal`. Stream interruption before a terminal `done` is an error; `/api/events` uses an explicit `after` sequence for reconnect.

Admin tokens can be supplied to the SDK for local HTTP or remote HTTPS. The Rust server currently binds to loopback and restricts browser origins; this package does not turn the backend into an internet-facing service.
