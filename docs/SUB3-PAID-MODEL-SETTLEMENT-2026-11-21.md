# Sub3 — durable model-turn settlement and no replay after ambiguity

Date: 2026-11-21. Branch: `dominion-sub3`.
Scope: native Rust AgentLoop; no UI, pricing, Cloudflare or other branch changes.

## Why this matters

The shared Rust runtime's `dispatch_model_turn` writes
`model_in_flight=true` **before** asking the provider for a paid generation.
This protects an interrupted provider request from automatic paid replay.
Previously, the native AgentLoop cleared the marker immediately upon return
and saved usage **before** decoding and journaling the model's decision.
A process death in that gap left a running checkpoint with charged usage,
no in-flight fence and no durable decision. Resume could start another model
generation rather than reviewing the missing first observation.

## Changed native settlement protocol

1. Before provider dispatch, persist `model_in_flight=true` and fence
   owner cancellation/steering as before.
2. When the provider returns, charge the actual reported or estimated tokens
   and persist them **while the in-flight marker remains set**.
3. Decode and validate a model decision. Append it to `decision_tape` and
   clear `model_in_flight` in the **same Journal save**.
4. If the response cannot be decoded, persist the concrete parse-failure
   checkpoint and clear the marker in that **same save**.
5. If token charging exceeds the owner budget, stop with `NeedsReview`
   and a saved terminal checkpoint, without accepting the decision or
   dispatching its tools.
6. A failed Journal save never authorizes a model-proposed external action.
   A prior checkpoint with `model_in_flight=true` cannot be resumed as
   a clean new model call; an owner must review it.

The product's `Journal` implementation still supplies atomic SQLite
persistence. The SDK `Journal` trait remains stable. This patch changes
the ordering of durable observations, not the model/provider protocol.

## Crash-injection regression

`crates/dominion-agent-sdk/tests/kernel.rs` includes:
- `paid_model_settlement_never_releases_fence_before_decision_receipt`:
  all successful running checkpoints that release a paid-generation fence
  must already contain a model decision or explicit parse error.
- `interrupted_model_decision_commit_fails_closed_without_paid_replay`:
  fail precisely the model-decision Journal write, then prove the previous
  charged snapshot retains `model_in_flight`, no tool executes and
  `Kernel::resume` refuses to repeat the paid model call.

CI entrypoint: `cargo test -p dominion-agent-sdk --test kernel`,
now required by `sub3-rust-sdk-check.yml`.

## What this does not claim

This is a **model-generation fence**, not general exactly-once
network billing. A remote provider may charge a request even when the
client receives no reply; an ambiguous call remains reviewable rather
than silently retried. Automatic mid-turn recovery of an already committed
but not yet applied decision needs a separate replay cursor and is not
claimed here.

## 2026 architecture references

- OpenAI Codex app-server: thread / turn / item lifecycle and persisted
  execution observations:
  https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md
- ByteDance DeerFlow 2.x: gateway RunManager and scoped runtime
  execution/context boundary:
  https://github.com/bytedance/deer-flow
- Qwen Code: fork/subagent isolation and explicit tool-scoped delegation:
  https://github.com/QwenLM/qwen-code/blob/main/docs/developers/tools/task.md

The patch is Dominion-specific: it keeps the native Rust kernel's
existing journal and no-replay authority and does not copy code.
