mod api;
mod engine;
mod store;

use axum::Router;
use serde_json::{json, Value};
use std::{env, path::PathBuf};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub store: store::Store,
    pub bus: broadcast::Sender<Value>,
    pub http: reqwest::Client,
    pub workspace: PathBuf,
    pub model_url: String,
    pub model_name: String,
    pub api_key: Option<String>,
}

impl AppState {
    pub fn emit(&self, task_id: &str, kind: &str, detail: Value) {
        match self.store.append_event(task_id, kind, detail) {
            Ok(record) => { let _ = self.bus.send(record); }
            Err(error) => eprintln!("event persistence failure: {error}"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = env::var("DOMINION_WORKSPACE").unwrap_or_else(|_| ".".into());
    let workspace = PathBuf::from(workspace).canonicalize()?;
    let db_path = env::var("DOMINION_DB")
        .unwrap_or_else(|_| ".dominion/data.sqlite".into());
    let store = store::Store::open(std::path::Path::new(&db_path))
        .map_err(std::io::Error::other)?;
    // Mark uncertain in-flight work for review; the scheduler will pick up
    // queued tasks without replaying a potentially completed external effect.
    store.recover()?;
    let (bus, _) = broadcast::channel(2048);
    let state = AppState {
        store,
        bus,
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90)).build()?,
        workspace,
        model_url: env::var("DOMINION_MODEL_URL").unwrap_or_else(|_|
            "http://127.0.0.1:11434/v1/chat/completions".into()),
        model_name: env::var("DOMINION_MODEL").unwrap_or_else(|_| "qwen3".into()),
        api_key: env::var("DOMINION_API_KEY").ok().filter(|s| !s.is_empty()),
    };
    // A task queued behind another turn in the same session must start after
    // the earlier turn finishes. A durable queue scan also resumes safe queued
    // work after restart, without requiring an open browser or SDK connection.
    let queue_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(400));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            for id in queue_state.store.runnable_ids(32) {
                let next = queue_state.clone();
                tokio::spawn(async move { engine::run(next, id).await; });
            }
        }
    });
    let port = env::var("DOMINION_PORT").ok()
        .and_then(|s| s.parse::<u16>().ok()).unwrap_or(4317);
    let app: Router = api::router(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("{}", json!({"service":"dominion","port":port,"runtime":"rust"}));
    axum::serve(listener, app).await?;
    Ok(())
}
