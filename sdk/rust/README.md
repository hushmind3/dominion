# DOMINION Rust SDK (active `dominion-sub3` contract)

[Product README](../../README.md) · [한국어](../../README.ko.md) · [Native execution architecture](../../docs/HARNESS.md) · [Kernel source](../../crates/dominion-agent-sdk/src/lib.rs)

The `dominion-sdk` workspace crate has **two roles**:

1. `dominion_sdk::harness` re-exports the model-independent **`dominion-agent-sdk` Rust kernel**. It runs the bounded plan → tool → evidence/verification cycle with durable checkpoints. The embedding host supplies model access, tools, authorization, and the journal.
2. `dominion_sdk::Dominion` is an optional **HTTP/SSE client** for a running DOMINION server (default `http://127.0.0.1:4317`). It does not execute an agent loop.

The retired `dominion-harness` crate, standalone RSI generations, built-in video/audio generation API, and their earlier clients are **not part of the active branch**. Older documentation claiming that they still execute is historical and must not be used as an installation guide.

## Embed the Rust kernel

Use the workspace dependency:

```toml
dominion-sdk = { path = "sdk/rust" }
```

The public harness contract is re-exported from `crates/dominion-agent-sdk`:

```rust
use dominion_sdk::harness::{
    Kernel, RunRequest, Model, Tools, Evidence, Journal,
    Limits, Criterion, Strategy, Snapshot,
};
```

Implement `Model` (model planning and optional native tool calls), `Tools` (definitions and actual authorized operations), `Evidence` (independent postconditions), and `Journal` (durable write-ahead storage). Construct `Kernel` with those host adapters, then call `kernel.run(request).await`; for a previously persisted safe checkpoint, use `kernel.resume(request, snapshot).await`.

**Owner approval belongs in the host tool boundary**, and `Journal::save` must persist the intended effect before execution. Unknown in-flight model/tool outcomes are for manual review, not automatic replay. Independent host-confirmed reads can be scheduled concurrently; writes are serialized by the Rust scheduler.

`RunRequest` contains an ID, instruction, owner instructions, strategy, tool allowlist, limits and acceptance checks. The native kernel validates sizes, permission names and evidence criteria before model/tool calls. Evidence receipts remain in durable `Snapshot` records; `dominion_receipt_search` and `dominion_receipt_read` provide bounded lookup of previous operations.

## Use the already-running product

```rust
use dominion_sdk::Dominion;

async fn inspect_server() -> Result<(), Box<dyn std::error::Error>> {
let client = Dominion::local();
let status: serde_json::Value = client.health().await?;
let job = client.submit_prompt(
    "Inspect this workspace and report failing tests",
    None,
).await?;
let saved = client.get_job(&job.id).await?;
println!("engine={status:?}, job={}", saved.status);
    Ok(())
}
```

The real Rust client supports jobs, wait/cancel/steering, approvals, rooms, sessions, health, SSE chat, stored events, and `native_run_events(id, before, limit)` / `native_run_receipts(id, before, limit)` for read-only, owner-scoped paging (1–40 entries per request). Earlier event rows may fall outside the bounded in-job window; the response reports `eventsDropped`, while kernel tool receipts are independently persisted and omit tool arguments and raw outputs. See [`src/lib.rs`](src/lib.rs). Some calls need a configured model or permission. `with_admin_token` supports server installations that require an admin token.

## Integrated product execution

```text
React work panel / sdk/typescript / optional Rust HTTP client
       -> POST /api/jobs or POST /api/sdk/runs
       -> src/engine/jobs.rs (SQLite queue)
       -> src/engine/native_agent.rs (provider/tools/journal)
       -> dominion_sdk::harness::Kernel
```

New `POST /api/harness/runs` compatibility requests also use the native job kernel under a restricted workspace/shell tool set. Historical `sdk_run` records are visible as **read-only archives**, not re-executable checkpoints. Interactive `/api/chat` has a separate streaming turn lifecycle; it is **not** yet the same top-level Rust SDK loop.

## Targeted development checks

```sh
cargo test -p dominion-agent-sdk --lib
cargo test -p dominion-agent-sdk --test kernel_replanning
cargo test -p dominion-agent-sdk --test progressive_tool_discovery
cargo test --bin dominion-rust engine::native_agent::tests
cargo test --bin dominion-rust api::sdk::tests
```

Run relevant **development-profile** Rust tests during regular work; release builds, Windows/macOS packaging, frontend builds and paid provider comparisons are separate release/evaluation steps. An earlier focused GitHub run passing on its original commit does **not** establish success on the latest `dominion-sub3` HEAD. This document update itself has not been compiled or validated with a real model.

See [source audit](../../docs/SUB3-SOURCE-AUDIT-2026-11-18.md) and [SDK architecture notes](../../docs/HARNESS.md) for current boundaries.
