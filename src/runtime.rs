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
    /// Explicit run-level consent to use configured external MCP tools.
    #[serde(default)]
    pub allow_mcp: bool,
    /// Explicitly permit bounded, read-only child tasks with lineage.
    #[serde(default)]
    pub allow_subagents: bool,
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
            "allowMcp": self.allow_mcp,
            "allowSubagents": self.allow_subagents,
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
        let mcp = std::sync::Arc::new(crate::mcp::Mcp::from_env().map_err(RuntimeError)?);
        let rate = |key: &str| env::var(key).ok().and_then(|text| text.parse::<f64>().ok())
            .unwrap_or(0.0);
        let mut main = crate::provider::ModelConfig::basic(
            config.model_url.clone(),config.model_name.clone(),config.api_key.clone()
        );
        main.input_krw_per_million=rate("DOMINION_INPUT_KRW_PER_MILLION");
        main.output_krw_per_million=rate("DOMINION_OUTPUT_KRW_PER_MILLION");
        let economy = env::var("DOMINION_ECONOMY_MODEL_URL").ok().map(|endpoint| {
            let mut cheap = crate::provider::ModelConfig::basic(
                endpoint,env::var("DOMINION_ECONOMY_MODEL")
                    .unwrap_or_else(|_|config.model_name.clone()),
                env::var("DOMINION_ECONOMY_API_KEY").ok()
            );
            cheap.input_krw_per_million=rate("DOMINION_ECONOMY_INPUT_KRW_PER_MILLION");
            cheap.output_krw_per_million=rate("DOMINION_ECONOMY_OUTPUT_KRW_PER_MILLION");
            cheap
        });
        let provider = crate::provider::ModelClient::new(main,economy).map_err(RuntimeError)?;
        let group_dir = config.database.parent().unwrap_or(std::path::Path::new("."))
            .join("groups");
        let groups = std::sync::Arc::new(
            crate::group::Groups::open(group_dir).await.map_err(RuntimeError)?
        );
        let video_dir = config.database.parent().unwrap_or(std::path::Path::new("."))
            .join("videos");
        let videos = std::sync::Arc::new(
            crate::video::Videos::open(video_dir).await.map_err(RuntimeError)?
        );
        let improvement_dir = config.database.parent().unwrap_or(std::path::Path::new("."))
            .join("improvements");
        let improvements = std::sync::Arc::new(
            crate::improvement::Improvements::open(improvement_dir,workspace.clone())
                .await.map_err(RuntimeError)?
        );
        let state = AppState {
            store, bus, mcp, provider, groups, videos, improvements,
            workspace,
            model_name: config.model_name,
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

impl DominionSdk {
    /// Actual persisted multi-participant rooms, sharing the same native
    /// model router and configured economy/strong models as agent runs.
    pub async fn group_create(&self, input: crate::group::NewRoom)
        -> Result<crate::group::Room,RuntimeError> {
        self.state.groups.create(input).await.map_err(RuntimeError)
    }
    pub async fn group_list(&self) -> Vec<crate::group::Room> {
        self.state.groups.list().await
    }
    pub async fn group_get(&self,id:uuid::Uuid)->Result<crate::group::Room,RuntimeError>{
        self.state.groups.get(id).await
            .ok_or_else(||RuntimeError("Room not found".into()))
    }
    pub async fn group_say(&self,id:uuid::Uuid,text:String)
        ->Result<crate::group::Room,RuntimeError>{
        self.state.groups.say(id,text).await.map_err(RuntimeError)
    }
    pub async fn group_next(&self,id:uuid::Uuid)
        ->Result<crate::group::Room,RuntimeError>{
        self.state.groups.next(&self.state.provider,id).await.map_err(RuntimeError)
    }
}

impl DominionSdk {
    pub async fn video_create(&self,input:crate::video::NewVideo)
        -> Result<crate::video::Video,RuntimeError>{
        self.state.videos.create(&self.state.provider,input).await.map_err(RuntimeError)
    }
    pub async fn video_list(&self)->Vec<crate::video::Video>{
        self.state.videos.list().await
    }
    pub async fn video_get(&self,id:uuid::Uuid)
        ->Result<crate::video::Video,RuntimeError>{
        self.state.videos.get(id).await
            .ok_or_else(||RuntimeError("Video not found".into()))
    }
    pub async fn video_scene(&self,id:uuid::Uuid,index:usize,change:crate::video::ChangeScene)
        ->Result<crate::video::Video,RuntimeError>{
        self.state.videos.change_scene(id,index,change).await.map_err(RuntimeError)
    }
    pub async fn video_render(&self,id:uuid::Uuid)
        ->Result<crate::video::Video,RuntimeError>{
        self.state.videos.start(id).await.map_err(RuntimeError)
    }
    pub async fn video_output(&self,id:uuid::Uuid)->Result<std::path::PathBuf,RuntimeError>{
        self.state.videos.output_path(id).await.map_err(RuntimeError)
    }
}

impl DominionSdk {
    /// An isolated candidate never changes the checkout until explicitly
    /// adopted and independently scored against the frozen reference.
    pub async fn improvement_create(&self,input:crate::improvement::NewImprovement)
        ->Result<crate::improvement::Improvement,RuntimeError>{
        self.state.improvements.create(input).await.map_err(RuntimeError)
    }
    pub async fn improvement_list(&self)->Vec<crate::improvement::Improvement>{
        self.state.improvements.list().await
    }
    pub async fn improvement_get(&self,id:uuid::Uuid)
        ->Result<crate::improvement::Improvement,RuntimeError>{
        self.state.improvements.get(id).await
            .ok_or_else(||RuntimeError("Improvement trial not found".into()))
    }
    pub async fn improvement_evaluate(&self,id:uuid::Uuid)
        ->Result<crate::improvement::Improvement,RuntimeError>{
        let enabled=std::env::var("DOMINION_ENABLE_SHELL")
            .as_deref()==Ok("true");
        self.state.improvements.start(self.state.provider.clone(),id,enabled)
            .await.map_err(RuntimeError)
    }
    pub async fn improvement_adopt(&self,id:uuid::Uuid)
        ->Result<crate::improvement::Improvement,RuntimeError>{
        self.state.improvements.adopt(id).await.map_err(RuntimeError)
    }
    pub async fn improvement_descendant(&self,id:uuid::Uuid)
        ->Result<crate::improvement::Improvement,RuntimeError>{
        self.state.improvements.descendant(id).await.map_err(RuntimeError)
    }
}

impl DominionSdk {
    pub fn remember(&self, session:Option<&str>,content:&str,kind:&str)
        ->Result<Value,RuntimeError>{
        self.state.store.remember(session,content,kind).map_err(RuntimeError)
    }
    pub fn memory_list(&self,session:Option<&str>)->Vec<Value>{
        self.state.store.memory_list(session)
    }
    pub fn memory_search(&self,session:Option<&str>,query:&str,limit:usize)->Vec<Value>{
        crate::knowledge::recall(&self.state.store.memory_list(session),
            query,session,limit)
    }
    pub fn memory_disable(&self,id:&str)->Result<bool,RuntimeError>{
        self.state.store.memory_disable(id).map_err(RuntimeError)
    }
    pub fn skill_create(&self,name:&str,trigger:&str,instructions:&str,enabled:bool)
        ->Result<Value,RuntimeError>{
        self.state.store.skill_save(name,trigger,instructions,enabled)
            .map_err(RuntimeError)
    }
    pub fn skill_list(&self)->Vec<Value>{
        self.state.store.skill_list()
    }
    pub fn skill_enable(&self,id:&str,enabled:bool)->Result<bool,RuntimeError>{
        self.state.store.skill_enable(id,enabled).map_err(RuntimeError)
    }
}

impl DominionSdk {
    /// Delegate bounded research to a separate native worker. Child tasks
    /// cannot gain write/shell/network tools and cannot recursively delegate.
    /// Model-initiated descendants are capped to three per parent.
    pub async fn delegate(&self,parent_id:&str,mut input:TaskInput)
        ->Result<TaskRecord,RuntimeError>{
        let parent=self.task(parent_id)?;
        if parent.status!="running" || parent.cancel_requested {
            return Err(RuntimeError("Parent execution must still be running".into()));
        }
        if parent.input["allowSubagents"]!=true {
            return Err(RuntimeError("Parent task has no subagent authorization".into()));
        }
        if input.allow_write || input.allow_shell || input.allow_mcp || input.allow_subagents {
            return Err(RuntimeError("Child research cannot elevate tool permissions".into()));
        }
        let children=self.state.store.child_tasks(parent_id);
        if children.len()>=3 {
            return Err(RuntimeError("At most 3 independent child tasks per parent".into()));
        }
        let allowed=parent.input["tokenBudget"].as_u64().unwrap_or(40000);
        // Reserve each child's full budget when admitting it, not merely its
        // currently spent tokens. Parallel delegates cannot overbook the cap.
        let reserved=children.iter().map(|child|
            child["input"]["tokenBudget"].as_u64().unwrap_or(2000)
        ).sum::<u64>();
        let remaining=allowed.saturating_sub(parent.tokens).saturating_sub(reserved);
        if remaining<256 {
            return Err(RuntimeError("Parent family token budget exhausted".into()));
        }
        let requested=input.token_budget.unwrap_or(remaining.min(2000));
        if requested>remaining {
            return Err(RuntimeError("Child token budget exceeds parent allowance".into()));
        }
        input.token_budget=Some(requested);
        input.max_steps=Some(input.max_steps.unwrap_or(6).min(8));
        // Derive a stable transcript from the idempotency key: repeating a
        // previously acknowledged tool call cannot open a second child session.
        let key=input.request_id.clone()
            .unwrap_or_else(||uuid::Uuid::new_v4().to_string());
        let seed=format!("{parent_id}/{key}");
        use sha2::Digest;
        let digest=sha2::Sha256::digest(seed.as_bytes());
        input.session_id=Some(format!("child-{digest:x}"));
        input.request_id=Some(key);
        let (key,mut payload)=input.normalized()?;
        payload["parentId"]=json!(parent_id);
        let (record,created)=self.state.store.submit(&key,&payload).map_err(RuntimeError)?;
        if created{
            self.state.emit(parent_id,"subagent_started",json!({
                "taskId":record["id"],"sessionId":payload["sessionId"],
                "tokenBudget":requested
            }));
        }
        decode_record(record)
    }

    pub fn children(&self,parent_id:&str)->Result<Vec<TaskRecord>,RuntimeError>{
        self.task(parent_id)?;
        self.state.store.child_tasks(parent_id)
            .into_iter().map(decode_record).collect()
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
    #[tokio::test]
    async fn rust_sdk_executes_verified_work_without_http_frontend() {
        use axum::{routing::post, Json, Router};
        async fn provider(Json(request): Json<Value>) -> Json<Value> {
            let has_receipt = request["messages"].as_array().is_some_and(|history|
                history.iter().any(|message| message["role"] == "tool"));
            let response = if has_receipt {
                json!({"role":"assistant","content":"File written and readback verified."})
            } else {
                json!({"role":"assistant","content":null,"tool_calls":[{
                    "id":"write-test","type":"function","function":{
                        "name":"workspace_write",
                        "arguments":json!({"path":"outcome.txt","content":"native-sdk-proof"}).to_string()
                    }
                }]})
            };
            Json(json!({"choices":[{"message":response}],"usage":{"total_tokens":30}}))
        }
        let listener = tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("http://{}/chat/completions",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(provider)))
                .await.unwrap()
        });
        let root=std::env::temp_dir().join(
            format!("dominion-rust-sdk-{}",uuid::Uuid::new_v4())
        );
        std::fs::create_dir_all(&root).unwrap();
        let sdk=DominionSdk::start(DominionConfig {
            workspace:root.clone(),
            database:root.join("native.sqlite"),
            model_url:url,
            model_name:"mock-provider".into(),
            api_key:None,
            max_concurrent_tasks:2,
        }).await.unwrap();
        let result=sdk.execute(TaskInput {
            prompt:"Create outcome.txt".into(),
            request_id:Some("sdk-native-2026".into()),
            allow_write:true,
            require_evidence:true,
            max_steps:Some(4),
            ..Default::default()
        },Duration::from_secs(5)).await.unwrap();
        assert_eq!(result.status,"completed");
        assert_eq!(std::fs::read_to_string(root.join("outcome.txt")).unwrap(),
            "native-sdk-proof");
        let events=sdk.events_after(&result.id,0,50).unwrap();
        assert!(events.iter().any(|event|event.kind=="tool_result"));
        assert!(events.iter().any(|event|event.kind=="finished"));
        let duplicate=sdk.submit(TaskInput {
            prompt:"Create outcome.txt".into(),
            request_id:Some("sdk-native-2026".into()),
            allow_write:true,
            require_evidence:true,
            max_steps:Some(4),
            ..Default::default()
        }).await.unwrap();
        assert_eq!(duplicate.id,result.id);
        server.abort();
        let _=std::fs::remove_dir_all(root);
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
