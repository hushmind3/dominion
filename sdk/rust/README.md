# Dominion Native Rust SDK

**The Rust SDK is the primary agent harness.** The canonical `dominion_sdk::harness` exposes the actual native model-independent kernel, with bounded model/tool loops, parallel read-only actions, owner steering, evidence verification and durable pre-effect journals. The HTTP client in lib.rs is optional.

Use it from another Rust application with this path dependency:

    dominion-sdk = { path = "sdk/rust" }

Then implement the provider-neutral dominion_sdk::harness::Driver trait using that application's model, tool, storage, verification and cancellation services. Call dominion_sdk::harness::run with the owner's task and policy. The host must persist each checkpoint before external tool effects.

The DOMINION production adapter, src/engine/native_agent.rs, is an actual Driver implementation: model planning through the native router, execution through the native Rust agent tool/approval/runtime, deterministic acceptance through the native verifier, and checkpoints through the SQLite-backed job queue. GUI tasks and TypeScript SDK both target that path via /api/jobs. The separately isolated RSI generation/independent acceptance path remains in place.

The client methods in sdk/rust/src/lib.rs optionally connect to an existing Rust engine at 127.0.0.1:4317 for jobs, sessions, approvals, rooms, video, RSI, chat SSE and replayable events. They are secondary to the native harness.

No vendor SDK owns the loop. No goals.rs and no /api/goals API. Tests for the core cover verified repair, pre-effect checkpoint barriers and cancellation. Run cargo test --manifest-path sdk/rust/Cargo.toml and cargo test --manifest-path Cargo.toml in a Rust development environment. Live model and server tests remain outstanding. See docs/SDK-ARCHITECTURE-2026-11-15.md.

Native entrypoint: `use dominion_sdk::harness::{Kernel, RunRequest, Model, Tools, Evidence, Journal};`. This is a real execution kernel rather than a wrapper around an agent provider. `Dominion` HTTP methods are secondary, for remote/local product integration.
