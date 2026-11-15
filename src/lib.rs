//! DOMINION native SDK and autonomous execution engine.
//! HTTP, desktop frontends and language bindings share this same Rust runtime.
pub mod api;
pub mod engine;
pub mod runtime;
pub mod store;
pub mod mcp;
pub mod provider;
pub mod group;
pub mod video;
pub mod veo;
pub mod improvement;
pub mod knowledge;

pub use runtime::{DominionConfig, DominionSdk, RuntimeError, TaskInput, TaskRecord, TaskEvent};

use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::broadcast;

/// Native shared process state. Use [DominionSdk] to construct and operate it.
#[derive(Clone)]
pub struct AppState {
    pub(crate) store: store::Store,
    pub(crate) mcp: Arc<mcp::Mcp>,
    pub(crate) provider: provider::ModelClient,
    pub(crate) groups: Arc<group::Groups>,
    pub(crate) videos: Arc<video::Videos>,
    pub(crate) improvements: Arc<improvement::Improvements>,
    pub(crate) bus: broadcast::Sender<Value>,
    pub(crate) workspace: PathBuf,
    pub(crate) model_name: String,
}

impl AppState {
    /// Persist before publishing. Call this checked variant before external
    /// tool side effects: a lost event cannot authorize untracked work.
    pub(crate) fn record_event(&self, task_id: &str, kind: &str, detail: Value)
        -> Result<Value, String> {
        let record = self.store.append_event(task_id, kind, detail)?;
        let _ = self.bus.send(record.clone());
        Ok(record)
    }

    pub(crate) fn emit(&self, task_id: &str, kind: &str, detail: Value) {
        if let Err(error) = self.record_event(task_id, kind, detail) {
            eprintln!("event persistence failure: {error}");
        }
    }

    pub(crate) fn diagnostic(&self) -> Value {
        json!({"engine":"rust","model":self.model_name,
            "workspace":self.workspace.to_string_lossy(),"localOnly":true})
    }
}
