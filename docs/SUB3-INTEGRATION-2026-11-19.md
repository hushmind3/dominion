# DOMINION sub3 — frontend and backend integration audit (2026-11-19)

## Verified Git boundaries

- Repository: `hushmind3/dominion-rust`; all writes scoped to `dominion-sub3`; `main` only inspected.
- Latest main comparison: `f86a36e1c8ddac149baea2ac2391d317add36cbe`.
- **Identical `web/` subtree SHA**: `fe65b961b037ad04e8ab4fc7f5c3d6e18abba392` in both main and sub3 at time of audit. This includes React/Vite sources, dependencies, lockfile, public OAuth client environment, model exploration, connected accounts and regression tests.
- Tested sub3 code commit: `ed70b2efa0581d94a1d52e35eb057043a3917223`.

## Money domain retirement (safe archival behavior)

Removed the built-in Money research/earning menu, public routes, engine, database feature modules, incremental SQL schema initializer, plugin scheduler, CI Money workflow and active integration docs. The job worker no longer starts the collector, schedules maintenance or dispatches Money tool runners. Deprecated Money job kinds are rejected and marked for review rather than dispatched. **Existing SQLite user records and historical receipts are preserved**; this is source removal, not a destructive database migration.

The source-inventory and architecture guard in `scripts/check-lean-product.mjs` rejects executable Money modules/routes and validates key frontend/backend contract surfaces.

## Main-derived API and runtime changes

- Added `GET /api/providers/catalog/:id/models` and `GET /api/providers/openrouter/free-models` to the sub3 backend for public model browsing without API keys. Model invocation still requires applicable authorization.
- Registered remote providers that expose no accessible `/models` route may fall back to a **metadata-only** models.dev catalog matched by exact provider API endpoint. Model prices unknown to the catalog remain unknown.
- Shared one 24-hour cached models.dev document across public company/model discovery to avoid duplicate cold network requests.
- Retained the sub3 Rust SDK kernel, durable receipts/replay, bounded 9-agent context and recognized llama.cpp KV cache use.
- From latest main source, adopted Codex WebSocket request-contract cache invalidation (model, tools, reasoning and policy controls) and truthfully persisted effective parallel execution mode; unverified/mutating parallel children serialize.

## Completed CI and boundaries

- [Sub3 Rust SDK tests — passed](https://github.com/hushmind3/dominion-rust/actions/runs/44483266171) on the tested source commit.
- [Sub3 product Rust + frontend tests — passed](https://github.com/hushmind3/dominion-rust/actions/runs/44483266148) on the tested source commit. Frontend job runs npm install, TypeScript checker, all `web/tests/*.test.mjs` and Vite production build. Rust job covers SDK/room/provider/repair fixtures and a native job e2e.
- These are automated tests, **not** end-user desktop launch on Windows/macOS, production OAuth payment certification, or an actual 9-person live 27B provider throughput test.
- `.github/workflows/ci.yml` is a separately dispatched full release gate and was not triggered.

## Source boundaries for independent review

- UI `web/src/App.tsx`, `web/src/pages/Models.tsx`, `web/src/components/ModelExplorer.tsx`, `web/src/components/ConnectedAccounts.tsx`, `web/src/components/DominionCloudAccount.tsx`.
- API `src/api/mod.rs`, `src/api/providers.rs`, `src/api/cli_oauth.rs`.
- Persistence `src/db.rs`; job admission and retiring legacy work `src/engine/jobs.rs`.
- Execution `crates/dominion-agent-sdk/src/lib.rs`, `src/engine/chat/codex_ws.rs`, `src/engine/chat/runtime/program.rs`, `src/engine/program.rs`.
- CI `.github/workflows/sub3-product-acceptance.yml`, `.github/workflows/sub3-rust-sdk-check.yml`, `scripts/check-lean-product.mjs`.

No claims of complete automatic domain-general task success, charged-token savings, or absence of bugs outside these exercised boundaries.
