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
Publication is not implied. See [Rust SDK architecture](../../docs/HARNESS.md).

## Optional origin metadata for tool-initiated runs

`POST /api/sdk/runs` may include `agentId` and `source: {"kind":"money_candidate","id":"..."}`.
The agent ID is validated and retained in the shared model turn; provider/model overrides remain explicit.
The source pair is **untrusted display provenance only**, not authorization, evidence, or a tool permission.
`GET /api/sdk/runs/source/:kind` returns at most 40 recent indexed run links;
use `GET /api/sdk/runs/:id` for execution/approval status and existing cancel/resume endpoints.
No second SDK, executor, or SQLite database is created.


Money owners may use `POST /api/money/candidates/:id/implement` with explicit `approved: true`, `agentId`, `providerId`, `model` and a `tokenBudget`. The Money adapter atomically claims the candidate and prepares an ordinary SDK run; it returns the same SDK run on repeated approval. Its run ID supports all existing SDK status, approve, cancel, and resume operations. No separate SDK client surface is required.
