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
  planningMode: 'automatic',
  maxTotalTokens: 120_000,
  mcpEnabled: false,
  skillsEnabled: true,
  webEnabled: false,
  subagentsEnabled: false,
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

The optional flags apply to the **new native agent run**, not to legacy chat,
room, video or RSI execution. When enabled, public web requests and delegated
tasks still require per-call approval. `planningMode: 'always'` exposes
`update_plan`; each revision is returned as `current.checkpoint?.plan`.
Token ceilings are enforced before the next request from provider-reported usage;
a single model response may exceed the ceiling. This is not an exact KRW
billing cap.

The engine service is `src/engine/native_sdk.rs`; `src/api/sdk.rs` is only
its HTTP surface. Native model support includes Anthropic Messages and
OpenAI-compatible chat completions. External TypeScript code never executes
tools itself.
