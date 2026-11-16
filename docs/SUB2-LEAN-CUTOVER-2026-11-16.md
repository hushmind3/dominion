# Sub2 — Lean-core transition plan (decision 2026-11-16)

## Product contract superseding previous branch brief
The user decided to **retire permanently embedded video/audio generators and the independent RSI research/evolution system**. No moving automatic generation search, source selection, model-graded core mutation or autonomous campaign adoption into the SDK. The Rust harness must focus exclusively on operator-requested execution: planning, code/file/web/terminal/browser/MCP tools, approvals, budget, parallel delegation, cancellation, resume, deterministic verification and durable correction/procedure memory.

Existing conversations, user memories, file and output history, credentials and agent records are preserved. Optional media tasks are delegated through the Connections/MCP surface without engine-specific generation packages. Source-changing algorithm/harness improvements remain owner-directed development only.

## Applied stage: runtime waste and new UI defaults
1. Workers subscribe to the existing durable `Bus` before scanning the indexed active-job subset. A queued Job event wakes all workers; SQLite CAS keeps one job owner. A 5-second watchdog catches externally inserted/availableAt delayed/restarted work. Old completed and cancelled job records remain untouched in the same SQLite database.
2. `Store::active_jobs` selects only queued/running records. The partial `idx_job_status` index is created with `IF NOT EXISTS` for new and existing databases. `jobs::next_queued` retains the same learning/room concurrency, priority, availability and CAS checks.
3. Removed the default 5-minute background sweeps of `agent_growth`, `distill`, learning requeue and memory learning. Manual explicit chat correction/memory and code evaluation paths remain compiled.
4. Default chat/room auto-learning set false across Rust request parser, room save defaults and browser controls. Previously saved explicit values/records are preserved, not rewritten.
5. Removed the embedded Video and SelfImprovement page imports and menu navigation. Existing user records, MP4 files and old APIs/engines are not deleted by the transition.

## Retired live entrances (stage 2)
- HTTP mutating endpoints for RSI campaigns, worktree adoption, built-in video models/rendering and video publishing return HTTP 410. Legacy GET paths remain for read-only job and output inspection.
- The normal Rust worker refuses `rsi_evolve`, `rsi_compare`, `video` and `youtube_upload` jobs; previously queued jobs are preserved as terminal `needs_review` records when claimed, never silently submitted. The former `rsiApplyWhenFinished` installation branch has been removed.

## Stage 3 — source and dependency removal

- Removed 23 separate `src/engine/rsi*.rs` research/generation/worktree files, native `src/engine/video.rs` and `video_quality.rs`, their `engine/mod.rs` exports, the embedded Video/SelfImprovement pages and 5 RSI lab scripts.
- Removed former `dev::start_rsi_install`, source promotion/rebuild/restart, RSI-only autonomous prompt controller and extra tool descriptors. Native source edits still use ordinary code tools, owner approval and frozen verifier evidence.
- Replaced `api/rsi.rs` and `api/video.rs` with read-only archive adapters. Old MP4 paths remain sandboxed under their original job IDs; outgoing creation and publishing return HTTP 410. TypeScript SDK removes all built-in creation/install/adoption mutations and retains only legacy retrieval.
- Removed the unused `resvg` direct dependency and pruned its 51 orphaned lockfile packages. No archived SQLite tables, file contents or migration histories were deleted.
- Routine job completion no longer schedules algorithm-search experiments or policy meta-improvement.

## Stage 4 — verification

- CI checks TypeScript web type safety, TypeScript SDK contract tests, lean/static architecture guards, browser build, and `cargo test --release` on Linux, macOS and Windows.
- Separate Rust tests cover durable job lifecycle, real tool approval, crash/restart behavior, shared session context, library archive reads and MCP. The exact final CI SHA is authoritative. Real model end-to-end performance is not inferred from synthetic fixtures.
- Trace all `src/engine/{rsi*,video*}` references in `jobs.rs`, `chat/runtime.rs`, SDKs, API routes, tool definitions, build scripts and web navigation. Remove modules topologically only once normal chat, room, task execution and MCP contracts are verified.
- Separate manual code editing/evaluation and verified memory/procedure reuse from RSI-specific campaign semantics.
- Verify a genuine chat turn with tool calls, file editing, approvals, interruptions and restart on Windows/Linux/macOS against existing database fixtures and API contract tests.
- Rewrite scorecard to prioritize task completion, context/token budget, durable work, MCP/provider integration and codebase simplicity; legacy RSI/video point floors no longer match the product.

## Limits / evidence
Source/module deletion is complete for the built-in media and RSI research paths, retaining historical API/DB records and manual code tools. The 5-second watchdog is still required for delayed jobs and external writers. Real CPU savings need profiling. Past explicitly enabled autoLearning settings remain respected. CI claims require the exact final-commit run; only a completed all-OS run counts as a release pass.
