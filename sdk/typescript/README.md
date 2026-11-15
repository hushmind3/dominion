# Dominion first-party TypeScript client

The package **@dominion/native-client** is a typed HTTP/SSE adapter to the native Rust runtime. It does not own agent planning, prompt policy, model turns, tool execution, completion or workflow scheduling. Browser and SDK share exactly the same Rust routes.

## Run a native task

~~~ts
import {DominionClient,DominionRuntime} from '@dominion/native-client'

const client=new DominionClient({baseUrl:'http://127.0.0.1:4317'})
const task=await client.submit({
  requestId:crypto.randomUUID(),
  prompt:'Inspect the repository and summarize remaining work',
  allowWrite:false,
  allowShell:false,
  modelTier:'auto',
  tokenBudget:10000,
  costBudgetKrw:2500
})
for await (const event of client.watch(task.id)) {
  console.log(event.type,event.detail)
  if (event.type==='finished') break
}
console.log(await client.task(task.id))
~~~

A requestId is required by the HTTP API (SDK submit generates one if omitted); retransmit the same key **and identical input** after an uncertain network response. Duplicate requests return the previous durable task, not a second model run. SSE GET reconnects from the last persisted event sequence, never from an invented cursor. If configured remote provider rates are unavailable, the backend refuses a paid-model KRW cap rather than claiming to enforce it.

## Continue a persisted task explicitly

~~~ts
const next=await client.resume(task.id,{
  requestId:crypto.randomUUID(),
  prompt:'Re-read the failed verification receipt, apply a narrow correction and test again.',
  allowWrite:true,allowShell:true,requireEvidence:true,
  maxSteps:10,modelTier:'quality'
})
~~~

This creates a fresh linked native task with the same session. No prior write, shell, MCP or subagent permissions are inherited and no earlier tool action is automatically repeated.

## Thread convenience layer

~~~ts
const runtime=new DominionRuntime({baseUrl:'http://127.0.0.1:4317'})
const thread=runtime.startThread({allowWrite:false,tokenBudget:9000})
const result=await thread.run('Check README and inspect project layout.')
const stream=await thread.runStreamed('Inspect another module.')
for await (const event of stream.events)console.log(event.type,event.seq)
const resumed=await thread.continueFrom(stream.taskId,
  'Verify the previous conclusion from the persisted work evidence.')
~~~

The thread facade is a transport-level session handle. Every method delegates to the native Rust task scheduler; it is not an embedded external agent harness.

## Real product APIs

~~~ts
const agent=await client.agentCreate('Code reviewer',
  'Verify the source and test evidence before accepting a change.')
const allAgents=await client.agents()

const providers=await client.providerCatalog()
const tools=await client.toolCatalog()

const workflow=await client.workflowCreate('Repository verification',[
  'Read source code and identify required changes.',
  'Inspect changed files and run available tests.'
])
const run=await client.workflowStart(workflow.id,{
  requestId:crypto.randomUUID(),allowShell:true,
  tokenBudget:10000,modelTier:'quality'
})
const latest=await client.workflowRun(run.id)
~~~

Native endpoints also support owner-defined discussion rooms, local or optional paid videos, isolated improvement experiments, memory and skills. Keep secrets in the runtime's server-side configuration; they are never returned by providerCatalog.

## Autonomous coding with real acceptance checks

~~~ts
const repaired=await client.submit({
  requestId:crypto.randomUUID(),
  prompt:'Fix calc.js, inspect the failing test and repair until it passes',
  allowWrite:true,
  allowShell:true,
  requireEvidence:true,
  planningMode:'auto',
  maxSteps:8,
  acceptanceChecks:[{
    program:'node',
    args:['--test','calc.test.js']
  }]
})
for await (const event of client.watch(repaired.id)) {
  if (event.type==='acceptance_result') {
    console.log('actual test:', event.detail.check,
      event.detail.success, event.detail.exitCode)
  }
  if (event.type==='repair_required') {
    console.log('repair stage:',event.detail.revision)
  }
  if (event.type==='finished')break
}
const final=await client.task(repaired.id)
console.log(final.status,final.result?.acceptance)
~~~

The check list contains executable and argument-array pairs, never a shell command string. The native Rust runtime validates up to four explicit checks; without one it may select a conservative Cargo/npm test from an existing project manifest. Only a real passing test **after the last source change** constitutes verified acceptance. Failed code gets the recorded stderr/stdout and another bounded repair opportunity, not a fabricated green result. The Rust runtime serializes mutating tasks sharing its workspace until acceptance. Read-only tasks can still run concurrently.

The main React workspace exposes the same check selection in its advanced task options. The implementation and actual Node integration tests are documented in [Native coding acceptance](../../docs/CODING_ACCEPTANCE.md). A green test on a tiny fixture is not an independent SWE-bench performance claim.

## Native evidence-backed context inspection

~~~ts
const evidence=await client.contextEvidence(task.id,'earlier design restriction')
for (const fact of evidence.facts.facts) {
  console.log(fact.sourceSeq,fact.kind,fact.quote)
}
for (const result of evidence.results) {
  console.log(result.kind,result.messageSeq,result.sourceTaskId,result.callId)
}
console.log(evidence.executionState)
~~~

This endpoint uses the same SQLite full-text index and citation-validated owner fact ledger as the actual Rust model loop. The full source is available through scoped native tools; the response's excerpts are not instructions and do not authorize tool effects. Explicit session continuation verifies prior-task ancestry before returning tool observations.

## Scope

Task costs are metered from operator-configured KRW rates, with separate cached-input accounting and usage uncertainty flags; they are not a provider invoice. Video generation has a separate project KRW ceiling. Image assets are uploaded through the native scene-image API and animated by local FFmpeg.

Use npm run build and npm test from sdk/typescript to run the TypeScript library and transport tests. The repository's primary runtime and source tests require Rust and are not run by importing this JS package.
