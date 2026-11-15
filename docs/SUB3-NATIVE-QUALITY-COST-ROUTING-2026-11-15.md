# Sub3 — quality-first, price-aware native Rust routing (2026-11-15)

## Real product flow

`ModelHub` provider modal → `POST /api/providers` → `Store.provider.nativeRouting` → `NativeAgentPanel` advanced routing policy → `POST /api/jobs` → `native_agent::run` → `native_routing::choose` → durable `job.nativeRoute` → independent `dominion_sdk::Kernel` → `native_model::complete_with_run` → provider-reported `nativeModelUsage` + KRW owner-rate cost projection → `GET /api/jobs/:id` → React + TypeScript SDK.

The Rust kernel remains the sole owner of its planner, tool contracts, approvals, scheduler, verification and checkpoint transitions. Routing is a **pre-run choice**, not a second agent loop or a covert model switch halfway through an execution.

## Owner-configured source of truth

Provider records use:

```json
{
  "model": "owner-verified-model-id",
  "nativeRouting": {
    "model": "owner-verified-model-id",
    "quality": 0.92,
    "pricesKRWPerMillion": {
      "input": 1200,
      "output": 5800,
      "cachedInput": 120,
      "cacheWrite": 1500
    }
  }
}
```

The values above are **illustrative**, not market claims or verified quotes. Prices are **KRW per million tokens**, configured by the owner per provider/model and validated to be finite/nonnegative. Missing quality or input/output tariffs never count as cheap. The model identity in the tariff record must match the actual configured model; changing the model in UI clears an existing profile. A provider can be explicitly selected without participating in optimization.

For a job, `input.routingMode = "quality_cost"` enables the selector, accompanied by `input.minQuality` (default 0.80) and `input.maxQualityDrop` (default 0.10). Automatic selection is allowed **only** when neither the owner nor the agent has pinned a provider/model. Qualified native-capable candidates must meet the minimum declared quality and remain within the allowed difference of the best declared quality. Disabled, text-incapable, CLI/legacy-compatible, subscription OAuth, credential-less cloud, over-context and unpriced routes cannot enter automatic candidate comparisons. When no qualified route exists, the job fails **before sending a billable model request**; it does not silently downgrade.

The selection is deterministic: lowest projected cost, then highest declared quality, then stable provider/model ID. Owner-declared `quality` is a **ranking constraint**, not a measured success rate. Real task acceptance remains in the independent Rust SDK verifier.

## Cache-aware forecasts and actual-use accounting

Candidate projections include a constant per-request input/tool allowance plus 2,048 output tokens. This is a **relative routing estimate**, not a promised usage or invoice. The model's actual context limit is still checked again at generation admission. With no trustworthy historical cache observations, the projection assumes full uncached input.

When at least **3 separate route-bound completed jobs** and 3 provider-reported cache samples exist, the model may forecast a partial cache read/write ratio (70% weight on the observed ratio, 30% uncached conservatism). Data from unreported calls, unrelated routes or missing cache rates never grants a discount. These observations never rewrite prices or authorize a model switch mid-job.

The chosen provider/model and **nonsecret route/contract fingerprint** are persisted on `job.nativeRoute` before its session/model is created. The fingerprint includes provider, model, kind, endpoint, account, protocol, context/output configuration, and a hashed credential digest. A crash/recovery or mid-run provider edit cannot redirect a previously approved paid call to a different model/account/endpoint. Such drift stops for manual review. Changing a provider's tariff mid-job does not affect the recorded price snapshot.

Each settled native model reply reads the provider token counters, computes `job.nativeModelUsage.configuredCostKRW` from the **pinned owner tariff**, and reports `pricedCalls` versus `unpricedCalls`. Cached reads and cache writes are billed at their separately provided rates. Missing cache tariffs or provider usage makes that call's KRW cost unknown rather than free. These amounts are **owner-rate × provider-reported tokens**, not receipt-verified provider invoices; never label them measured financial savings.

The automatic-cost selector does not import third-party agent harnesses, require a paid service, or mutate user-provided approvals or model input.

## Current-source references (2026-11-15)

- [OpenAI developer model prices and cache tariffs](https://developers.openai.com/api/docs/pricing): model/processing prices and cache-write tiers can differ materially.
- [Anthropic prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching): cache writes/reads have separate usage counters.
- [LiteLLM documented routing strategies](https://docs.litellm.ai/docs/router/learn/strategies): rate/busy/latency/cost groups; Dominion implements its own Rust quality-floor routing and kernel identity rather than embedding a proxy/router.
- [LiteLLM model groups](https://docs.litellm.ai/docs/router/learn/deployments_and_model_groups): explicit grouping and route affinity as references.

## Regression evidence

Focused host tests include: pure deterministic quality and cache-aware selection, rejection of unpriced and low-quality alternatives, exact explicit-provider precedence, manual price/identity matching, cache tariff missing-usage accounting, and a loopback native model call via `native_agent::run` that verifies its persisted provider choice, provider-reported usage, configured KRW amount, and rejection of changed-endpoint recovery. The TypeScript SDK/React build covers the same persisted job/API fields. There are **no paid-model price probes, live commercial invoices, automated benchmark quality calibration or production ROI claims** in this stage.
