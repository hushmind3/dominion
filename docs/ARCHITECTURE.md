# Dominion Runtime Architecture — 2026-11-15

## One owner of execution

```text
Browser UI ────┐
               ├── HTTP/SSE API → Rust task runtime → configured LLM
TypeScript SDK ┘                    │
                                   ├── local workspace tools
                                   ├── owner correction/cancellation
                                   └── SQLite tasks/events/transcript
```

The TypeScript package provides transport, types and a read-only reconnecting event stream. It does not independently decide which tool to run or when a task is complete.

The Rust worker claims one queued task with an atomic SQLite status transition. A repeated request ID cannot enqueue a second worker. Each model step reads the most recent queued user correction before sending the next request, checks cancellation before tool effects, and stores each tool observation before its next step. Verified file writes include a SHA-256 readback receipt.

Task status is persisted independently from SSE subscribers. On restart, queued work is replayable; previously running work becomes `needs_review` because a remote effect could already have happened before a crash.

## Source comparison informing the new boundaries

- DeepSeek Harness (2026): event-backed sessions and agent-loop boundaries distinguish a persisted runtime from presentation. https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/core.md
- Qwen Code (2026): HTTP clients and programmatic SDKs share a single execution core. https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md
- OpenAI Codex Rust source: native tool dispatch is isolated from agent-facing UI and provider transport. https://github.com/openai/codex/tree/main/codex-rs/core/src

These are design comparisons, not borrowed execution policies or reproduced benchmarks.

## Contracts and evidence

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | single loopback-bound Rust runtime and provider setup |
| `src/api.rs` | validated task commands and replayable SSE |
| `src/store.rs` | atomic SQLite admission, state, messages and receipts |
| `src/engine.rs` | model requests, permissioned tool loop, verification and resource caps |
| `web/index.html`, `web/app.js` | task submission, progress, corrections, cancellation and receipts |
| `sdk/typescript/src/index.ts` | typed external HTTP/SSE access |

## Known boundaries

The initial engine supports workspace file reads, opt-in verified writes, and opt-in constrained local test commands. Tool execution is sequential, not parallel. The model request is non-streaming; task events stream as durable SSE. The token cap stops **future** calls after a recorded provider usage report, but cannot undo an already billed request or infer missing usage.

No real provider inference, paid invoice comparison, SWE-bench score, film output, distributed runner, or multi-user security audit is claimed by the source commit. Runtime tests need a Rust-equipped host. The first implementation emphasizes true native behavior rather than mock-complete feature surfaces.
