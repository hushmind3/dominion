# DOMINION TypeScript SDK

Typed client for the **same Rust execution product**. Use `Dominion` from `sdk/typescript/src/index.ts`. All planning, MCP permission enforcement, cost accounting, cancellation, durable job writes and verification happen in Rust, not in a duplicate JavaScript agent loop.

- `sdk.jobs.create({kind:'prompt',input:{prompt:'Inspect and fix the tests'}})`, `sdk.jobs.get(id)`, `sdk.jobs.cancel(id)` and `sdk.jobs.steer(id, instruction)`.
- Per-run algorithms: `sdk.jobs.create({kind:'prompt',input:{prompt:'Inspect code',algorithmId:'focused-v1'}})` or `sdk.nativeRuns.start({instruction:'Inspect code',algorithmId:'focused-v1'})`. Omit for `standard-v1`; the server pins the selected revision across resume and rejects unknown profiles.
- `sdk.nativeRuns.start({instruction:'...', verifyCommand:'cargo test'})`, `sdk.nativeRuns.approve/reconcile/resume/cancel`. Runs use existing UI job records.
- `sdk.nativeRuns.replay(id)` and `sdk.nativeRuns.compare(baselineId, candidateId)` reconstruct recorded decisions and compare the **same task/model/budget/permissions** without repeat model/tool calls; missing cost or independent acceptance is explicitly reported.
- `sdk.events()`, `sdk.chat(...)`, provider, session, memory, tasks and tool calls work with existing Rust REST/SSE routes.

This package deliberately has **no native `video` or `rsi` client**. Add third-party media or other services through user-approved MCP/tools/plugins when needed.

Run `npm install --prefix sdk/typescript --no-package-lock --ignore-scripts --no-audit && npm test --prefix sdk/typescript`. See [product contract](../../docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md).
