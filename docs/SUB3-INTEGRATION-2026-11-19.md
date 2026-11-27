# DOMINION sub3 — frontend and backend integration audit (2026-11-19)

## Verified Git boundaries

- Repository: `hushmind3/dominion`; all writes scoped to `branch3`; `main` only inspected.
- Latest main comparison: `f86a36e1c8ddac149baea2ac2391d317add36cbe`.
- **Historical baseline:** `web/` sources originally matched main at SHA `fe65b961b037ad04e8ab4fc7f5c3d6e18abba392`. After this integration, sub3 adds a public-only staging environment example; React source implementations remain unchanged, but the complete subtree hash differs.
- Native Rust product tested at imported-source commit `33be0625cfd01dd1ba21bc25ce74f9daa68494e3`.
- Cloud Gateway tested on `5b973c0520e10225d81dc04b95c1b4a8fbc4a12f`; [8 cloud contract tests and D1 schema passed](https://github.com/hushmind3/dominion/actions/runs/44503096540).

## Main-derived API and runtime changes

- Added `GET /api/providers/catalog/:id/models` and `GET /api/providers/openrouter/free-models` to the sub3 backend for public model browsing without API keys. Model invocation still requires applicable authorization.
- Registered remote providers that expose no accessible `/models` route may fall back to a **metadata-only** models.dev catalog matched by exact provider API endpoint. Model prices unknown to the catalog remain unknown.
- Shared one 24-hour cached models.dev document across public company/model discovery to avoid duplicate cold network requests.
- Retained the sub3 Rust SDK kernel, durable receipts/replay, bounded 9-agent context and recognized llama.cpp KV cache use.
- From latest main source, adopted Codex WebSocket request-contract cache invalidation (model, tools, reasoning and policy controls) and truthfully persisted effective parallel execution mode; unverified/mutating parallel children serialize.

## Completed CI and boundaries

- [Sub3 Rust SDK — passed](https://github.com/hushmind3/dominion/actions/runs/44499399570), source `536447c` (later code change affects only a group-room regression assertion).
- [Sub3 product Rust + frontend + TypeScript SDK — passed](https://github.com/hushmind3/dominion/actions/runs/44499894159), source `affd870`.
- Expanded the product Rust suite with `engine::rooms::tests`, `api::providers::tests` and `api::cli_oauth::tests`, plus `dominion-sdk` Rust client tests in the SDK workflow. This caught a stale test assumption: a deliberately tiny conversation target must keep required owner/agent/tool evidence and report `room-required-evidence-over-soft-target`, not the older `native-work-context` value. The test now checks the actual policy, required-over-budget accounting and completed tool receipt.
- The obsolete source-wide forbidden-identifier scanner and its frontend counterpart were deleted. Scoped workflows exercise Rust execution, API, TypeScript and UI tests directly.
- These are automated tests, **not** end-user desktop launch on Windows/macOS, production OAuth payment certification, or an actual 9-person live 27B provider throughput test.
- `.github/workflows/ci.yml` is a separately dispatched full release gate and was not triggered.

## Source boundaries for independent review

- UI `web/src/App.tsx`, `web/src/pages/Models.tsx`, `web/src/components/ModelExplorer.tsx`, `web/src/components/ConnectedAccounts.tsx`, `web/src/components/DominionCloudAccount.tsx`.
- API `src/api/mod.rs`, `src/api/providers.rs`, `src/api/cli_oauth.rs`.
- Persistence `src/db.rs`; job admission and retiring legacy work `src/engine/jobs.rs`.
- Execution `crates/dominion-agent-sdk/src/lib.rs`, `src/engine/chat/codex_ws.rs`, `src/engine/chat/runtime/program.rs`, `src/engine/program.rs`.
- CI `.github/workflows/sub3-product-acceptance.yml`, `.github/workflows/sub3-rust-sdk-check.yml`.

No claims of complete automatic domain-general task success, charged-token savings, or absence of bugs outside these exercised boundaries.

## Cloud gateway import (2026-11-19)

- Imported `main/cloud/src/index.js` and `cloud/schema.sql` together with `services/dominion-cloud/gateway-contract.ts`; the worker implements verified OAuth entrypoints, D1 member sessions and limits, free-model SSE proxy, non-streaming paid-credit relay, identity verification and TossPayments confirmation.
- Staging `cloud/wrangler.toml` uses an unprovisioned **separate** `dominion-cloud-sub3` Worker and D1. Source imports do not modify the main Worker or its D1.
- Frontend already targets `<gateway>/v1` using the account session as a stored provider credential; no modifications to the Rust native chat/kernel are required for existing OpenAI-compatible requests.
- [Isolated Cloud Gateway source and tests](SUB3-CLOUD-GATEWAY-2026-11-19.md). No live OAuth, payment capture, paid streaming/tools, real-usage settlement or desktop SSO has been proven by the mocked CI.
