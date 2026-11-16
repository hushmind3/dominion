# Dominion Sub3 — independent judge evidence (2026-11-16)

**Scope:** `hushmind3/dominion-rust`, branch **`dominion-sub3` only**. The existing main UX, menu structure and authentication continue to route through the same Rust product. No main, sub2, sub4 or money branch was modified.

## User goal and actual wiring

The request was to finish four improvements before comparison and scoring: (1) pinned MCP tool contracts, (2) tool argument validation and evidence provenance, (3) indexed model-cost history, and (4) replay of failures and before/after measurement.

| Evaluation issue | Actual execution contract and code |
|---|---|
| MCP tool manifest changes or server name collision | `src/engine/mcp.rs::native_contract_fingerprint` hashes enabled server configuration + exact model-reviewed tool schema/description. `src/engine/native_agent.rs` saves `nativeMcpContracts` in the real durable job. After owner approval and immediately before each external MCP action, `mcp::call_pinned` **re-fetches tools/list in the same initialized session as tools/call** and stops on any missing, duplicate or changed tool. A resumed job refuses reconfigured pins. Legacy interactive MCP API remains intact. |
| LLM cannot turn an unverified argument into host authority | `crates/dominion-agent-sdk/src/schema.rs::validate_arguments` bounds every tool argument to 256 KiB and 12 schema levels, validates required keys, types, enums, const, nested objects, arrays, numeric bounds and combinators; unsupported high-impact constraints fail closed. `crates/dominion-agent-sdk/src/lib.rs` applies this to the **host-approved** ToolSpec immediately before every actual dispatch; `tool_discovery.rs` still restricts calls to previously discovered and owner-authorized names. |
| Provenance of external effects | `NativeTools::execute` writes an `execution_operation` **before** side effects, including trusted origin, exact arguments SHA-256, optional pinned MCP contract SHA-256; successful responses store `resultSha256`. Reconciliation rules still prohibit blindly replaying unknown external effects. Tool-result text is marked as lower-trust evidence, not as instructions. |
| Provider cache usage at large DB sizes | `src/db.rs::native_route_cache_aggregate` uses a SQLite partial expression index `idx_native_job_route_completed`. `src/engine/native_routing.rs::estimate_with_history` no longer materializes all jobs separately for each provider. Only completed jobs with same frozen route ID and provider-reportable cache tokens enter the aggregate. Unknown billing is **not free**. |
| Failure investigation without new paid calls | `crates/dominion-agent-sdk/src/lib.rs::Snapshot.decision_tape` captures the actual model decision **before** dispatch, alongside prior durable host receipts. `crates/dominion-agent-sdk/src/replay.rs::trace` reconstructs decisions, steps, failures, dependencies, missing/duplicate action identifiers, uncertainty from this persisted tape. Does **not** rerun model, tests, browser or irreversible tools. Legacy snapshots deserialize safely and report missing tape. |
| Compare cost and verification at identical work shape | `src/engine/native_replay.rs::report/compare` joins the snapshot with independent `execution_operation` records, tests argument/target/result/contract hashes, surfaces unobserved actions. Pair scoring requires same prompt, verification criteria, model/provider, limits and tool permissions. Reports verification comparability, number of model/tool calls, token count, owner-configured KRW cost and elapsed time when both are actually measured. It does **not** falsely claim real API-invoice certainty or automatically promote a revision. |
| One actual product across SDK/API/GUI | `src/api/native_replay.rs` exposes `GET /api/jobs/:id/replay`, `POST /api/jobs/:id/compare`, and matching `/api/sdk/runs/:id/*` routes, **read-only**. `sdk/typescript/src/index.ts` adds typed `jobs.replay/compare` and `nativeRuns.replay/compare`. `web/src/components/NativeAgentPanel.tsx` adds a compact expanded section within existing work card: inspect offline replay and compare two runs; no redesigned menus. |

### Main GUI/API compatibility

The main branch added `POST /api/video/jobs/:id/regenerate` during this iteration. Sub3 restored the **real** endpoint in `src/api/video.rs` and `src/engine/video.rs::enqueue_regeneration`, keeping existing clip fingerprint/SHA reuse. Derivative jobs preserve original seed, source and settings, change the requested scene, disable autopublishing and leave the source job unchanged. A new unit regression confirms this. The parity workflow checks main routes and source layout; this is no fake compatibility handler.

