# Dominion harness boundary — 2026-11-18

## Decision

**One native Rust execution driver; many interchangeable tools, strategies and external runtimes.** The primary engine remains `dominion_core::engine::Engine::drive` in `crates/dominion-core/src/engine.rs`. Its single durable budget/checkpoint/approval/effect chain must not be replaced or independently duplicated by an SDK, plugin, Money feature, video feature or optional external harness.

Branch comparison is selective, not wholesale. The `dominion-sub3` discovery/search approach inspired the capability interface, but the independent `dominion-agent-sdk::Kernel::run/resume` loop is **not imported**. The `dominion-sub2` decision helpers and the standalone `dominion-sub4` engine remain isolated evaluation sources. The `money` branch does not own the primary agent loop.

## Implemented on main

- `src/engine/capability.rs`: stable `dominion_tool_search` and `dominion_tool_call` contracts; deterministic, bounded capability retrieval from actual enabled MCP and native tools; schema returned only for hits.
- `src/engine/chat/runtime/tool_catalog.rs`: shortlist for large inventories while keeping the stable search/call bridge visible; never dump every unrelated MCP schema into a model request.
- `src/engine/chat/runtime/tool_dispatch.rs`: search is read-only. `dominion_tool_call` requires a successfully persisted discovery operation from the **same execution and session**; resolved calls pass through the original Rust tool gateway (real tool name, effect ledger, approval, result receipt, sandbox and cancellation checks). A failed resolution stays a normal tool failure rather than executing the requested name.
- `src/engine/chat/runtime.rs`, `src/engine/chat/product.rs`: original `dominion-core` model/tool loop stays in control, including after owner steering. A successful discovered mutation invalidates cached environment state.
- Focused tests exercise shortlist bounds, discovery receipt scoping, schema validation, read-only restrictions, and the end-to-end native gateway path.

**Not implemented:** Installing Codex, Hermes, OpenCode or any external agent; ACP runtime interoperability; a new plugin marketplace; model-size-based auto-configuration; a new kernel. Those need distinct compatibility and benchmark evidence before adopting them.

## Extension / external runtime contract (future interoperability)

A future `ExternalRuntime` adapter must be a **subtask worker**, not a second controller of the parent execution:
- identify `runtime_id`, supported protocol and capabilities without granting new permissions;
- `start(task, session_scope, budget_lease, workspace_scope)` returns a durable external ID;
- `status/poll(external_id)` returns progress and structured receipts;
- `cancel(external_id, generation)` and `reconcile(external_id)` avoid duplicate writes or unconfirmed work;
- outputs must carry artifacts, verification evidence, cost accounting, and pending-effects metadata;
- all external effects are subject to the owner's ordinary approval and workspace boundaries.

Do not rename an inference-only CLI adapter into an external harness. `src/engine/cli_model.rs` currently uses official CLIs as text generators, **not** independent executors.

## Model- and task-aware benchmark plan

Before changing the model's default tool exposure, compare at least:
1. native structured tools (small fixed set);
2. the same set plus progressive search/call;
3. shell-first, minimum loop (coding / terminal tasks only);
4. explicit planning on/off with exactly the same tool and context budgets.

Run on local 4B and 27B and an available stronger model with the same tasks and acceptance checks. Track *verified task success*, calls, prompt/tool-schema tokens, elapsed runtime, duplicate-effects, context-overflow and recovery completion. A bigger model does not automatically earn unrestricted permissions. Selection is based on measured behavior, not parameter count alone.

## Public reference implementations

- Pi minimal agent + extensions: https://github.com/badlogic/pi-mono
- mini-SWE-agent's shell-centric loop: https://github.com/SWE-agent/mini-swe-agent
- DeepSeek Harness Cordis plugin composition: https://github.com/deepseek-ai/deepseek-harness
- Goose MCP extensions and recipes: https://github.com/block/goose
- OpenHands Software Agent SDK, local/isolated runtime: https://github.com/OpenHands/software-agent-sdk
- OpenCode service, sessions and SDK: https://github.com/anomalyco/opencode
- SWE-ReX environment abstraction: https://github.com/SWE-agent/swe-rex
- Empirical harness component ablation (four models, 176 matched conditions): https://arxiv.org/abs/2609.20804
- Source-code study of eleven coding harnesses: https://arxiv.org/abs/2609.00006

These are comparative references, not bundled dependencies or performance guarantees.
