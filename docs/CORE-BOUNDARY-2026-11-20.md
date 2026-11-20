# Sub3 minimal language-to-action core — 2026-11-20

## Functional boundary

Owner utterance → model inference → authorized action selection → effect execution → observed result → next model turn/final answer. A minimal engine **still must** retain approval/effect identity, cancellation, durable journal and verification to avoid replaying unknown writes.

The independent `crates/dominion-agent-sdk/` contains typed model/tool/evidence/journal ports and the native agent loop. The Rust product host `src/engine/` is now a reduced composition adapter with 10 top-level module declarations. Its 10 modules contain the native agent execution adapter, goal/run state, job execution, evidence/verification and recovery. The workspace/file/shell driver implementation now lives at `src/integrations/local_tools.rs` and is called through the existing SDK Tools host port. The conversation service's streaming, per-room context and chat provider adapters are separately owned by `src/conversation/chat/`.

| Ownership | Path | Former engine modules |
| --- | --- | ---: |
| Cognition, memory and learning | `src/features/cognition` | 16 |
| Multi-agent rooms and personas | `src/features/collaboration` | 5 |
| Provider models, routing and transport | `src/features/providers` | 12 |
| Product management, backup, indexing | `src/features/platform` | 10 |
| Optional forensic review | `src/features/forensics` | 2 |
| External MCP, channels, accounts, browser, Skills, public HTTP | `src/integrations` | previously separated |

Total: **45 relocated top-level modules** (46 files including adaptation types).

## Conversation is a host surface, not the kernel

The former `src/engine/chat/` implementation (27 Rust files) now lives in `src/conversation/chat/`. The engine retains a source compatibility re-export so existing API/SDK entry points do not change. It is **not** removed: multi-agent rooms, streaming chat and session history still use this product service around the same executable kernel.

## Composition

- `bootstrap.rs` boots the executor independently of `features::start` and `integrations::start`; feature recovery, maintenance and optional algorithms are coordinated outside the core worker.
- `engine/jobs.rs` owns only `prompt` and `resume`; rooms, learning and reflection are handled by `features::dispatch_job`; Discord by `integrations::registry::dispatch_job`.
- `integrations/local_tools.rs` declares and implements workspace/file/shell execution and delegates feature/tool declarations through `features::registry` and `integrations::registry`. Memory tools move to `features/cognition/tools.rs`, web search/HTTP transport to `integrations/network.rs`.
- Existing approvals, user-selected permissions, MCP schema checks, durable receipts, tool identifiers, SQLite data shapes and HTTP/UI routing are preserved.
- The 2026 comparison target is a minimal agent-loop spine with service/provider/feature plugins around it, as in [DeepSeek Harness official architecture](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md) and [Qwen Code architecture](https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md). This refactor is independently authored Rust, **not** importing those runtimes.

## Native external-tool port

The native SDK host adapter no longer imports the MCP transport module. It receives typed `ToolSpec` descriptors and pinned external contracts from `src/integrations/registry.rs`, which owns MCP discovery, identity validation, approval classification and transport execution. The durable `nativeMcpContracts` field remains unchanged for stored-run compatibility.

## Known remaining seams

- `src/engine/mod.rs` has five grouped compatibility re-exports of the moved modules. Rust callers outside the feature directories still reference historical `crate::engine::memory`, etc. They preserve compile-time source compatibility while call-site migration continues. Reexports do **not** copy implementation back to the engine.
- Host `AppState` is a shared concrete application context. No separately unloadable plugin container, standalone Cargo crates per feature, binary-size savings measurement, or dynamic isolation is asserted.
- Existing chat/room context features and optional provider decisions still execute when configured. Making them independently toggleable with narrow traits is distinct from this module and dispatch separation.
- Product CI includes Rust compiler/unit tests, a source-boundary test, and frontend contracts. Verify each head run; no 27B-model accuracy or paid token claims follow from file movement.

## Regression protection

`scripts/core-boundary.test.mjs` is part of the sub3 product CI and asserts the 10 core declarations, 45 relocated optional modules, and registry-based dispatch and execution. `scripts/integration-boundary.test.mjs` continues to police external connector ownership. Focused Rust CI filters were changed to follow the relocated test modules; avoiding 0-test false positives is part of the refactor.
