# DOMINION legacy-compatible TypeScript client

[Product README](../../README.md) · [Primary TypeScript SDK](../../sdk/typescript/README.md) · [Active Rust harness architecture](../../docs/HARNESS.md)

`@hushmind3/dominion-sdk` is a small, **optional HTTP client** for `/api/harness/runs`. It does **not** contain a model planner, tool executor, permission authority, or separate agent runtime.

**Current `dominion-sub3` behavior:** New `/api/harness/runs` requests are translated by `src/api/sdk.rs` into the same `src/api/sdk_runs.rs` / `src/engine/native_agent.rs` / `dominion-agent-sdk::Kernel` job path used by the native work panel. The compatibility route restricts the authorized tool set to workspace/file operations and shell. Historical standalone `sdk_run`/FileJournal entries remain inspectable as **read-only archives**; approval/resume/reconcile requests for those old records return a conflict instead of replaying uncertain effects.

## Usage

```ts
import { DominionAgentClient } from '@hushmind3/dominion-sdk'

const client = new DominionAgentClient({
  baseUrl: 'http://127.0.0.1:4317',
})

const run = await client.startRun({
  instruction: 'Inspect and repair this repository',
  verifyCommand: 'cargo test',
})
const current = await client.getRun(run.id)

if (current.status === 'awaiting_approval') {
  const call = current.checkpoint?.pendingCall
  // Inspect actual tool name/arguments and approve only when authorized.
  if (call) await client.approveCall(run.id, call.id)
}
```

Methods implemented in [`src/index.ts`](src/index.ts): `listRuns`, `getRun`, `startRun`, `approveCall`, `reconcileRun`, `resumeRun` and `cancelRun`. The client surfaces API errors rather than inferring completion from a disconnected request.

```sh
npm install --prefix packages/sdk-typescript --no-package-lock --ignore-scripts --no-audit
npm run typecheck --prefix packages/sdk-typescript
npm run build --prefix packages/sdk-typescript
```

This package remains marked `"private": true` in its own `package.json`; the README is not an npm publication claim. New integrations should normally prefer [`sdk/typescript`](../../sdk/typescript/README.md) and its `/api/sdk/runs` contract.

The previous `dominion_harness::sdk::AgentSdk` implementation and standalone CLI were **removed** from the current source. Do not base new integrations on that namespace.
