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
