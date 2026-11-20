# Sub3 integration boundary — 2026-11-20

## Ownership after the refactor

- **Agent kernel**: `crates/dominion-agent-sdk/` handles run lifecycle, plan/replan, tool admission, typed receipts, replay and acceptance. No MCP transport, message bot, OAuth flow or external account driver lives there.
- **Host engine**: `src/engine/` supplies model/provider and local workspace tools, chat/rooms, effect journal and runtime coordination. External account definitions, GitHub REST execution and channel transport startup were removed from `src/engine/tools.rs` and `src/engine/jobs.rs`.
- **Optional integrations**: `src/integrations/mcp.rs` owns the MCP transport, tool catalog, connection pool and cache; `src/integrations/channels/` owns Discord, Telegram, outbound adapters and delivery bridge; `src/integrations/accounts/` owns OAuth, GitHub, Google and account tool definitions/dispatch/availability.
- **Composition**: `src/bootstrap.rs` starts integration transports separately from core job workers. Shared MCP leases now live in `AppState::integrations.mcp_sessions`, so the core state machine does not own connector sessions.
- **Presentation**: `web/` and `src/api/` remain in place; no UI menu, saved connection configuration, room history or external API path is renamed.

Additional optional integrations: `src/integrations/browser.rs` owns Chrome DevTools browser control; `src/integrations/skills/` owns Skill installation, discovery and scoped retrieval. Their existing API routes remain stable.

## Runtime paths

```text
Web / SDK -> API -> Host engine planner/approval/effect journal
                        |                ^
                        | approved call  | result/receipt
                        v                |
                 integrations::{mcp,accounts,channels}
                  | external network/auth/outbox
                  v
            provider endpoint / MCP server / connected account
```

The host engine retains the final permission, effect-identity, cancellation and durable-receipt gates. A model-visible tool, a changed MCP registry entry or an OAuth connection is never a grant of execution permission. The account adapter returns results to the same tool-execution gate; it does not create a second agent runtime.

## Transition compatibility

The **source** aliases `engine::mcp`, `engine::discord`, `engine::telegram`, `engine::channel_adapter`, `engine::oauth`, `engine::github_files`, `engine::github_workflow`, and `engine::google_workspace` have been removed after updating callers. Existing network REST endpoints, persisted store object kinds and frontend routes are unchanged.

## Architectural limits and future boundary

This is a **real source/module and responsibility split inside one Rust binary**, not a separate hot-pluggable process, security sandbox, independent Cargo dependency graph, or dynamic plugin marketplace. Integrations still receive `AppState`, and the channel bridge still calls canonical rooms; replacing these with narrow traits and per-connector feature toggles is a further decoupling step, best done after trace-based contract parity. `engine::tools` remains the host admission/dispatch facade and intentionally invokes the account adapter rather than directly owning Google/GitHub requests.

MCP 2026-07-28 introduces stateless request metadata, discovery and cacheable list semantics, which are transport-level work and **not implied by relocating this module**. Reference: [MCP 2026-07-28 architecture](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/architecture/index.mdx).

## Checks

`scripts/integration-boundary.test.mjs` verifies module ownership, host composition and absence of legacy engine connector imports. It is added to the existing sub3 frontend contract job; native Rust tests and real remote connector behavior require separate runtime evidence. No new menu, production OAuth consent or payment operation is introduced here.
