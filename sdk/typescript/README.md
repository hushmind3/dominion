# Dominion TypeScript client

The client is an HTTP/SSE adapter to the **same native Rust engine** used by the browser interface. No vendor agent driver or secondary policy loop runs in JavaScript.

```js
import {DominionClient} from "@dominion/native-client";
const client = new DominionClient({baseUrl:"http://127.0.0.1:4317"});
const task = await client.submit({
  requestId:"my-unique-request-2026",
  prompt:"Read README.md and explain the project",
  maxSteps:8,
  tokenBudget:10000
});
for await (const event of client.watch(task.id)) {
  console.log(event.type,event.detail);
  if (event.type==="finished") break;
}
```

To retry after an uncertain network response, submit **the same ID and identical input**. A mismatched payload returns HTTP 409. The SDK never silently resends a POST. Only SSE GET reconnects from a committed cursor.

Writes and local inspection commands must be explicitly enabled per task.

## Stateful thread API (2026)

The canonical SDK also exposes a **native-backed thread facade**, following the
session/stream separation used by current Codex, Qwen Code and DeepSeek Harness
SDKs. The TypeScript layer **does not** run a second agent loop.

```ts
import { DominionRuntime } from '@dominion/native-client'

const runtime = new DominionRuntime({baseUrl:'http://127.0.0.1:4317'})
const thread = runtime.startThread({
  allowWrite: false,   // opt in deliberately for mutating tasks
  allowShell: false,
  maxSteps: 8,
  tokenBudget: 10000,
})

// For synchronous application logic:
const result = await thread.run('Inspect README.md and summarize the engine.')
console.log(result.status, result.finalResponse)

// For UI event streaming:
const streamed = await thread.runStreamed('Inspect another file.')
for await (const event of streamed.events) {
  console.log(event.seq, event.type, event.detail)
}

// Persist thread.id in your own application to resume its Rust transcript:
const resumed = runtime.resumeThread(thread.id)
const next = await resumed.run('Continue from the earlier inspection.')
```

A run's result is accepted only after reading the persisted terminal task
record from Rust. Failed, cancelled, or review-required runs return a non-success
status and a null `finalResponse`. The SDK preserves the native request-ID
idempotency contract and never retries POST requests behind the caller's back.
An `AbortSignal` stops **client waiting**; `thread.cancel()` is the explicit
server-side stop action.

The high-level facade lives in `src/runtime.ts`; the original `DominionClient`
low-level methods, events, and return types remain available. Both call
`/api/tasks` on the same Rust daemon. No external model vendor SDK, parallel
workflow brain, or additional package is necessary.

### Implementation references

- [Codex TypeScript SDK](https://github.com/openai/codex/tree/main/sdk/typescript):
  thread continuation and structured run events
- [Qwen Code SDK](https://github.com/QwenLM/qwen-code/tree/main/packages/sdk-typescript):
  CLI/SDK transport separates the execution core from a programmatic client
- [DeepSeek Harness SDK](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/sdk):
  protocol/server/client boundary between a complete runtime and its SDK
- [ACP TypeScript SDK](https://github.com/agentclientprotocol/typescript-sdk):
  a potential future interoperability adapter; not a replacement for Rust execution
