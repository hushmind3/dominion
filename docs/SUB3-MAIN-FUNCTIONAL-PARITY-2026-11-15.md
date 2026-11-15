# DOMINION sub3: current-main feature parity and execution evidence

Date: 2026-11-15 (Asia/Seoul)
Main reference: `808505e66b053e6df1748dac0052851179dd7e06`
Integrated sub3 code: `cbaed6284aaa212ff0b14ca527ca033966310e87`

## Source and public contract

- **Main tracked files: 360/360 present in sub3**. Main-only 17 files imported, including `crates/harness`, main's optional TypeScript client, `src/api/sdk.rs` and docs.
- **Shipping web GUI: 56/56 blob-identical** between pinned main and sub3.
- **Public API routes: 210/210 preserved**. 204 handlers are byte-equivalent router expressions. Six original main `sdk::* ` handlers are accessible via `/api/harness/runs[/:id][/{resume,approve,reconcile,cancel}]`, and `/api/sdk/runs` still maps to sub3's real Rust native-agent jobs, retaining main-GUI compatibility. There are 217 total sub3 routes.
- Guard: `scripts/check-main-parity.mjs` is run by `.github/workflows/sub3-product-acceptance.yml`; reads the actual main branch and asserts files, UI blob equivalence, 210 route paths/handler mappings. A changed main requires an explicit rerun.

## Actual behavior reconnected

1. `crates/harness` and `src/api/sdk.rs`: standalone independent Rust planner, tool permissions, approvals, checkpoints and original main's optional TypeScript HTTP client, with separate host API. `src/api/sdk_runs.rs` remains the native Rust product GUI entry point.
2. `src/engine/chat/runtime.rs`: current main's model-in-flight checkpoints, Rust `dispatch_batch`, durable tool observations, read parallelism with write barriers, and deterministic single-file creation. The sub3 event/context/cost improvements remain.
3. `src/engine/verification.rs`: main Korean desktop-name inference, frozen artifact acceptance and creation-only verification, retaining sub3's `freeze_criteria` and rejecting read-only proof as proof of an actual write.
4. `src/engine/rsi.rs`, `rsi_evolve.rs`: Rust workspace crate source and untracked manifest snapshots into isolated experiments, offline Cargo metadata check, preserved sub3 source-integrity protections.
5. `src/engine/video.rs`: main native/ComfyUI and upload paths with sub3's dynamic scenes, Windows offline narration and strict H.264 media acceptance.

## Verified executions

[Focused product GitHub Actions #43722406603](https://github.com/hushmind3/dominion-rust/actions/runs/43722406603) **SUCCESS** for code commit `cbaed628`.

- Static parity verification checked 56 shipping web Git blobs and 210 main API route handler mappings.
- Rust backend `cargo check` passed.
- Chat 20/20 tests (including imported main's real mocked-HTTP → Rust kernel → two durable tool receipts) passed.
- Context-editing/Anthropic/receipt-preservation tests passed.
- Artifact verification, FFprobe-inspected real H.264 MP4 test, and focused HTTP/mock-model chat, rooms, tool, budget and delegation tests passed.
- Main single-file creation HTTP E2E: existing file requires **0** model calls; new empty file requires **1** model call without paid final summary; compound shell file creation proof all passed.
- `cargo test -p dominion-harness`: **26/26 passed**.
- Original main independent harness endpoint persisted a real Rust SDK run in the host test.
- Web application TypeScript typecheck and production build plus `packages/sdk-typescript` typecheck and build passed.

[Sub3 SDK/RSI CI #43721554704](https://github.com/hushmind3/dominion-rust/actions/runs/43721554704) **SUCCESS** for earlier work: SDK, RSI, independent frozen runner and related workflows.

## Verification limit

Source/route parity and the enumerated E2E assertions are evidence of concrete feature preservation, **not** proof every installed provider, OS, file-system permission, and all ~102 distinct HTTP E2E test cases have run. This parity check must be re-executed when main changes. The live paid-token-saving percentage and multi-generational long-running RSI performance remain separate acceptance measurements. No model-weight modification was added.
