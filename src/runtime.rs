//! First-party native execution SDK. No HTTP server or JS runtime is required.
//! Work is admitted by SQLite, executed by Rust and observed through durable
//! records. A separate HTTP adapter may expose these exact same operations.
use crate::{api, engine, store::Store, AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{env, error::Error, fmt, path::PathBuf, sync::Arc, time::{Duration, Instant}};
use tokio::sync::{broadcast, Semaphore};

#[derive(Debug, Clone)]
pub struct DominionConfig {
    pub workspace: PathBuf,
    pub database: PathBuf,
    pub model_url: String,
    pub model_name: String,
    pub api_key: Option<String>,
    /// Independently identified sessions may run in parallel.
    pub max_concurrent_tasks: usize,
}

impl DominionConfig {
    pub fn from_env() -> Result<Self, RuntimeError> {
        Ok(Self {
            workspace: PathBuf::from(env::var("DOMINION_WORKSPACE").unwrap_or_else(|_| ".".into())),
            database: env::var("DOMINION_DB").map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(".dominion/data.sqlite")),
            model_url: env::var("DOMINION_MODEL_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434/v1/chat/completions".into()),
            model_name: env::var("DOMINION_MODEL").unwrap_or_else(|_| "qwen3".into()),
            api_key: env::var("DOMINION_API_KEY").ok().filter(|key| !key.trim().is_empty()),
            max_concurrent_tasks: env::var("DOMINION_TASK_WORKERS").ok()
                .and_then(|value| value.parse::<usize>().ok()).unwrap_or(4).clamp(1, 32),
        })
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeError(pub String);

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl Error for RuntimeError {}
impl From<String> for RuntimeError {
    fn from(value: String) -> Self { Self(value) }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub prompt: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub allow_write: bool,
    #[serde(default)]
    pub allow_shell: bool,
    #[serde(default)]
    pub require_evidence: bool,
    #[serde(default)]
    pub max_steps: Option<u64>,
    #[serde(default)]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub planning_mode: Option<String>,
}

impl TaskInput {
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.prompt.trim().is_empty() || self.prompt.len() > 32_000 {
            return Err(RuntimeError("A non-empty prompt of at most 32,000 bytes is required".into()));
        }
        if self.session_id.as_deref().is_some_and(|id| id.is_empty() || id.len() > 128) {
            return Err(RuntimeError("Invalid native session identifier".into()));
        }
        if let Some(key) = &self.request_id {
            if !(8..=128).contains(&key.len()) || !key.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') {
                return Err(RuntimeError("Invalid idempotency key".into()));
            }
        }
        if !(1..=24).contains(&self.max_steps.unwrap_or(12)) {
            return Err(RuntimeError("maxSteps must be 1..=24".into()));
        }
        if !(256..=500_000).contains(&self.token_budget.unwrap_or(40_000)) {
            return Err(RuntimeError("tokenBudget must be 256..=500000".into()));
        }
        if self.planning_mode.as_deref().is_some_and(
            |value| !matches!(value,"auto"|"always"|"never")
        ) {
            return Err(RuntimeError("planningMode must be auto, always or never".into()));
        }
        Ok(())
    }

    /// Generates a caller-independent durable retry key when none was supplied.
    fn normalized(self) -> Result<(String, Value), RuntimeError> {
        self.validate()?;
        let key = self.request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let payload = json!({
            "prompt": self.prompt.trim(),
            "sessionId": self.session_id,
            "allowWrite": self.allow_write,
            "allowShell": self.allow_shell,
            "requireEvidence": self.require_evidence,
            "maxSteps": self.max_steps.unwrap_or(12),
            "tokenBudget": self.token_budget.unwrap_or(40_000),
            "planningMode": self.planning_mode.unwrap_or_else(||"auto".into()),
        });
        Ok((key, payload))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub request_id: String,
    pub input: Value,
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub cancel_requested: bool,
    pub steps: u64,
    pub tokens: u64,
    pub created_at: String,
}

impl TaskRecord {
    pub fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled" | "needs_review")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub seq: u64,
    pub task_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub detail: Value,
}

#[derive(Clone)]
pub struct DominionSdk {
    state: AppState,
}

impl DominionSdk {
    /// Bind an HTTP handler to the same native execution authority.
    pub(crate) fn from_state(state: AppState) -> Self { Self { state } }

    /// Construct a fully native runtime and its asynchronous durable worker.
    /// It executes work without an HTTP listener or a TypeScript process.
    pub async fn start(config: DominionConfig) -> Result<Self, RuntimeError> {
        if config.model_name.trim().is_empty() {
            return Err(RuntimeError("A configured model name is required".into()));
        }
        if !config.model_url.starts_with("http://")
            && !config.model_url.starts_with("https://") {
            return Err(RuntimeError("The model endpoint must use HTTP(S)".into()));
        }
        let workspace = config.workspace.canonicalize()
            .map_err(|error| RuntimeError(format!("Invalid workspace: {error}")))?;
        let store = Store::open(&config.database).map_err(RuntimeError)?;
        // Uncertain external tool effects are never replayed after a crash.
        store.recover().map_err(RuntimeError)?;
        let (bus, _) = broadcast::channel(2048);
        let state = AppState {
            store, bus,
            http: reqwest::Client::builder().timeout(Duration::from_secs(90))
                .build().map_err(|error| RuntimeError(error.to_string()))?,
            workspace,
            model_url: config.model_url,
            model_name: config.model_name,
            api_key: config.api_key,
        };
        let sdk = Self { state };
        sdk.start_dispatcher(config.max_concurrent_tasks.clamp(1, 32));
        Ok(sdk)
    }

    fn start_dispatcher(&self, max_concurrent: usize) {
        let state = self.state.clone();
        let semaphore = Arc::new(Semaphore::new(max_concurrent));
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(400));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                for id in state.store.runnable_ids(max_concurrent) {
                    // Native concurrency is bounded even when many clients
                    // submit work simultaneously. SQLite claim() prevents
                    // two jobs mutating the same session in parallel.
                    let Ok(permit) = semaphore.clone().try_acquire_owned() else { break; };
                    let worker = state.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        engine::run(worker, id).await;
                    });
                }
            }
        });
    }

    /// The exact Axum API used by all presentation and external clients.
    pub fn router(&self) -> axum::Router {
        api::router(self.state.clone())
    }

    pub fn health(&self) -> Value { self.state.diagnostic() }

    /// Queue a task. A repeated request key with the same payload returns the
    /// existing record, never a second execution or provider charge.
    pub async fn submit(&self, input: TaskInput) -> Result<TaskRecord, RuntimeError> {
        let (key, payload) = input.normalized()?;
        let (record, _) = self.state.store.submit(&key, &payload).map_err(RuntimeError)?;
        decode_record(record)
    }

    pub fn task(&self, id: &str) -> Result<TaskRecord, RuntimeError> {
        let value = self.state.store.get(id)
            .ok_or_else(|| RuntimeError("Task record not found".into()))?;
        decode_record(value)
    }

    pub fn list_tasks(&self) -> Result<Vec<TaskRecord>, RuntimeError> {
        self.state.store.list().into_iter().map(decode_record).collect()
    }

    /// Durable cancellation, distinct from dropping a client-side future.
    pub fn cancel(&self, id: &str) -> Result<TaskRecord, RuntimeError> {
        let value = self.state.store.cancel(id)
            .ok_or_else(|| RuntimeError("Task record not found".into()))?;
        self.state.emit(id, "cancel_requested", json!({}));
        decode_record(value)
    }

    /// Queue a user correction to be read before the next model iteration.
    pub fn steer(&self, id: &str, instruction: &str) -> Result<TaskRecord, RuntimeError> {
        let text = instruction.trim();
        if text.is_empty() || text.len() > 4096 {
            return Err(RuntimeError("Instruction must contain 1..=4096 bytes".into()));
        }
        let value = self.state.store.steer(id, text).map_err(RuntimeError)?;
        self.state.emit(id, "instruction_queued", json!({"text":text}));
        decode_record(value)
    }

    /// Replay persisted events, including after disconnect/restart.
    pub fn events_after(&self, id: &str, after: u64, limit: usize) -> Result<Vec<TaskEvent>, RuntimeError> {
        if self.state.store.get(id).is_none() {
            return Err(RuntimeError("Task record not found".into()));
        }
        self.state.store.events_after(id, after, limit.clamp(1, 500)).into_iter()
            .map(|value| serde_json::from_value(value)
                .map_err(|error| RuntimeError(error.to_string()))).collect()
    }

    /// Subscribe to transient notifications. Always backfill through events_after
    /// when continuity matters because broadcast receivers can lag.
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.state.bus.subscribe()
    }

    /// Convenience for headless Rust applications. Returns the persisted
    /// terminal record, not the model's unverified completion text.
    pub async fn wait(&self, id: &str, deadline: Duration) -> Result<TaskRecord, RuntimeError> {
        let start = Instant::now();
        loop {
            let record = self.task(id)?;
            if record.terminal() { return Ok(record); }
            if start.elapsed() >= deadline {
                return Err(RuntimeError("Task wait deadline exceeded; execution may continue".into()));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn execute(&self, input: TaskInput, deadline: Duration) -> Result<TaskRecord, RuntimeError> {
        let record = self.submit(input).await?;
        self.wait(&record.id, deadline).await
    }
}

fn decode_record(value: Value) -> Result<TaskRecord, RuntimeError> {
    serde_json::from_value(value).map_err(|error| RuntimeError(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_task_validation_precedes_any_model_or_database_calls() {
        assert!(TaskInput::default().validate().is_err());
        assert!(TaskInput {prompt:"ok".into(),max_steps:Some(25), ..Default::default()}
            .validate().is_err());
        assert!(TaskInput {prompt:"ok".into(),request_id:Some("bad!".into()),..Default::default()}
            .validate().is_err());
        assert!(TaskInput {prompt:"ok".into(),token_budget:Some(128),..Default::default()}
            .validate().is_err());
    }
    #[test]
    fn native_task_serialization_matches_the_public_rust_http_contract() {
        let (key, normalized) = TaskInput {
            prompt:"  read README.md  ".into(),
            request_id:Some("same-task-2026".into()),
            session_id:Some("workspace-session".into()),
            allow_write:false,
            ..Default::default()
        }.normalized().unwrap();
        assert_eq!(key, "same-task-2026");
        assert_eq!(normalized["prompt"], "read README.md");
        assert_eq!(normalized["sessionId"], "workspace-session");
        assert_eq!(normalized["allowWrite"], false);
        assert_eq!(normalized["maxSteps"], 12);
    }
}
