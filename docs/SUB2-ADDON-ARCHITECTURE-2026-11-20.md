# Sub2: optional integration architecture (2026-11-20)

Branch: dominion-sub2. This is a source-level refactor, not a menu redesign or a production deployment.

## One runtime, optional extensions

~~~
React UI (existing page identifiers and navigation unchanged)
  -> guarded Axum core router: src/api/mod.rs
       |- core: models, agents, chat, rooms, jobs, SDK, memory
       '- addons: src/api/addons.rs (27 existing routes)
                |- MCP registry and external gateway
                |- OAuth, connected channels, webhooks
                '- browser and skill catalogs

One Rust execution kernel, single policy/approval/effect ledger
  -> engine::addons::{tool_definitions,requires_tool_approval,execute_tool}
       '- addons/mcp.rs (stdio and HTTP)
  -> native channel bridge -> addons/{discord,telegram,channel_adapter}
  -> optional source modules: addons/{browser,github_work,google_work}
~~~

- Kept all 27 external API paths and handler implementations, but moved registration out of the core API router.
- Kept shared core security middleware, CORS, maintenance guards, and HTTP listener. Addon routes never create parallel listeners or authority.
- Moved seven existing Rust implementation blobs losslessly into src/engine/addons/ with compatibility reexports, removing root-level duplicates.
- Native model/tool orchestration now depends on the addons interface, not on MCP transport methods. The existing dispatcher still owns permissions, cancellation, budgets and durable receipts.
- UI components, sidebar items, connections page and persisted settings have not been modified.
- The source audit checks both API routers for duplicate path shapes. A focused CI test prevents accidental reconnection of optional modules to the core root.

## Verification and limits

The boundary test checks source ownership and route presence; CI Rust tests check compilation/execution. This change does not claim live connector authorization, real-model token reductions or actual browser interaction. Extensions are logically and physically separated modules, not feature-gated Cargo crates or separate plugin processes.

Architecture reference: MCP 2026-07-28 host/client/server and transport separation, https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/docs/2026-07-28/learn/architecture.mdx
