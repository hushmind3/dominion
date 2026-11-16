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

Other methods: `listRuns`, `resumeRun`, `cancelRun`, `reconcileRun`,
`steerRun(run.id, instruction, "append" | "replace")`. Steering changes the
active owner goal in the same native execution; usage and effect receipts remain
cumulative. `budgetUsd` is an alternative to `tokenBudget` and requires configured
provider prices. `shellSandbox: "required"` rejects shell execution when the OS
sandbox backend is unavailable.
`reconcileRun` requires manual observation of interrupted effects; it never
silently repeats a potentially applied filesystem or shell operation.

## Video jobs from the same client

```ts
const video = await agent.createVideo({
  brief: 'Three short shots about a new product',
  scenes: ['Exterior opening', 'Close-up demonstration', 'Final detail'],
  engine: 'comfyui',
  duration: 18,
  autoUpload: false,
})
const progress = await agent.getVideoJob(video.id)
if (progress.status === 'completed') {
  const derivative = await agent.regenerateVideoScene(
    video.id, 1, 'Detailed hand-held product demonstration',
  )
  console.log(derivative.id) // new job, original remains immutable
}
```

Methods `listVideoJobs`, `createVideo`, `getVideoJob`, `regenerateVideoScene`
and `queueVideoUpload` use the existing Rust video endpoints. An
`autoUpload: true` option must be explicit, requires YouTube credentials and
creates a **private** upload job after rendering. Quality reports are technical
container/stream checks, not semantic or aesthetic quality guarantees.

Build separately with `npm install && npm run build` inside this package.
Publication is not implied. See [Rust SDK architecture](../../docs/HARNESS.md).
