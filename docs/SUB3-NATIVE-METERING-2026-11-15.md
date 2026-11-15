# Sub3 — Provider-reported native agent token/cache metering (2026-11-15)

## User-facing execution path

```text
/api/jobs → jobs::run_prompt → native_agent::run
  → dominion_sdk::Kernel::run/resume
  → native_model::complete_with_run
  → native_stream::Assembler (SSE) OR native_codex_ws::generate (WebSocket)
  → native_model::reported_usage / record_job_usage
  → Store.job.nativeModelUsage + audit(native.model_usage)
  → GET /api/jobs/{id} → NativeAgentPanel / TypeScript Dominion Job type
```

The Rust SDK still owns decision-making, tool permissions, approvals, evidence and durable checkpoints. The provider adapter alone parses usage. No third-party agent runner or shadow execution loop was added.

## What is measured

For every **settled native model reply**, the engine records per-job aggregate input, output, provider cache reads and writes when the provider sends those numbers. The job's `nativeModelUsage` has `inputTokens`, `outputTokens`, `totalTokens`, `cacheReadTokens`, `cacheWriteTokens`, `reportedCalls`, `unreportedCalls`, `cacheReportedCalls`, `last` and `source`. A call with missing usage increases `unreportedCalls`: its unknown consumption is **excluded** from measured aggregates, not written as zero-cost. Only a still-running job can commit the new measured ledger. The SDK's independent `nativeState.chargedTokens` is the kernel's planning/budget counter and may estimate when usage is absent.

Provider protocol differences:

| Provider wire | Model input used for SDK budget | Prompt cache counters |
| --- | --- | --- |
| OpenAI-compatible Chat | `usage.prompt_tokens` | `usage.prompt_tokens_details.cached_tokens` when reported |
| Codex Responses (HTTP SSE and job WebSocket) | `usage.input_tokens` | `usage.input_tokens_details.cached_tokens` when reported |
| Anthropic Messages (HTTP and SSE) | `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` | Separate cache-read and cache-write counters |

Chat/Codex input **already includes** cached input. Anthropic excludes cache read/write from `input_tokens` and reports those counts separately. The adaptation corrects the input charged to the SDK's token budget for Anthropic. `native_stream::Assembler` retains cache statistics from partial SSE packets through its terminal response so cache measurements aren't silently discarded in streaming paths.

## Display, uncertainty and accuracy

`NativeAgentPanel` shows provider-reported input/output and cache counters, with a warning for calls that lacked usage. The TypeScript SDK exports `NativeModelUsage` and types the identical `Job.nativeModelUsage` field. Both read the persisted Rust job; neither runs a second model or builds a separate proxy ledger.

The transport may fail after submitting a request. Existing `native.codex_ws.uncertain` records possible unknown billing and prevents automatic replay. A provider that reports only one token metric is also incomplete; the remaining part is unknown. `native.model_usage.currencyCost:null` explicitly avoids inventing a local-currency price: price tables, subscription allocations, cache-tier rates and provider surcharges must be verified for the selected account and model before any monetary claim. Wire-payload size reductions do not equal actual token cost reductions.

For protocol comparison see [OpenAI 2026 Responses WebSocket mode](https://developers.openai.com/api/docs/guides/websocket-mode), [OpenAI 2026 Responses conversation state](https://developers.openai.com/api/docs/guides/conversation-state) and [Anthropic 2026 usage/caching references](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching).

## Tests and release scope

Focused Rust regression tests cover native SSE caching for Chat and Anthropic, provider-specific token parsing, a persistent two-call cache ledger, absence of provider usage, cancelled-job rejection, and an actual HTTP SSE tool-call path that commits measured usage to the job database. The independent TypeScript SDK now runs its own build/tests in the focused verification workflow in addition to the React GUI build.

No paid Codex/Anthropic credential call, pricing validation, full release CI or frontier agent benchmark is claimed.

## Verified focused CI result (2026-11-15)

[GitHub Actions #43691823640](https://github.com/hushmind3/dominion-rust/actions/runs/43691823640) succeeded on implementation commit `825409f66d8df01fa8799b3496baad62e8c08693`: Rust SDK **45**, native Rust host/agent/transport **54**, interactive Codex WS **15**, workflow recovery **1** — **115/115 Rust tests passed**. React TypeScript typecheck and Vite production build passed; the standalone TypeScript SDK compiled and all **7/7 Node tests passed**. The Rust native tests include the real HTTP-SSE → model adapter → job-store meter integration fixture, along with cache preservation and unknown usage. This does not measure monetary savings or validate commercial provider credentials.
