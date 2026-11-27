# Dominion — typed native model tool calls through independent Rust kernel
2026-11-15 · branch \`branch3\`

## Why this was necessary

Previously the product job path \`POST /api/jobs → jobs::run_prompt → native_agent::ConfiguredModel → llm::complete_for_session\` asked the model to emit a JSON *text plan* and discarded provider-native \`tool_calls\`. Model/provider-specific tool protocol support already existed in the separate chat engine, but was not reaching the Rust SDK kernel.

This is a **real product runtime change**, not a UI mock or an external harness dependency.

## Concrete reference implementations examined

- **Codex:** \`openai/codex/codex-rs/core/src/tools/router.rs\` keeps advertised model tool schemas alongside a registered execution-tool registry; \`tools/orchestrator.rs\` keeps approvals and sandbox decisions outside the model. This informed separating provider-decoded native calls from kernel-owned authorization/dispatch.
- **DeepSeek Harness:** \`deepseek-ai/deepseek-harness/packages/core/agent-loop/src/assistant-stream.ts\` separates stream accumulation and tool-call assembly from durable completion; \`packages/core/agent-loop/src/agent.ts\` derives request context from session facts.
- **OpenCode:** \`anomalyco/opencode/packages/core/src/session/runner/model.ts\` resolves a configured model independently of session execution; \`packages/core/src/session/compaction.ts\` handles the separate context budget. The product should not embed these TypeScript runners as Dominion's own loop.

Source links:
- https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/router.rs
- https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/orchestrator.rs
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/assistant-stream.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/agent.ts
- https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/runner/model.ts

## Production dataflow

\`\`\`
POST /api/jobs (same GUI, Rust SDK and TypeScript HTTP API)
  → engine/jobs::run_prompt
  → native_agent::ConfiguredModel [model/provider selected by owner]
  → Kernel::run_from → Model::complete_with_tools(schemas, receipts)
      → native_model::complete [typed provider adapter]
          Chat Completions: function tools + role=assistant/tool exchange
          Anthropic Messages: tool_use + tool_result content blocks
          Codex Responses: function_call + function_call_output SSE terminal
      ← ModelTurn { reply: usage/content, decision: typed native Action IDs }
  → scheduler::arrange [permissions, dependencies, parallel read bounds]
  → NativeTools::execute [owner approval + durable intent/receipt]
  → context::project + native role-paired transcript for next turn
  → kernel acceptance/replanning/checkpoint → result
\`\`\`

The provider's native function-call ID is carried intact into \`Action.id\` and persisted \`Receipt.id\`; arguments must decode to a JSON object. Duplicate or malformed IDs/arguments fail closed. Tool names are checked against owner grants and the actual registered Rust tool catalog. The native model cannot approve its own shell edits or bypass the SDK scheduler.

Native providers receive their **typed tool schemas once**, as a separate protocol field; the kernel now uses an appropriate text-only system prompt for text-only compatible models and a non-JSON native-call instruction for native models. This removes the contradictory legacy instruction asking a native model to produce fake JSON actions. Non-native CLI/local models continue to use the previous deterministic JSON text decision fallback.

Complete tool results stay durable; the provider sees recent host-confirmed tool results as properly paired native assistant and tool messages. The current prompt's duplicate recent tool output is omitted for native routes. Tool schemas, model use, reasoning protocol, credentials, and billed usage still follow the production \`gateway\`/vault route. No vendor SDK runs the agent lifecycle.

## Tests and limitations

- Public kernel integration: native \`Decision\` is executed and persisted without calling the JSON text parser; unauthorized model tool names are rejected before host execution.
- Host unit tests: OpenAI Chat Completions, Anthropic tool_use and Codex Responses parsing, duplicate call ID guards, argument validation, tool result pairing, Codex SSE terminal settlement.
- Actual loopback HTTP integration tests: OpenAI-compatible and Anthropic mock HTTP endpoints receive native schema and replayed structured messages, then reply with typed provider calls.
- Full loopback HTTP → Rust kernel → actual filesystem read → host receipt → next provider request → final completion test.
- Focused GitHub Actions runs SDK tests, targeted host Rust tests and main-based GUI TypeScript/Vite production build. Full product CI and SWE-bench remain outside this change.

**Streaming now implemented:** the native agent adapter consumes provider-native SSE incrementally, validates complete turn settlement before authorizing any tool action, and displays bounded pending progress through the autonomous job GUI (see `docs/SUB3-NATIVE-STREAM-2026-11-15.md`). **Remaining:** real-provider credential live call matrix, global prompt-cache accounting, true provider session continuation for every route, and sandboxed multi-agent spawning. A loopback HTTP mock proves protocol/adapter behavior, not comparative frontier task completion.

## Audit criteria

A successful code test alone is not evidence of frontier-grade autonomous intelligence. Independent model-and-task benchmarks still have to measure success rate, repaired code correctness, cost, latency, interruption/recovery and provider compatibility. This subsystem now gives the strong models a native tool protocol instead of forcing them through improvised JSON text.
