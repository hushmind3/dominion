# DOMINION — Native Autonomous Workspace

A clean, local-first Rust agent runtime with a browser workspace and a first-party TypeScript SDK.

## Run

Install Rust stable. Configure a local OpenAI-compatible model server (Ollama, llama.cpp server, or another compatible endpoint).

```sh
export DOMINION_WORKSPACE=.
export DOMINION_MODEL_URL=http://127.0.0.1:11434/v1/chat/completions
export DOMINION_MODEL=qwen3
cargo run
```

Open http://127.0.0.1:4317

For a compatible paid API, configure `DOMINION_MODEL_URL`, `DOMINION_MODEL`, and `DOMINION_API_KEY`. The engine does not resell subscriptions. The default endpoint expects a model that the user has already installed.

## Actual execution path

- Browser `web/app.js` → `POST /api/tasks` → Rust `src/api.rs` → SQLite `src/store.rs` → Rust `src/engine.rs`.
- The Rust model/tool loop reads session messages, chooses the configured model, interprets tool calls, checks execution permissions, persists receipts, reads written files back and returns durable completion status.
- The browser and the TypeScript SDK call exactly the same HTTP API and native runtime.
- After an interrupted connection, the client follows a read-only SSE stream using the last persisted event cursor.
- An existing request ID with identical input returns the original task. Changing the input under that ID returns 409.
- Restart recovery marks uncertain in-flight effects for human review instead of replaying them.

## User controls

`allowWrite` enables file modifications within the configured workspace; default is false. `allowShell` enables a restricted set of inspection/test commands; default is false. `requireEvidence` requires an observed verified write before the task can be marked completed. `maxSteps` and `tokenBudget` bound execution.

The backend listens only on `127.0.0.1` by default and uses a separate SQLite file at `.dominion/data.sqlite`. This is a local single-owner edition. Shell commands are **not** OS-sandboxed, and the write path is not a substitute for filesystem isolation under hostile concurrent processes. Do not expose the server to the public internet.

## Tests

```sh
cargo test
npm install --prefix sdk/typescript
npm test --prefix sdk/typescript
node --check web/app.js
```

## Development status

This is the first functional vertical slice of a fresh implementation: native task lifecycle, memory transcript, model adapter, permission-controlled file/tools, durable event stream, TypeScript client and desktop-friendly browser UI.

Legacy multi-agent rooms, film rendering and experimental recursive self-improvement have **not yet been reimplemented** in this new tree. They are not represented as working features until real execution routes and tests exist. The original main branch remains available for source reference and data migration work.

See [runtime architecture](docs/ARCHITECTURE.md).
