# Sub3 — Lean agent execution architecture (2026-11-16)

Branch: `dominion-sub3` only. The current owner direction **supersedes** the former 100-point RSI/video-focused rubric and prior feature-growth guides.

## Actual removals

- `src/engine/rsi*.rs`: all standalone RSI research, optimizer, campaign, self-patch, generation and automatic acceptance loops removed physically. Ordinary source editing, source-hash protection, acceptance tests and owner-driven repair remain.
- `src/engine/video.rs`, `video_studio.rs`, `video_hub.rs`: built-in media generation, ComfyUI model download/installation, scene production, internal render/autopublish removed.
- `src/api/rsi.rs`, `src/api/video*.rs`, `src/api/benchmark.rs`, `src/api/experiments.rs`, `src/api/improvements.rs`: old feature/product/auto-experiment routes removed rather than remapped to non-working handlers.
- `web/src/pages/Video.tsx`, `SelfImprovement.tsx`, media-only subcomponents and memory growth dashboard are removed from source and navigation. `sdk/typescript` no longer declares RSI/media producer facades.
- RSI-only Rust tools and special automatic-write approval exception removed. CLI runner, periodic experiment scheduler, media smoke and standalone RSI benchmark GitHub workflows removed.
- Unused raster library `resvg` and its transitive dependencies removed from `Cargo.toml` and `Cargo.lock`.

## Retained critical capabilities

- Native Rust SDK and product adapter; existing chat SSE and rooms; files, memory, transcript history, corrections, job receipts, linked MCP/plugin channels, browser, OAuth, tools, local/remote models.
- Rust owner-approved edits, actual tool invocation, file SHA checks, tests, cancellation, step budgets, recovery and replay evidence.
- `jobs::retired_legacy_kind` refuses new old-media/RSI jobs. Recovering persisted old queued/running jobs cancels or marks review without executing side effects. Completed archived runs and user data remain stored.
- SQLite indexed active job fetch instead of scanning historic jobs every 400ms, Tokio Notify for immediate in-process enqueues, 5s durable catch-up for external writers.
- Default chat automatic learning disabled in Rust and UI; 5-minute growth/distillation/learning sweeps and post-chat self-experiment/strategy adoption disabled. Explicit owner-directed memory/procedure work stays available.

## Regression and release gate

`scripts/check-lean-product.mjs` asserts the **absence of removed source/routes/tools** and presence of retained core chat, memory, MCP, SDK, authorization, archived-job safety, indexed workers and nonmedia dependencies.

`.github/workflows/ci.yml`: Linux, macOS, Windows compile and release tests; TypeScript GUI and SDK; lean architectural gate. `sub3-product-acceptance.yml` and `sub3-rust-sdk-check.yml`: real agent, chat, recover, immutable tool evidence, API and provider regressions. **A green workflow belongs only to the exact HEAD it tested.**

Historical SQLite/job content is not deleted; existing audio/video files in a user's data directory are not removed by upgrading this source branch. They are simply no longer created by the built-in engine.

## Scope of future external media

Media can be done via separately installed and authorized MCP or plugin tools. No paid media provider is hardcoded, no local weight download occurs at startup, and no media-only CPU or GPU worker exists in the Rust product.
