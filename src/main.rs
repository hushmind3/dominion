//! Optional HTTP/frontend host for the first-party Rust SDK.
//! The same runtime also works in-process with no web server.
use dominion::{DominionConfig, DominionSdk};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = DominionSdk::start(DominionConfig::from_env()?).await?;
    let port = env::var("DOMINION_PORT").ok()
        .and_then(|value| value.parse::<u16>().ok()).unwrap_or(4317);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("{}", serde_json::json!({
        "service":"dominion",
        "port":port,
        "runtime":"rust",
        "sdk":"native"
    }));
    axum::serve(listener, sdk.router()).await?;
    Ok(())
}
