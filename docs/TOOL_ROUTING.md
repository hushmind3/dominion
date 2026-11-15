# Native lazy MCP schema routing — 2026-11-15

## Why this exists

The champion branch already retains owner instructions and exact historical
receipts with a bounded context projector. A different bottleneck remained:
`src/engine.rs` appended **every permitted MCP schema** to each model request.
The MCP registry can list up to 8 servers with up to 80 tools per server.
Even when the user needs one tool, the provider previously received every
definition. Reducing the original history would not fix this repeated schema
cost.

This change uses a **first-party native Rust selector**, not a foreign agent
SDK or a provider-specific model tool-search service. It preserves the
existing permission model, model endpoint, SQLite observations, and HTTP/SDK
task contracts.

## Actual execution path

1. `POST /api/tasks` → `src/api.rs` → `src/runtime.rs` →
   `src/engine.rs::run` reads `allowMcp`, `allowWrite`,
   `allowShell` and `allowSubagents` from the task.
2. `specs()` creates the allowed built-in declarations; only
   `allowMcp=true` exposes `tool_discover`. The permitted MCP registry is
   loaded from `Mcp::definitions()` and remains *in Rust memory*.
3. For **each model step**, `schema_router::select()` retains the native
   tools, scores external tools by meaningful name/description terms, and
   automatically includes up to four strongly relevant MCP schemas.
   Promoted schemas from an explicit discovery request take priority.
   It strictly enforces a bounded serialized-JSON schema budget:
   `min(16,384, max(4,096, floor(context_limit/3)))` bytes.
4. The existing `context::project()` subtracts the **selected** tool-schema
   bytes and protocol reserve before projecting the model messages. The
   provider receives only `exposed` schemas.
5. A model may call `tool_discover` with `{"query":"invoice payment", "limit":4}`.
   The dispatcher searches *only the already permitted MCP catalog* and
   returns short, scored candidate descriptions. It verifies which full
   definitions fit within the next-step budget; only those names appear
   in `promoted` and are callable on the next step. No paid search model,
   embedding server or separate TypeScript harness is involved.
6. If the model invents an unadvertised tool name, the Rust dispatcher
   records a paired negative `tool_result` and **does not record a
   `tool_start` or execute an external effect**. The model can then
   discover the correct capability without creating a false uncertain-write
   record.
7. `tool_schema_routed` events persist `catalogCount`,
   `advertisedCount`, `catalogBytes`, `advertisedBytes`,
   `omittedCount` and selected MCP aliases. The existing SSE,
   TypeScript client and native React task inspector use the same events.

The router does **not** grant any MCP permission. Eligible tools are
populated only when the user opted into MCP for that task. Network side
effects are still made by the existing Rust `Mcp::call` and are recorded
as task tool observations. Credentials remain in the server's MCP
configuration and are not copied into schema events.

## Test evidence and honest measurement

- `src/engine/schema_router.rs` tests: exact budget enforcement,
  no eager irrelevant MCP schemas, meaningful term relevance, explicit
  promotion, fail-closed oversized schemas and duplicate-name resistance.
- `src/engine/tool_routing_tests.rs`: a **real local HTTP MCP service with
  46 definitions** plus a real local mock OpenAI-compatible HTTP model.
  The model first requests discovery, then receives the promoted full
  schema, calls the actual MCP tool and completes. The negative test
  exercises an unadvertised write request through the real engine and
  ensures it creates neither a file nor a side-effect start record.
- CI workflow: `.github/workflows/ci.yml` (or the branch's native
  test workflow). The feature's pass/fail evidence is the concrete
  GitHub Actions run, not the number of code lines or model statements.
- React task execution view shows the actual serialized byte counts;
  the native `model_usage` event separately exposes input/output/cached
  tokens and the **estimated** KRW model cost from the configured rates.

`catalogBytes - advertisedBytes` is **JSON schema bytes avoided in a
particular request**, not guaranteed tokenizer savings, a proven increase in
task success, or settled API invoices. Current relevance scoring is a cheap
deterministic lexical heuristic. It can miss cross-lingual synonyms; model
discovery and a later embedding-backed selector are separate improvements.

## 2026 references and differences

- [Tool Attention (April 2026)](https://arxiv.org/abs/2604.21816):
  intent gating and lazy schemas motivate the two-phase pattern. Its
  headline end-to-end gains include simulated/projection evidence and
  are **not** Dominion benchmark results.
- [Semantic Tool Discovery (March 2026)](https://arxiv.org/abs/2603.20313):
  semantic similarity can replace the lexical ranker behind the same
  `schema_router` seam once measurable retrieval-quality tests exist.
- [OpenAI Agents SDK: hosted tool search](https://openai.github.io/openai-agents-python/tools/):
  deferred tool loading provides a useful interface comparison; Dominion
  implements independent model-neutral routing rather than requiring
  a specific hosted Responses model.
- [MCP-Zero (updated 2025)](https://github.com/xfey/MCP-Zero):
  tool discovery can be iterative rather than limited to an initial
  one-shot query. Dominion's discovery call feeds the next native Rust
  model step.

**Remaining:** measured true provider billed/cached token outcomes across
multiple real models, retrieval hit-rate benchmarks on multilingual tasks,
semantic matching and task-success ablations, and a dynamic policy for
promoted schema eviction under multi-phase tool-heavy jobs.
