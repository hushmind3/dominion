# Sub2 — Progressive tool schemas in the single Rust turn loop (2026-11-18)

Branch: `dominion-sub2`. This is a real source implementation, **not** a claimed measured completion-rate or token improvement. Rust integration/unit tests are committed but have not been run in this workspace (Cargo is not installed). No release, frontend or platform-wide validation was performed.

## Problem linked to the 76/100 review

The previous `tool_projection.rs` selected tool schemas once from the original owner request. A later tool failure, revised plan, or user-compatible work loop could need a connector not advertised on that round. Native model tool calls were also missing a final provider-round advertised-name admission check: some models could ask for an unadvertised native tool directly, bypassing *tool projection* (not native user approval). A full schema list avoided discovery misses but repeated many irrelevant MCP schemas on every paid model request.

The main and sub3 branches already implement progressive discovery using a search-and-call bridge. This sub2 change intentionally does **not** copy a second SDK agent loop or install another process. It uses the existing `run_turn_body -> dispatch_tool_call -> execution_operation` path.

## Native integration

```text
owner chat / SDK prompt job / room turn
  → existing Rust run_turn_body
  → existing owner/context/tool/agent algorithm selection
  → freeze allowed tool catalog for this turn
  → (large catalog + no explicit core policy + not single-file fast path)
      -> initial projected 8 stable coding/research tools
         plus 4 query-relevant tools + dominion_tool_search
  → provider tool list for this round
  → model proposes tool call
      -> enforce that its exact name was advertised this round
      -> native dispatch_tool_call (cancel, budget, permission, receipts)
           ├─ regular native/MCP tool
           └─ dominion_tool_search (read-only, execution-ID-scoped)
                 -> rebuild currently connected, user-enabled catalog
                 -> rank real native/MCP definitions by request
                 -> persist normal search operation/receipt
  → select up to 8 discovered names from the frozen approved catalog
  → add their unchanged provider schemas for the *next* model round
  → model invokes actual tool by native name (not a synthetic wrapper)
  → same existing native dispatcher/approval/effect sealing
```

- The default projection is enabled only when more than 18 permitted tools exist. Small catalogs, user-selected `core` or `query-relevant` strategies, tools-disabled turns and deterministic single-file creation retain their previous behavior.
- The core has **eight** essential native schemas; the remaining four slots are request-relevant. Search is additional, deliberately first in the provider declaration order so schema-budget trimming does not silently evict it.
- The search query is at most 256 characters; each search returns at most eight concrete matches. At most 20 provider-visible schemas (including search) can accumulate in one turn. Duplicated names do not consume slots.
- The exact result uses native tools' existing JSON schema, preserving provider/runtime argument contracts. No new permission table, host/tool bridge, model router, data store, worker, SDK endpoint or background process exists.
- Current session policy and account connection filtering apply to the search catalog. Read-only parallel child sessions cannot discover writes, shells or MCP. Results from a newly connected service cannot exceed the catalog frozen after initial account/user filtering; real execution also rechecks policy at dispatch.
- A model tool name not included in the exact provider-round schemas is recorded as a soft tool error and **not** dispatched. This closes the hidden-name escape in explicit tool selection and prevents fabricated MCP tools from being launched.
- The search result is an actual `execution_operation` with `origin=model` and tool name `dominion_tool_search`; later tools use their real original names, approval lineage, budgets, cancellation and effect receipts. A search response is not an artifact-completion oracle.
- No automatic second planner or semantic reviewer is introduced. Advanced ProgramIR local candidate rollouts remain separately opt-in under the previous contract.

## Test inventory (unexecuted in this workspace)

- `src/engine/chat/capability.rs`: 80+ fake connector schemas, deterministic bounded prefix, direct task-relevant email connector exposure, bounded search/promotion, no forged/uninstalled promotion, explicit mode exclusions, invalid queries.
- `src/engine/chat/tool_projection.rs`: an unadvertised model tool name is rejected even if some other catalog contains it.
- `src/engine/chat/runtime/capability_tests.rs`: an actual search is executed through `dispatch_tool_call`, promotes the real `file_read` schema, then executes a native file read with two persistent operations and tool messages; wrong-session discovery, disabled discovery and read-only filtering are denied.
- The existing Rust `tool_schedule` read/exclusive barrier and execution-operation tests remain the behavior owner. No synthetic model call claims were made.

## 2026 sources and tradeoffs

- **DeepSeek Harness (2026):** registry scope and tool admission are distinct. The run loop resolves the available tools for a step rather than allowing registrations to serve as privileges: https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/tools.md
- **Qwen Code (2026):** progressive/lazy tool discovery can avoid sending a full MCP schema inventory, but must preserve exact tool identity and authorization: https://github.com/QwenLM/qwen-code/blob/main/docs/design/lazy-code-mode.md
- **Qwen Code (2026):** stable ordering helps cache hits when registered tools change: https://github.com/QwenLM/qwen-code/blob/main/docs/design/prompt-cache/global-tool-schema-stable-sort.md
- **Beyond the Model (2026):** tool registry and task-specific composition can help while unconditional context compression and extra subagents may regress certain models: https://arxiv.org/abs/2609.32459
- **An Empirical Study of Harness Design (2026):** planning/tool granularity interactions depend on model and task: https://arxiv.org/abs/2609.20804

**Design choice:** sub2 promotes real tool definitions after the search, rather than use a two-step search-and-call virtual dispatcher. This retains one native name for both provider and effect receipts, avoiding wrapper policy bypasses. It changes the provider tool schema array at a search boundary and may reduce prompt cache reuse for some providers. This tradeoff requires identical-task real-model benchmarks before declaring a net improvement.

## Reproduction (development profile only)

```sh
cargo test -p dominion-agent-kernel
cargo test --lib capability
cargo test --lib advertised_tool
```

Model comparison after local model or allowed API budget is available: freeze a task/model/provider/tool permissions/time and token cap, compare default full-catalog baseline to progressive projection for success, verified artifact, model rounds, provider input/output tokens (including uncached tokens), wall time and duplicate effects. Repeat for 4B/8B/27B and a cloud API where feasible; separate any setup/MCP inventory costs. **No such real-model result exists in this revision.**

No changes were made to main, sub3, sub4 or the existing React UI.