### Test and production artifacts

- `crates/dominion-agent-sdk/src/schema.rs`: nested required fields, enum, numeric and size rules, and fail-closed unsupported schemas.
- `src/engine/mcp.rs`: changed schema/description, duplicate live tool names, endpoint reconfiguration and server slug collision tests.
- `src/db.rs`: SQL aggregates over 150 mixed-route records, rejects incomplete usage; `EXPLAIN QUERY PLAN` asserts the actual new index.
- `crates/dominion-agent-sdk/src/replay.rs`: failed tool reproduction, tamper detection, uncertain-effect rejection, and older checkpoint compatibility.
- `src/engine/native_replay.rs`, `src/api/native_replay.rs`, `sdk/typescript/tests/sdk.test.mjs`: read-only forensic report, mismatched paired-run refusal, no mutation, correct typed API paths, JSON contract.
- `src/engine/video.rs`: main scene-regeneration parity, frozen original, derivative video, no autopublish.
- GitHub Actions workflows: [cross-platform CI](https://github.com/hushmind3/dominion-rust/actions/workflows/ci.yml), [Sub3 focused SDK](https://github.com/hushmind3/dominion-rust/actions), [Sub3 product acceptance](https://github.com/hushmind3/dominion-rust/actions). **Check the exact branch HEAD, failed steps and subsequent fixed commits before grading.** Earlier cancelled/red runs during iterative development are not evidence about the latest code.

### Source research and independent engineering

Read and compared public 2026-era source implementations and published designs including:

- [Qwen Code's executable Tool Search](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-search.ts), [tests](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-search.test.ts), and [fixed-prefix discovery design](https://github.com/QwenLM/qwen-code/blob/main/docs/design/toolsearch-preload-threshold.md) — search cannot silently mutate the provider tool prefix or privilege model suggestions over the host registry.
- [Model Context Protocol specification](https://modelcontextprotocol.io/specification/2025-06-18) — the tools/list and tools/call contracts and lower-trust tool descriptions.
- [OpenHands software agent SDK](https://github.com/OpenHands/software-agent-sdk) — separation of host executor, durable traces, observed steps, and evaluation boundaries.
- [VeriHarness](https://github.com/google-research/veriharness) — evidence-based independent challenge vs mere model-reported success.

The implementation is independently authored Rust, uses the existing platform job store/authorization and TypeScript public SDK, and introduces **no external imported agent execution loop or LoRA/model-weight training**.

## Judge scoring requirements

Use the project's **current official weights** in `공통지침.md`:
`25 / 15 / 10 / 10 / 5 / 5 / 5 / 5 / 10 / 10 = 100`, in order agent harness, engine/RSI, video, token costs, multi-model groups, plugins/MCP, provider, UX, architecture, operational runtime.

The judge should independently compare current `main`, `dominion-sub2`, `dominion-sub3`, `dominion-sub4`, and confirm all available artifacts, CI and real end-to-end calls. The team does **not** assign itself a score.

- **Code-supported**: code paths and regression tests above, session-local MCP manifest pinning, durable zero-paid-call event reconstruction and indexed provider history.
- **Measured by deterministic tests**: parser/schema failures, routing SQL counts, main endpoint parity, trace mismatch detection and mock model/host replay integrity.
- **Not yet measured as equal-to-frontier**: SWE-bench/Terminal-Bench task completion with the same model/compute, actual real-provider token savings, video visual quality, cost/throughput percent improvement, and operator-run production cancellation. The offline trace reconstructs decisions but is not a new real-world execution; cost is based on owner tariffs and provider reports, **not invoices**.
- **Security and backward compatibility**: fail-closed unknown tool schemas may make some unusual external MCP tools unusable until their schema constraints are supported; this is an intentional explicit limit, not a claim of universal schema coverage.

The ideal result is an independent 100-point score with per-category evidence and a concise list of the next critical shortfalls. Do not apply money-branch obligations to sub3.
