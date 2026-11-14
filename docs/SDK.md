# DOMINION Native Agent SDK · 2026-11-14

DOMINION's first-party Node.js agent SDK reuses its OWN model provider, execution runtime, MCP, tool permissions, persistent SQLite message store and goal harness. No vendor CLI, external policy engine or third-party SDK is a runtime dependency.

## Public programmatic entrypoint

```js
import {dominionSdk} from './server/native-sdk.mjs';

const thread=dominionSdk.startThread({
  agentId:'general', providerId:'configured-provider', model:'configured-model',
  delegates:['researcher','coder'], maxDelegateDepth:2, maxDelegations:4,
  limits:{maxIterations:12,maxToolCalls:40,maxParallelReadTools:4}
});
const turn=await thread.run('Inspect my repository and implement a working fix.');
console.log(turn.finalResponse,turn.threadId,turn.runId,turn.usage);

// Continue with the SAME persisted message history after a process restart:
const again=dominionSdk.resumeThread(thread.id);
const result=await again.run('Verify the changed files and summarize the outcome.');

// Consume structured runtime/model/tool events:
const {events}=await again.runStreamed('Show the current implementation status');
for await (const event of events) console.log(event.type,event);
```

`run()` returns `{threadId,runId,finalResponse,items,usage}`; `runStreamed()` returns `{events:AsyncGenerator}`. Cancellation uses `AbortSignal` or `thread.cancel()`. Tools are real: a finished answer is not evidence of a successful file write.

SDK developers can register real tools without altering the model provider interface:

```js
dominionSdk.registerTool({
  name:'sdk_project_metrics',description:'Read local project metrics',
  parameters:{type:'object',properties:{project:{type:'string'}},required:['project']},
  approval:false, // Set only for read-only, trusted operations.
  handler:async ({project},context)=>({project,files:5})
});
```

Registered tools enter the SAME execution loop and tool-call ledger. New SDK tools require user approval by default. The same persisted permission workflow handles privileged actions. Registered tools must not shadow built-in/MCP tool names. External code should not be treated as intrinsically trustworthy.

## Authenticated local HTTP access

```text
GET  /api/sdk/status
GET  /api/sdk/threads
POST /api/sdk/threads                      {agentId,providerId,model,delegates,limits}
GET  /api/sdk/threads/:id
GET  /api/sdk/threads/:id/history
POST /api/sdk/threads/:id/run              {input,limits}
POST /api/sdk/threads/:id/stream           {input,limits} -> text/event-stream
POST /api/sdk/threads/:id/cancel
```

The existing gateway's admin-token/origin checks apply to all SDK routes. A thread stores its options in the existing session record and its turns in the existing message store.

## Multi-agent execution

A configured thread exposes `agent_delegate` when its allowed agent IDs resolve to real agents. A specialist gets its own linked child session, its own Provider/model (fallback to parent's choice), bounded iterations and tool budget. Child events expose start/completion/failure and the child session identifier. The parent receives the actual final response; failure is returned as a tool failure, never invented as a successful result.

This is **agent-as-tool delegation**, not a handoff that silently replaces the root agent or edits root instructions. Delegation is whitelisted, depth-limited, and budgeted. Child actions use the same tool approval gates.

## Agent runtime invariants

- Runtime tool batches keep the model's call IDs and output order.
- Known read-only tools can overlap with a concurrency cap; unknown/MCP writes and mutations are serialized barriers. No optimistic parallel writes.
- Model and tool starts/results are persisted in execution records. Interruption of an unresolved external effect demands review; no blind replay.
- SSE streams propagate cancellation; disconnect does not imply an external mutation was rolled back.
- Provider routing remains independent of this SDK. A user's configured provider/model supplies inference.
- No extra content-refusal classifier is installed in this SDK. Provider-side behavior and user-granted execution permissions remain separate concerns.
- Models/credentials are not bundled. Without a configured model the SDK produces an error rather than fabricated results.

## Verified code paths and testing

```text
server/routes/sdk.mjs HTTP/SSE
  -> server/native-sdk.mjs (host wiring)
    -> server/agent-sdk.mjs (threads, tools, agent delegation, streaming)
      -> server/engine.mjs runTurn()
        -> server/execution-runtime.mjs
          -> server/tool-scheduler.mjs
          -> server/tools.mjs (approval + MCP + filesystem/shell)
          -> server/providers.mjs
      -> server/store.mjs (SQLite messages, sessions, executions)
```

Unit specifications: `tests/agent-sdk.test.mjs`, `tests/tool-scheduler.test.mjs`, `tests/execution-runtime.test.mjs`.
The source and tests were committed; end-to-end physical provider/MCP tests, full Node 24 CI, OS sandbox enforcement, durable stream replay and multi-user remote tenancy have **not yet been demonstrated** in this change. No claim of measured token savings or performance speedups is made.

## 2026 source-code research (design reference, not copied code)

- [Codex SDK: `sdk/typescript/src/thread.ts`](https://github.com/openai/codex/blob/main/sdk/typescript/src/thread.ts) — thread identity, `run`, `runStreamed`, resume handling and per-turn events. The official TS SDK invokes Codex CLI; DOMINION doesn't require it.
- [Copilot SDK: `nodejs/src/session.ts`](https://github.com/github/copilot-sdk/blob/main/nodejs/src/session.ts) — session event handlers, external tool calls, factory/subagent lifecycle, cancellation. Its SDK uses CLI/RPC, not a standalone extracted inference engine.
- [OpenAI Agents SDK: `packages/agents-core/src/run.ts`](https://github.com/openai/openai-agents-js/blob/main/packages/agents-core/src/run.ts), [`runner/toolExecution.ts`](https://github.com/openai/openai-agents-js/blob/main/packages/agents-core/src/runner/toolExecution.ts) — decomposed runner, response/tool effects, approvals, streaming and state ownership.
- [Claude Agent SDK 2026 changelog](https://github.com/anthropics/claude-agent-sdk-typescript/blob/main/CHANGELOG.md) — transport, task tracking, session mirror/resume. Claude Code's closed native binary is not source code that can simply be transplanted.

Implementation is independent and intentionally limited to documented ideas and directly inspected public interfaces; no vendor internals or policy are pasted into DOMINION.
