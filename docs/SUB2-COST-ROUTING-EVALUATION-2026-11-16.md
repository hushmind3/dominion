# Sub2 — verified routing, per-provider tariff ledger, resource locks, video QA & native evaluation
**Branch:** `dominion-sub2` | **Date:** 2026-11-16

## Real user-facing workflow

- User / TypeScript SDK / API / GUI → `engine/router.rs` → `route_calibration.rs` → existing Rust provider/gateway and tool scheduler → persisted `execution` / `execution_operation` → independent acceptance evaluator → `chat/outcome.rs` → `route_observation`. The calibrated router is invoked only during *auto* selection, with at least 8 independent verified roots for **every** currently eligible candidate; explicit model selection and fallback retain existing behavior. Confidence-lower-bound ranking is not a guaranteed improvement.
- Existing Model dialog → `POST /api/providers` → validated `pricing` rate card → `model_cost::estimate` → `gateway::record_usage_tokens_cached` → SQLite audit `model.usage` → `Store::session_usage` / `Store::usage_summary`. Rates are owner-entered USD per million tokens; no rate is silently fabricated. Missing cache-read rate on a measured cache-hit call produces **unpriced**, not a full-rate false invoice. Estimated totals omit cache creation, GPU, server overhead, storage and provider adjustments.
- Agent tool execution `tools::execute` obtains resource-scoped process lock for shell, terminal, workspace modifications, browser and GitHub. Identical workspaces serialize mutation and shell commands, while read-only calls, other workspaces and delegation proceed concurrently. Process-local locks are not a distributed multi-host mutex.
- Video Studio → existing `POST /api/video/jobs` → Rust video engine → ComfyUI / FFmpeg → final mux → `video_quality::inspect` via `ffprobe` → persisted result receipt and existing Video preview. Real MP4 streams, codec, FPS, duration, and claimed audio/subtitle presence are checked. Meaning-level character consistency, aesthetics, lip-sync and cost-per-quality require independent VLM/human measurements; no perfect quality claim is made.
- Existing Activity page and SDK `evaluation.summary()` both call `GET /api/evaluation/summary`, reading the *same persisted* native root attempts. It reports independent-acceptance pass/fail, unverified, tokens, tool/model calls and elapsed time. Same model/tool/budget comparisons can be produced by `scripts/sub2-compare.mjs`, but require real exported paired task cohorts and a common `comparisonMeta`.

## Test and comparator protocol

`node --test scripts/sub2-compare.test.mjs`: identical task IDs, real verifier flags, same provider/model, and frozen `toolSchemaHash`/`maxRounds`/`maxTools`/`providerPolicy` comparisons. Unknown conditions yield `comparable:false` rather than invented deltas. Tests use synthetic fixtures **only to verify the comparator**, not as evidence that DOMINION beat another system.

`node --test scripts/sub2-architecture.test.mjs`: guards module boundaries and actual GUI/API/SDK route links.

Rust tests cover rate cards, correct treatment of unpriced cache hits, model-routing Wilson lower bound/duplicate roots, resource lock serialization, actual media ffprobe output shape and persisted root-attempt deduplication. GitHub Actions on Linux/macOS/Windows runs TypeScript/SDK/GUI, Node regression scripts, and `cargo test --release`. Success is counted only when the exact final commit's workflow completes.

### Open-source principles reviewed
- [OpenHands parallel executor](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/parallel_executor.py): resource-level locking while preserving independent concurrency
- [OpenHands benchmark harness](https://github.com/OpenHands/benchmarks): separate fixtures, verifiers and execution outcomes
- [Qwen Code provider configuration](https://github.com/QwenLM/qwen-code/blob/main/docs/users/configuration/model-providers.md): keep explicit provider configuration authoritative
- [LLMRouterBench (2026)](https://arxiv.org/abs/2601.07206), [TwinRouterBench (2026)](https://arxiv.org/abs/2605.18859): route quality must be measured on identical tasks and downstream agent steps rather than inferred from model names
- [2026 Stanford AI Index](https://hai.stanford.edu/assets/files/ai_index_report_2026_chapter_2_technical.pdf): media quality extends beyond container validity

### Evidence boundaries
Measured in CI only: unit and offline integration contracts. Source-connected but not yet live-measured: per-provider calibration and user-configured pricing. Not measured: frontier parity, real GPU throughput, video character continuity, external billing, end-user paid success, controlled same-model before/after completion and median token cost. No main branch mutation or UI menu redesign.
