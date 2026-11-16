# Sub3 lean architecture judge evidence (2026-11-16)

The 2026-11-16 user decision retired native video/audio and independent RSI. **Old scoring rules awarding native video and RSI points are obsolete for this architecture**. Evaluate the lean product by actual task completion, security, agent execution kernel, memory, tool permission handling, MCP/plugin integration, cancellation, recovery, token and CPU waste, SDK consistency, and preserved archival data.

## Source anchors

- Core native SDK: `crates/dominion-agent-sdk/src/lib.rs`, `schema.rs`, `tool_discovery.rs`, `replay.rs`.
- Real product path: `src/engine/native_agent.rs`, `src/engine/chat/runtime.rs`, `src/engine/jobs.rs`.
- SQLite queue and indexed cost history: `src/db.rs`, `src/engine/native_routing.rs`.
- Owner permissions and pinned MCP manifest: `src/engine/chat/approval.rs`, `src/engine/mcp.rs`, `src/engine/tools.rs`.
- Live public API and TypeScript SDK: `src/api/mod.rs`, `src/api/sdk_runs.rs`, `src/api/native_replay.rs`, `sdk/typescript/src/index.ts`, `web/src/components/NativeAgentPanel.tsx`.
- Physical removal and retention contract: `scripts/check-lean-product.mjs`, `docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md`.

## Evidence constraints

Code paths and deterministic tests prove limited behaviors (correct indexing, no runtime RSI campaign, no native media route, old job retirement, typed SDK, replay receipt contract, tool argument boundaries). They **do not prove** frontier-level real model task completion or actual API invoice reductions. Use recorded provider usage and owner-configured **KRW** tariffs separately from invoiced cost. Assess all installed MCP connectors with actual auth/permissions and cancellation; never substitute deleted media modules just to pass an outdated parity test.

Run all available CI workflows on **the same HEAD**, and compare actual benchmark tasks when available. Read-only replay reconstructs historical decisions without re-invoking models or proving current-world artifact acceptance. No automatic winner selection or code adoption occurs.
