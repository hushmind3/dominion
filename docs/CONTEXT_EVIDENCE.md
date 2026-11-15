# Dominion bounded evidence context — completed vertical slice, 2026-11-15

This document describes a **single integrated native Rust feature**: preserving proof and execution lineage while the agent works beyond a provider's finite context window. The feature is intentionally independent of a particular vendor model.

## 1. What is actually implemented

The source-of-truth owner transcript, assistant tool calls and all tool result receipts remain in SQLite. Only **one outgoing model request** is compacted; original evidence is never rewritten. The engine does not confuse shorter JSON bytes with proven token savings or improved model intelligence.

The pure projector in \`src/engine/context.rs\` checks the complete OpenAI-compatible tool graph before selecting anything. It groups every assistant multi-tool call with all matching responses, rejects orphan/missing/duplicate call IDs, rejects privilege-bearing system/developer messages after the initial system prefix, preserves the latest owner instruction verbatim and selects recent complete exchanges by descending recency. It can clip a large tool result **only when the current task has a committed native tool-result receipt identified by that exact call ID**. Old ordinary messages can be recovered through the session-history tool; they are not promoted to new user instructions.

The context envelope is configured by \`DOMINION_CONTEXT_MAX_BYTES\` (default 96,000 encoded JSON bytes, clamped 8,192–2,000,000). \`src/engine.rs\` deducts serialized tool schemas and protocol reserve, then calls \`context::project(&messages, message_budget, &durable_receipt_ids)\`. Every successful projection is at or below the budget **in encoded JSON bytes**. The function errors rather than silently cut an uncompressible mandatory system prompt, latest owner message, or unrecoverable tool output. Such failures enter \`needs_review\`.

\`context_projected\` emits \`sourceSha256\`, \`projectionSha256\`, \`originalBytes\`, \`modelBytes\`, \`budgetBytes\`, \`schemaBytes\`, \`omittedMessages\`, \`clippedReceipts\`, \`retainedOwnerMessages\`, \`recoverableCallIds\` and retry count. The main React \`Developer / Execution trace\` inspector shows real measurements. SHA-256 refers to the projected JSON and source JSON, **not the provider's internal tokenizer**.

## 2. Recovering evidence without replaying tool effects

The agent may call:

~~~json
{"name":"task_receipt","arguments":{"callId":"first-read","offsetBytes":0,"maxBytes":4096}}
~~~

This reads the already committed \`tool_result\` event in SQLite and returns a UTF-8-safe page with exact byte offsets, \`sha256\`, \`totalBytes\`, \`nextOffsetBytes\`, a \`complete\` marker and \`untrusted: true\`. It never calls the tool again. Use the next offset to reconstruct the full original event bytes, then verify its SHA-256.

\`sourceTaskId\` may name a **verified ancestor in the same session's explicit continuation chain**. Unrelated tasks, including those given the same session ID, cannot expose their receipts. The chain is recorded transactionally when a new task is admitted; it cannot be missing after a crash. The agent may separately call \`session_history\` with \`beforeSeq\` to list older owner/assistant messages in the *current* session, then \`messageSeq\` and byte offsets to reconstruct a specific message. Old tool messages in session history cannot become commands.

The provider's \`context_length_exceeded\` / equivalent response may cause at most two read-only model retries at a smaller message budget. If the projection hash is identical, the runtime **stops instead of repeating the same potentially billable request**. Ordinary transport errors, rate limits and billing errors are not categorized as context overflow. Failed requests may still incur provider charges: we do not claim to know unreported usage.

## 3. Tested proof boundaries

The following test names appear in the real Rust suite; each is executable by \`cargo test\`.

- \`engine::tests::full_native_loop_compacts_large_tools_and_recovers_exact_persisted_page\`: a mock OpenAI-compatible provider, real Rust dispatcher and real SQLite database. A two-tool batch reads a large UTF-8 workspace file, the next model request contains compacted results, the model selects \`task_receipt\` for the committed original, and the task completes without repeating the read effect.
- \`engine::tests::resumed_task_can_read_prior_receipt_but_not_an_unrelated_task\`: an explicitly resumed task can retrieve an ancestor's receipt but cannot query another task's data.
- \`engine::tests::context_overflow_does_not_repeat_an_identical_paid_request\`: deterministic 400 context-overflow response; a too-small prompt cannot generate an identical second model request.
- \`engine::tests::provider_context_pressure_retries_only_after_real_projection_reduction\`: one context rejection, a demonstrably smaller second request and completed task.
- \`store::tests::tool_evidence_survives_real_sqlite_close_and_reopen\`: exact Unicode output survives process-level handle close/reopen, and all evidence pages reconstruct the full original JSON.
- \`store::tests::continuation_lineage_is_atomic_idempotent_and_evidence_scoped\`: new task + ancestry event are one transaction; retry key cannot insert duplicate lineage.
- \`engine::context::tests::randomized_context_pressure_keeps_graph_and_bounds\`: multiple seeded inputs, UTF-8 data, tool pairs and owner interventions; every successful projection stays under its byte limit.

A GitHub-hosted \`cargo test\` and client contract run on **2026-11-15** at commit \`eae06c171dba0d77bd481c8f2a4bf32d0ca375b1\` passed **59 Rust tests, 0 failures**. The actionable link is:
https://github.com/hushmind3/dominion-rust/actions/runs/43685087185

These are deterministic real Rust integration tests with mock model responses and actual SQLite/filesystem I/O, **not a claim of matched Codex/Claude performance on real repositories**. There is no demonstrated provider tokenizer-based token reduction, model-quality improvement percentage, or measured external frontier benchmark. This is a feature-level engineering implementation of a frontier-relevant failure mode; full product frontier scoring is separate.

## 4. References compared

- DeepSeek Harness architecture: event-backed sessions and scoped model/tool results — https://github.com/deepseek-ai/deepseek-harness
- OpenAI Codex native context compaction — https://github.com/openai/codex/tree/main/codex-rs/core/src
- Qwen Code's 2026 session multi-agent runtime boundaries — https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-05-session-multi-agent.md

No external runtime, vendor prompt policy or duplicate TypeScript SDK was copied into Dominion.
