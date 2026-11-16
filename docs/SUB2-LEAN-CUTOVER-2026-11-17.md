# Sub2 lean retirement — 2026-11-17

**Branch:** `dominion-sub2` only. No merge to `main`.

## Current runtime contract

- Rust/SDK/GUI execute user-assigned work through persisted jobs, tools, approvals, budgets, cancellation and verifiable outputs.
- Built-in video/audio rendering, YouTube upload execution, autonomous RSI generations, auto-adoption and source-mutating workers are retired. Existing jobs that reference those kinds become owner-review outcomes instead of restarting risky effects.
- Read-only legacy `/api/video/*` and `/api/rsi/*` endpoints and SDK reads preserve historical job records, MP4 downloads and research records. They are not re-enabled by this cutover.
- New media creation is an explicit owner-requested MCP/Connections operation. Ordinary learning and correction memory remain available only when requested or opted into.

## Final removal pass

- Removed startup calls that queued improvement experiments, generated automatic policy candidates and ran optimizer meta-improvement on process restart.
- Removed the obsolete React RSI issue/notification component and the chat error button that posted into its retired endpoint.
- Narrowed `PageId` to currently navigable pages while preserving the main UI's layout, conversation state and settings.
- Removed defunct sub3 generational RSI test workflows, live-model campaign driver and generation-worker fixture from this branch. Historical research/evaluation material remains archived in `docs/`, `benchmark/` and `rsi/evaluations/`.
- Aligned English/Korean README and engine documentation to the read-only archival media/RSI contract.

## Verification and deferred CI

`scripts/sub2-lean.test.mjs` statically protects the startup contract, removed UI/workflows and read-only history. The connector-based audit confirms removed calls in `src/sdk.rs` and the absence of retired executable entrypoints in the new Git tree. **Full CI, platform builds, real-model task completion and runtime performance measurements are not claimed**; run them after the 1–3 pass as directed.

## Preserved data and ABI

No SQLite migration, stored asset rewrite or destructive data operation is performed. Historical `video`, `youtube_upload` and RSI job kinds remain recognizable for safe read/recovery handling, and historical API reads remain stable for existing clients.
