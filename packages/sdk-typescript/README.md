# Dominion auxiliary TypeScript client

The Rust `dominion_harness::sdk::AgentSdk` owns the execution, approvals,
checkpoints, policies and verification. This small package only calls the
native Rust server over HTTP. It is **not** an independent agent harness.

```ts
import { DominionAgentClient } from '@hushmind3/dominion-sdk'

const agent = new DominionAgentClient({
  baseUrl: 'http://127.0.0.1:4317',
})
const run = await agent.startRun({
  instruction: 'Check and repair this repository',
  verifyCommand: 'cargo test',
})
const current = await agent.getRun(run.id)
if (current.status === 'awaiting_approval') {
  const call = current.checkpoint?.pendingCall
  // Inspect call.name and call.arguments before approving:
  if (call) await agent.approveCall(run.id, call.id)
}
```

Other methods: `listRuns`, `resumeRun`, `cancelRun`, `reconcileRun`.
`reconcileRun` requires manual observation of interrupted effects; it never
silently repeats a potentially applied filesystem or shell operation.

Build separately with `npm install && npm run build` inside this package.
Publication is not implied. See [Rust SDK architecture](../../docs/HARNESS.md).


The original main Rust harness is reachable at `/api/harness/runs` in the integrated sub3 product. `/api/sdk/runs` remains the native-job endpoint used by the existing main GUI; both have durable Rust execution and approval flows.
