# Sub3 — Native provider prompt cache and replay-safe HTTP (2026-11-15)

## Production execution path

NativeAgentPanel → POST /api/jobs → jobs::run_prompt → native_agent::run → dominion_sdk::Kernel::{run,resume} → ConfiguredModel::complete_with_tools → native_model::complete_with_run → native_cache / gateway::send_once → native_stream::Assembler → kernel approval and durable receipts.

Dominion Rust remains the sole owner of planning, tool dispatch, authorization, verification and checkpoints. There is no imported agent harness.

## Previously observed gap

The Codex WebSocket transport already refused to replay uncertain request delivery, but the native OpenAI-compatible HTTP path called gateway::send, whose best-effort retry could send a second paid POST 400 milliseconds after an ambiguous network error. This bypassed the kernel's model_in_flight write-ahead guard. Separately, native Anthropic Messages returned cache usage statistics without requesting explicit reusable prompt prefix caching.

## Implemented behavior

1. Durable native Chat Completions use gateway::send_once: the same request authentication, benchmark route checking, model rate admission and concurrency guard, but exactly one HTTP request dispatch. A socket drop before the first delta remains an unknown paid call and may not be replayed. The generic helper/chat gateway::send remains backward compatible.

2. Native Anthropic Messages now place 5-minute ephemeral cache_control breakpoints after the stable tools list and system prompt, when the combined prefix exceeds a heuristic 1,024-token threshold. Owner instructions and dynamic task observations remain outside explicit cache write breakpoints. The prefix must be byte-for-byte stable for real cache hits.

3. Native prompt cache hints apply to non-local Anthropic providers automatically. Local Anthropic-compatible models must opt in with provider capabilities.promptCaching=true; capabilities.promptCaching=false or capabilities.promptCache=false always disables the hints.

4. Only a provider's explicit 400/422 indicating unsupported cache_control allows one compatibility retry using the identical original payload without hints. Generic 400, authentication errors, quota, 5xx, timeout, EOF, partial SSE, and stream errors never authorize replay. Rejection is remembered under provider_wire_feature for 24 hours, isolated by provider ID, model, base URL, kind and account (not API secret).

5. native.prompt_cache_hint and native.prompt_cache_compat_retry audits distinguish requested hints from actual cache hits. The existing nativeModelUsage ledger records provider-reported cacheReadTokens and cacheWriteTokens. Neither accepted hints nor smaller wire payloads are proof of a lower bill.

## 2026 implementation references

- Anthropic prompt caching: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- Anthropic tool-cache prefix semantics: https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-use-with-prompt-caching
- OpenAI prompt caching and differing cache-write prices: https://developers.openai.com/api/docs/guides/prompt-caching
- OpenClaw failover and sticky cache identity: https://docs.openclaw.ai/concepts/model-failover

## Focused verification scope

Tests include a real loopback TCP socket receiving a Chat Completions POST and dropping before any reply, asserting no duplicate generation; Anthropic loopback 400 unsupported-hint rejection followed by one uncached retry and durable provider capability memory; and two Anthropic request turns with identical static cache prefix and different task inputs, recording cache-read/write usage into the Rust job record.

These are deterministic protocol fixtures, not commercial model-account benchmarks or measured monetary savings. Source edits keep vendor functionality inside Dominion's Rust engine.
## Executed CI verification

Focused [GitHub Actions run 43693114690](https://github.com/hushmind3/dominion-rust/actions/runs/43693114690) succeeded on code commit `e9174a539f5fdcd83471c6cb7f38444b0eb9da2f`: 45 Rust SDK tests, 60 native Rust host/protocol tests (including Anthropic cache, provider incompatibility and disconnected POST), 15 interactive Codex WS tests, and 1 workflow recovery test: **121/121 Rust tests passed**. The production GUI passed TypeScript checking and Vite build; standalone TypeScript SDK compiled and passed **7/7 Node tests**. No paid provider account, released-product CI or monetary savings benchmark was exercised.
