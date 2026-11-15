//! DOMINION native SDK and autonomous execution engine.
//! HTTP, desktop frontends and language bindings share this same Rust runtime.
pub mod api;
pub mod engine;
pub mod runtime;
pub mod store;

pub use runtime::{DominionConfig, DominionSdk, RuntimeError, TaskInput, TaskRecord, TaskEvent};

use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::sync::broadcast;

/// Native shared process state. Use [DominionSdk] to construct and operate it.
#[derive(Clone)]
pub struct AppState {
    pub(crate) store: store::Store,
    pub(crate) bus: broadcast::Sender<Value>,
    pub(crate) http: reqwest::Client,
    pub(crate) workspace: PathBuf,
    pub(crate) model_url: String,
    pub(crate) model_name: String,
    pub(crate) api_key: Option<String>,
}

impl AppState {
    pub(crate) fn emit(&self, task_id: &str, kind: &str, detail: Value) {
        match self.store.append_event(task_id, kind, detail) {
            Ok(record) => { let _ = self.bus.send(record); }
            Err(error) => eprintln!("event persistence failure: {error}"),
        }
    }

    pub(crate) fn diagnostic(&self) -> Value {
        json!({"engine":"rust","model":self.model_name,
            "workspace":self.workspace.to_string_lossy(),"localOnly":true})
    }
}
