# DOMINION Rust SDK

Public Rust SDK for the existing independent DOMINION engine. The SDK is a standalone reusable crate, not an external agent runner. Planning, tools, memory, approval decisions, RSI and job recovery remain Rust engine responsibilities. Both the desktop UI and the SDK use the same server API and persisted state.

Add as a path dependency:

    dominion-sdk = { path = "sdk/rust" }

Example:

    use dominion_sdk::Dominion;
    use std::time::Duration;

    #[tokio::main]
    async fn main() -> Result<(), Box<dyn std::error::Error>> {
        let sdk = Dominion::local();
        let job = sdk.submit_prompt("Inspect and improve my repository", None).await?;
        let final_job = sdk.wait_job(&job.id, Duration::from_secs(120), Duration::from_secs(1)).await?;
        println!("{}: {:?}", final_job.status, final_job.result);
        Ok(())
    }

The actual backend routes are defined in src/api/mod.rs. Host/port default: 127.0.0.1:4317. Optional admin authentication uses the X-Dominion-Token header. There is no /api/v1 rewrite and no separate harness state.

- Prompt and workflow jobs, polling and cancellation
- Live chat SSE with terminal frame detection
- Durable /api/events replay with sequence deduplication
- Session, provider, agent, room, approval, execution, video and RSI entry points
- Structured HTTP error type; URL-safe identifiers
- Minimal dependencies; no model vendor SDK

Validation: cargo test --manifest-path sdk/rust/Cargo.toml (environment with Rust toolchain). Unit tests cover URL encoding, SSE framing and job terminal states. Full integration requires the running main Rust server plus configured model.
