# Dominion auxiliary TypeScript client

The Rust `dominion-core` harness and the product SQLite adapters own
execution, approvals, checkpoints, policies and verification. This small package only calls the
native Rust server over HTTP. It is **not** an independent agent harness.

```ts
import { DominionAgentClient } from '@hushmind3/dominion-sdk'

const agent = new DominionAgentClient({
  baseUrl: 'http://127.0.0.1:4317',
})
const run = await agent.startRun({
  instruction: 'Check and repair this repository',
  verifyCommand: 'cargo test',
  workspaceDir: '/absolute/path/to/project',
  tokenBudget: 100000,
})
const current = await agent.getRun(run.id)
if (current.status === 'awaiting_approval') {
  const call = current.checkpoint?.pendingCall
  // Inspect call.name and call.arguments before approving:
  if (call) await agent.approveCall(run.id, call.id)
}
```

A model reply and a verified task are different. Inspect `run.verified === true`
and `run.taskOutcome === 'succeeded'` before claiming a task was completed
successfully. A requested `verifyCommand` without successful acceptance
evidence returns `status: 'failed'` while retaining the model answer for review.

For external integrations, `await agent.waitForRun(run.id)` returns the
authoritative snapshot at completion, interruption, approval or reconciliation
boundaries. `await agent.waitForVerifiedRun(run.id)` accepts only independently
verified `taskOutcome: 'succeeded'` and rejects an unverified text reply.
Both helpers support `AbortSignal`, bounded timeouts and progress callbacks.

Other methods: `listRuns`, `resumeRun`, `cancelRun`, `reconcileRun`,
`steerRun(run.id, instruction, "append" | "replace")`. Steering changes the
active owner goal in the same native execution; usage and effect receipts remain
cumulative. `budgetUsd` is an alternative to `tokenBudget` and requires configured
provider prices. `shellSandbox: "required"` rejects shell execution when the OS
sandbox backend is unavailable.
`reconcileRun` requires manual observation of interrupted effects; it never
silently repeats a potentially applied filesystem or shell operation.

Build separately with `npm install && npm run build` inside this package.
An external local project can then run `npm install /absolute/path/to/dominion-rust/packages/sdk-typescript`.
Publication is not implied. For in-process Rust embedding without the HTTP server,
see the [Rust SDK](../../crates/dominion-agent-sdk/README.md).

## Optional source provenance

`POST /api/sdk/runs` accepts an optional agent ID and source reference
such as `{"kind":"task_request","id":"issue-123"}`. The source reference
is untrusted display provenance, not authorization or tool permission.
`GET /api/sdk/runs/source/:kind` retrieves recent indexed runs for that
source kind. All executions share the same native Rust approval and
verification pipeline.

Pass an optional `idempotencyKey` when creating a native SDK run through `POST /api/sdk/runs` to make retries safe. An identical retry returns the same persisted run; a changed request with the same key is rejected with HTTP 409. No new API endpoint, model runner or database is required.

SDK completion status and manual effect reconciliation now read only the matching session's latest native execution and exact child frames. API request/response contracts remain unchanged; evidence lookup errors surface instead of silently accepting an unverified result.

If a native run reports `status: "needs_review"` and `failureClass: "completion_evidence_unavailable"`, `POST /api/sdk/runs/:id/resume` rechecks only persisted completion evidence without replaying LLM or tools. It returns HTTP 409 when the completed execution is missing or has unresolved side effects.

## Durable Run/Event contract

A frontend or external integrator can page native run events in global SQLite
journal order. SDK facades and native execution IDs are different namespaces.
`getRunEvents(sdkRunId)` resolves the actual `runRef.executionId` instead
of guessing from the SDK run ID.

```ts
const page = await agent.getRunEvents(run.id, {after: 0, limit: 100})
for (const event of page.events) {
  // Persist the global event.seq cursor only after processing each event.
  console.log(event.seq, event.runRef.executionId, event.type)
}
const next = await agent.getExecutionEvents(page.executionId, {
  after: page.nextAfter, limit: 100,
})
```

`historyTruncated` indicates an actual retention gap; in that case retrieve
the authoritative execution state before continuing. Room speaker delivery,
Lead/Worker outcomes and external read-only agent receipts share the same event
journal while keeping their different execution identities and permissions.
For external agents, an `externalRunId` is **not** a native tool receipt.

`npm test` compiles the client and exercises run cursors, SDK facade lookup,
explicit 409 for unadmitted runs, approval boundaries and verified-success
gating. The development GitHub workflow runs this package independently.

## Optional remote Rust workers

`createRemoteJob({instruction, maxRounds, verifyCommand})` submits a
coordinator-owned task. `listRemoteJobs`, `getRemoteJob`,
`cancelRemoteJob` and `reviewRemoteJob(id, accept)` operate on the
coordinator HTTP API. They do not issue a worker credential to the frontend.
A remote result is always `nativeVerified: false` at the coordinator;
even an owner-reviewed report cannot impersonate local Tool Gateway evidence.

Worker setup and lease recovery: [Remote Worker](../../docs/REMOTE-WORKER-2026.md).


## Registered external SDK plugins (local server)

In DOMINION, open Connections → Plugins → SDK integrations and register a
plugin with an existing agent. Copy the one-time connection key securely.

```ts
import { DominionPluginClient } from '@hushmind3/dominion-sdk'
const plugin = new DominionPluginClient({
  pluginId: 'my-plugin',
  pluginToken: process.env.DOMINION_PLUGIN_TOKEN!,
})
await plugin.heartbeat()
const run = await plugin.startRun({
  instruction: 'Inspect the project',
  idempotencyKey: 'unique-logical-task-id',
})
console.log(await plugin.getRun(run.id))
```

Repeat heartbeat every 60 seconds while the process is active. Presence is
recent authenticated traffic, not a remote process health assertion.
The scoped credential cannot approve tools, change agents or read other
plugins' tasks. Existing native permission and approval rules apply.
The server enforces one active task, 20 submissions per rolling 24 hours,
8 rounds and a 20,000-token limit per task; retries keep the same key.
Disabling or rotating a key blocks new requests; already admitted tasks
can be cancelled by the owner in the SDK run panel. Local loopback use is
supported; the separate remote worker interface is for remote machines.

For isolation from local owner APIs, configure `DOMINION_ADMIN_TOKEN` on the
server. The registered plugin client needs only its own key for its scoped
endpoints; the server's pre-existing unauthenticated local-owner mode is not
an operating-system sandbox.

## Shared DOMINION Cloud account (external app)
The cloud gateway is independent of the local Rust runtime. Import DominionCloudClient from this SDK. In a trusted local backend, call DominionCloudClient.begin('DOMINION Assist'), persist the returned token in your OS credential store, and show only the returned id to the user. They approve the code in DOMINION Cloud > External apps. Construct new DominionCloudClient(token), poll status(), then use me(), models(), complete({model,messages}) or disconnect(). Pairing expires after ten minutes; approved app sessions expire after seven days. Free inference only, shared account quota. No upstream key is distributed. Requires a deployed Worker with app_pairing support; do not embed tokens in static assets.

For a standalone app, open pairing.verification_uri in the system browser; Google approval there is supported when the Worker origin is registered with Google. Otherwise display pairing.id for approval inside DOMINION. Never open a non-DOMINION verification URI.
