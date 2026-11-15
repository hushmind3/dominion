# Champion: owner task retries and execution controls (2026-11-15)

## Native call path

`POST /api/jobs` → `src/api/jobs.rs::create` → `src/engine/jobs.rs::enqueue_owner_prompt` → existing `enqueue_once` → `Store::put_if_absent("job", ...)` (SQLite INSERT OR IGNORE) → standard Rust job worker. This does **not** add a second planner or agent runtime.

A caller-supplied `requestId` (8–128 ASCII letters/digits/underscores/hyphens) is hashed with a domain separator to derive a stable owner-task job ID. The key stays out of the model prompt and job input. An identical request returns the existing job, even if its status changed; a changed normalized payload with the same key is HTTP 409, never silently overwritten. Calls without a key retain their original non-idempotent "new task" behavior. This is per local Dominion deployment, not an authorization boundary for future multi-tenant use.

The existing `toolsExecutionEnabled` flag was previously discarded in `src/api/jobs.rs::create`; it is now preserved alongside `toolsEnabled`. Invalid nonboolean settings are rejected instead of silently enabling tools. The worker already consumes both fields through its `TurnRequest`.

## SDK contract

`sdk/typescript/src/client.ts::jobs.promptOnce(prompt, requestId, model, options)` maps to the same Rust job endpoint; it validates the key before sending. `jobs.prompt` still starts a fresh job, and neither method silently retries a POST. The TypeScript input type `JobPromptOptions` exposes owner tool controls. Losing the HTTP response is not proof that the job never ran: reuse the *same* request ID to retrieve the original job.

External reference: [DeepSeek Harness agent-loop 2026](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/README.md) uses a durable session history for agent steps. We keep Dominion's SQLite and Rust execution ownership rather than nesting the external agent driver.

## Defined regression tests

- `src/api/jobs.rs::owner_request_tests`: same key reuses one task, changed prompt yields 409, concurrent requests yield a single SQLite job, malformed flags and IDs are rejected.
- `sdk/typescript/tests/sdk.test.mjs`: `promptOnce` sends the exact key, preserves a deliberate `toolsExecutionEnabled:false`, and rejects an invalid key before network I/O.

Suggested local validation:

```sh
cargo test --release owner_request_tests
npm test --prefix sdk/typescript
node --test scripts/source-hygiene.test.mjs
node scripts/source-hygiene.mjs
npm run build --prefix web
```

**Evidence boundary:** source was committed and structural anchors were checked in the connector, but Cargo, Node, workflow CI and live model execution were not run in this environment. This change guarantees only duplicate *job creation* under the same key at the server; it does not assert exactly-once external side effects inside a run or claim a benchmark score.
