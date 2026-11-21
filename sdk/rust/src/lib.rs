//! DOMINION SDK. All decisions, tools, memory, execution and checkpoints live in
//! the DOMINION Rust engine; this crate exposes that engine to other Rust apps.
//! The SDK does not spawn an external agent harness or depend on a vendor SDK.
/// DOMINION's canonical, model-independent native orchestration kernel.
pub mod harness {
    pub use dominion_agent_sdk::*;
}

use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use reqwest::{Client, Method, Response, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{fmt, pin::Pin, time::{Duration, Instant}};

#[derive(Debug)]
pub enum Error {
    Url(String), Http(reqwest::Error), Api { status: u16, message: String },
    Json(serde_json::Error), Stream(String), Timeout,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(s) | Self::Stream(s) => write!(f, "{s}"),
            Self::Http(e) => write!(f, "{e}"),
            Self::Api { status, message } => write!(f, "HTTP {status}: {message}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::Timeout => write!(f, "Dominion job did not finish before the deadline"),
        }
    }
}
impl std::error::Error for Error {}
impl From<reqwest::Error> for Error { fn from(e: reqwest::Error) -> Self { Self::Http(e) } }
impl From<serde_json::Error> for Error { fn from(e: serde_json::Error) -> Self { Self::Json(e) } }

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub status: String,
    #[serde(default)] pub input: Value,
    #[serde(default)] pub result: Value,
    #[serde(default)] pub error: Option<String>,
    #[serde(default)] pub session_id: Option<String>,
    #[serde(default)] pub attempts: u64,
    #[serde(default)] pub step_index: u64,
    #[serde(default)] pub step_total: u64,
}
impl Job {
    pub fn is_terminal(&self) -> bool {
        harness::RunPhase::from_host_status(&self.status)
            .is_some_and(|phase|phase.terminal())
    }
}

/// One versioned execution read-model for chat, rooms, tasks and SDK workers.
/// Only host-issued identifiers and statuses; no prompts or tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all="camelCase")]
pub struct RunView {
    #[serde(rename="ref")] pub run_ref: harness::RunRef,
    pub phase: harness::RunPhase,
    pub status: String,
    pub terminal: bool,
    pub review_required: bool,
    #[serde(default)] pub owner_action_required: bool,
    #[serde(default)] pub attempt: Option<u64>,
    #[serde(default)] pub session_id: Option<String>,
    #[serde(default)] pub provider_id: Option<String>,
    #[serde(default)] pub model: Option<String>,
    #[serde(default)] pub started_at: Option<String>,
    #[serde(default)] pub finished_at: Option<String>,
    pub source_journal: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Event {
    pub seq: u64,
    #[serde(rename = "type")] pub kind: String,
    #[serde(flatten)] pub fields: Map<String, Value>,
}
impl Event {
    /// Decode the typed RunEvent from the same global SSE sequence.
    pub fn run_event(&self)->Result<Option<harness::RunEvent>,Error>{
        self.fields.get("runEvent").filter(|v|!v.is_null())
            .map(|v|serde_json::from_value(v.clone()).map_err(Error::from))
            .transpose()
    }
}
pub type EventStream<T> = Pin<Box<dyn Stream<Item = Result<T, Error>> + Send>>;

/// A cloneable handle to DOMINION's actual Rust server (default port 4317).
/// Pass admin_token when DOMINION_ADMIN_TOKEN is configured on the server.
#[derive(Clone)]
pub struct Dominion {
    base: Url,
    client: Client,
    admin_token: Option<String>,
}
impl Dominion {
    pub fn connect(base_url: &str) -> Result<Self, Error> {
        let mut base = Url::parse(base_url).map_err(|e| Error::Url(e.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(Error::Url("DOMINION base URL must be HTTP(S)".into()));
        }
        base.set_path("/");
        base.set_query(None);
        base.set_fragment(None);
        Ok(Self { base, client: Client::new(), admin_token: None })
    }
    pub fn local() -> Self { Self::connect("http://127.0.0.1:4317").expect("valid local URL") }
    pub fn with_admin_token(mut self, token: impl Into<String>) -> Self {
        self.admin_token = Some(token.into());
        self
    }
    pub fn with_http_client(mut self, client: Client) -> Self { self.client = client; self }
    fn url(&self, path: &[&str]) -> Result<Url, Error> {
        if path.first() != Some(&"api") {
            return Err(Error::Url("Only DOMINION /api routes are supported".into()));
        }
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut()
                .map_err(|_| Error::Url("Invalid base URL".into()))?;
            segments.clear();
            segments.extend(path.iter().copied());
        }
        Ok(url)
    }
    /// Explicit history requests use path-segment encoding and bounded
    /// query parameters. They never authorize a new model or tool execution.
    fn native_history_url(
        &self, id:&str, kind:&str, before:Option<usize>, limit:usize
    )->Result<Url,Error>{
        if !(1..=40).contains(&limit) || !matches!(kind,"events"|"receipts") {
            return Err(Error::Url("Native history page limit must be 1..=40".into()));
        }
        let mut url=self.url(&["api","sdk","runs",id,kind])?;
        {
            let mut params=url.query_pairs_mut();
            params.append_pair("limit",&limit.to_string());
            if let Some(before)=before{
                params.append_pair("before",&before.to_string());
            }
        }
        Ok(url)
    }

    async fn open(&self, method: Method, url: Url, body: Option<Value>, sse: bool) -> Result<Response, Error> {
        let mut request = self.client.request(method, url).header("Accept",
            if sse { "text/event-stream" } else { "application/json" });
        if let Some(token) = &self.admin_token {
            request = request.header("X-Dominion-Token", token);
        }
        if let Some(body) = body { request = request.json(&body); }
        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let message = serde_json::from_str::<Value>(&text).ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| text.chars().take(300).collect());
            return Err(Error::Api { status, message });
        }
        Ok(response)
    }
    async fn json<T: DeserializeOwned>(&self, method: Method, path: &[&str], body: Option<Value>) -> Result<T, Error> {
        let response = self.open(method, self.url(path)?, body, false).await?;
        Ok(response.json().await?)
    }
    pub async fn get<T: DeserializeOwned>(&self, path: &[&str]) -> Result<T, Error> {
        self.json(Method::GET, path, None).await
    }
    pub async fn post<T: DeserializeOwned>(&self, path: &[&str], body: Value) -> Result<T, Error> {
        self.json(Method::POST, path, Some(body)).await
    }
    pub async fn delete<T: DeserializeOwned>(&self, path: &[&str]) -> Result<T, Error> {
        self.json(Method::DELETE, path, None).await
    }
    pub async fn health(&self) -> Result<Value, Error> { self.get(&["api", "health"]).await }
    pub async fn status(&self) -> Result<Value, Error> { self.get(&["api", "status"]).await }
    /// Read the same persistent lifecycle from any Rust product surface.
    pub async fn get_run(&self,id:&str)->Result<RunView,Error>{
        self.get(&["api","runs",id]).await
    }
    /// Read the oldest available retained Native run event page.
    /// Returned JSON includes earliestAvailable/eventsDropped when the
    /// historical 200-event ring has discarded its oldest progress.
    pub async fn native_run_events(
        &self,id:&str,before:Option<usize>,limit:usize
    )->Result<Value,Error>{
        let url=self.native_history_url(id,"events",before,limit)?;
        Ok(self.open(Method::GET,url,None,false).await?.json().await?)
    }
    /// Read real Rust kernel action receipts without tool arguments or output.
    pub async fn native_run_receipts(
        &self,id:&str,before:Option<usize>,limit:usize
    )->Result<Value,Error>{
        let url=self.native_history_url(id,"receipts",before,limit)?;
        Ok(self.open(Method::GET,url,None,false).await?.json().await?)
    }
    pub async fn agents(&self) -> Result<Vec<Value>, Error> { self.get(&["api", "agents"]).await }
    pub async fn providers(&self) -> Result<Vec<Value>, Error> { self.get(&["api", "providers"]).await }
    pub async fn sessions(&self) -> Result<Vec<Value>, Error> { self.get(&["api", "sessions"]).await }
    pub async fn create_session(&self, title: &str, agent_id: Option<&str>) -> Result<Value, Error> {
        self.post(&["api", "sessions"], json!({"title": title, "agentId": agent_id})).await
    }
    pub async fn session_messages(&self, id: &str) -> Result<Value, Error> {
        self.get(&["api", "sessions", id, "messages"]).await
    }
    pub async fn submit_prompt(&self, prompt: &str, agent_id: Option<&str>) -> Result<Job, Error> {
        self.post(&["api", "jobs"], json!({"kind":"prompt",
            "input":{"prompt":prompt,"agentId":agent_id,"toolsEnabled":true}})).await
    }
    /// Continue a private engine-owned session without creating another agent runtime.
    pub async fn submit_prompt_to_session(
        &self, session_id: &str, prompt: &str, agent_id: Option<&str>
    ) -> Result<Job, Error> {
        self.post(&["api", "jobs"], json!({"kind":"prompt",
            "input":{"prompt":prompt,"sessionId":session_id,"agentId":agent_id,"toolsEnabled":true}})).await
    }
    pub async fn run_workflow(&self, id: &str, input: &str) -> Result<Job, Error> {
        self.post(&["api", "workflows", id, "run"], json!({"input":input})).await
    }
    pub async fn get_job(&self, id: &str) -> Result<Job, Error> {
        self.get(&["api", "jobs", id]).await
    }
    pub async fn jobs(&self) -> Result<Vec<Job>, Error> { self.get(&["api", "jobs"]).await }
    pub async fn cancel_job(&self, id: &str) -> Result<Job, Error> {
        self.post(&["api", "jobs", id, "cancel"], json!({})).await
    }
    /// Mid-run owner correction; the native Rust kernel rechecks before tool batches.
    pub async fn steer_job(&self, id: &str, instruction: &str) -> Result<Value, Error> {
        self.post(&["api", "jobs", id, "directive"], json!({"instruction":instruction})).await
    }
    pub async fn wait_job(&self, id: &str, timeout: Duration, interval: Duration) -> Result<Job, Error> {
        let started = Instant::now();
        loop {
            let job = self.get_job(id).await?;
            if job.is_terminal() { return Ok(job); }
            if started.elapsed() >= timeout { return Err(Error::Timeout); }
            tokio::time::sleep(interval.max(Duration::from_millis(100))).await;
        }
    }
    pub async fn approvals(&self) -> Result<Vec<Value>, Error> {
        self.get(&["api", "approvals"]).await
    }
    pub async fn resolve_approval(&self, id: &str, allow: bool) -> Result<Value, Error> {
        self.post(&["api", "approvals"], json!({"id":id,"allow":allow})).await
    }
    /// Native Team DAG creation shares the same persisted job queue and Kernel.
    pub async fn create_task_team(&self,plan:Value)->Result<Value,Error>{
        self.post(&["api","task-teams"],plan).await
    }
    pub async fn task_teams(&self)->Result<Vec<Value>,Error>{
        self.get(&["api","task-teams"]).await
    }
    pub async fn task_team(&self,id:&str)->Result<Value,Error>{
        self.get(&["api","task-teams",id]).await
    }
    pub async fn cancel_task_team(&self,id:&str)->Result<Value,Error>{
        self.post(&["api","task-teams",id,"cancel"],json!({})).await
    }
    /// Starts a fresh owner-confirmed Team with new child IDs; no prior effects replayed.
    pub async fn retry_task_team(&self,id:&str)->Result<Value,Error>{
        self.post(&["api","task-teams",id,"retry"],json!({"confirm":true})).await
    }
    pub async fn team_bridges(&self)->Result<Value,Error>{
        self.get(&["api","task-teams","bridges"]).await
    }
    pub async fn preview_team_worktree(&self,team:&str,task:&str)->Result<Value,Error>{
        self.get(&["api","task-teams",team,"worktrees",task,"preview"]).await
    }
    /// Only stages an exact digest-approved patch; it never pushes or commits.
    pub async fn promote_team_worktree(&self,team:&str,task:&str,
        expected_diff_sha256:&str)->Result<Value,Error>{
        self.post(&["api","task-teams",team,"worktrees",task,"promote"],
            json!({"confirm":true,"expectedDiffSha256":expected_diff_sha256})).await
    }
    /// Owner-only read-only inference delegation to an opt-in remote worker.
    pub async fn remote_submit(&self,prompt:&str,max_tokens:u64)
        ->Result<Value,Error>{
        self.post(&["api","remote","tasks"],
            json!({"prompt":prompt,"maxTokens":max_tokens})).await
    }
    /// Durable per-machine inventory, model ID and current liveness.
    pub async fn remote_workers(&self)->Result<Vec<Value>,Error>{
        self.get(&["api","remote","workers"]).await
    }
    pub async fn remote_tasks(&self)->Result<Vec<Value>,Error>{
        self.get(&["api","remote","tasks"]).await
    }
    pub async fn remote_task(&self,id:&str)->Result<Value,Error>{
        self.get(&["api","remote","tasks",id]).await
    }
    pub async fn cancel_remote_task(&self,id:&str)->Result<Value,Error>{
        self.post(&["api","remote","tasks",id,"cancel"],json!({})).await
    }
    pub async fn rooms(&self) -> Result<Vec<Value>, Error> { self.get(&["api", "rooms"]).await }
    /// Paginate committed Room events and each member's durable read cursor.
    pub async fn room_events(&self,room_id:&str,after:u64,limit:usize)
        ->Result<Value,Error>{
        if !(1..=200).contains(&limit){
            return Err(Error::Url("Room event page must contain 1-200 items".into()));
        }
        let mut url=self.url(&["api","rooms",room_id,"events"])?;
        url.query_pairs_mut().append_pair("after",&after.to_string())
            .append_pair("limit",&limit.to_string());
        let response=self.open(Method::GET,url,None,false).await?;
        Ok(response.json().await?)
    }
    pub async fn run_room(&self, id: &str, goal: &str) -> Result<Value, Error> {
        self.post(&["api", "rooms", id, "run"], json!({"goal":goal})).await
    }
    pub async fn executions(&self) -> Result<Vec<Value>, Error> {
        self.get(&["api", "executions"]).await
    }
    /// Returns the same durable sequence as GET /api/events?after=N.
    /// Persist the last event seq on the caller side for crash-safe resubscription.
    pub async fn events(&self, after: u64) -> Result<EventStream<Event>, Error> {
        let mut url = self.url(&["api", "events"])?;
        url.query_pairs_mut().append_pair("after", &after.to_string());
        let response = self.open(Method::GET, url, None, true).await?;
        let mut values = parse_sse(response);
        let result = try_stream! {
            let mut cursor = after;
            while let Some(value) = values.next().await {
                let value = value?;
                let event: Event = serde_json::from_value(value)?;
                if event.seq > cursor {
                    cursor = event.seq;
                    yield event;
                }
            }
        };
        Ok(Box::pin(result))
    }
    /// POST /api/chat uses the same Rust model/tool/approval loop as the GUI.
    /// Stream end without a terminal 'done' is reported as an interruption.
    pub async fn chat(&self, request: Value) -> Result<EventStream<Value>, Error> {
        let response = self.open(Method::POST,
            self.url(&["api", "chat"])?, Some(request), true).await?;
        let mut values = parse_sse(response);
        let result = try_stream! {
            let mut finished = false;
            while let Some(item) = values.next().await {
                let event = item?;
                finished = event["type"] == "done";
                yield event;
                if finished { break; }
            }
            if !finished { Err(Error::Stream("Chat stream ended without done".into()))?; }
        };
        Ok(Box::pin(result))
    }
}

/// Byte-wise framing avoids corrupting multibyte UTF-8 split across HTTP chunks.
fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let (pos, separator) = buffer.windows(4).position(|w| w == b"\r\n\r\n")
        .map(|p| (p, 4))
        .or_else(|| buffer.windows(2).position(|w| w == b"\n\n" || w == b"\r\r").map(|p| (p, 2)))?;
    let frame = buffer[..pos].to_vec();
    buffer.drain(..pos + separator);
    Some(frame)
}
fn decode_sse_frame(frame: &[u8]) -> Result<Option<Value>, Error> {
    let text = std::str::from_utf8(frame).map_err(|e| Error::Stream(e.to_string()))?;
    let mut data = Vec::new();
    for line in text.lines() {
        if let Some(part) = line.strip_prefix("data:") {
            data.push(part.strip_prefix(' ').unwrap_or(part));
        }
    }
    if data.is_empty() { return Ok(None); }
    Ok(Some(serde_json::from_str(&data.join("\n"))?))
}
fn parse_sse(response: Response) -> EventStream<Value> {
    let result = try_stream! {
        let mut incoming = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = incoming.next().await {
            buffer.extend_from_slice(&chunk?);
            if buffer.len() > 2 * 1024 * 1024 {
                Err(Error::Stream("SSE frame exceeds 2 MiB".into()))?;
            }
            while let Some(frame) = take_sse_frame(&mut buffer) {
                if let Some(value) = decode_sse_frame(&frame)? { yield value; }
            }
        }
    };
    Box::pin(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls_encode_ids_and_use_existing_engine_routes() {
        let sdk = Dominion::local();
        assert_eq!(sdk.url(&["api", "jobs", "a/b", "cancel"]).unwrap().path(), "/api/jobs/a%2Fb/cancel");
        assert!(sdk.url(&["v1", "runs"]).is_err());
    }
    #[test]
    fn native_history_routes_are_safe_and_cursors_are_explicit(){
        let sdk=Dominion::local();
        let events=sdk.native_history_url("owner/a","events",Some(80),40).unwrap();
        assert_eq!(events.path(),"/api/sdk/runs/owner%2Fa/events");
        assert_eq!(events.query_pairs().find(|(name,_)|name=="before")
            .map(|(_,value)|value.to_string()),Some("80".into()));
        assert_eq!(events.query_pairs().find(|(name,_)|name=="limit")
            .map(|(_,value)|value.to_string()),Some("40".into()));
        let receipts=sdk.native_history_url("owner/a","receipts",None,10).unwrap();
        assert_eq!(receipts.path(),"/api/sdk/runs/owner%2Fa/receipts");
        assert!(!receipts.query_pairs().any(|(key,_)|key=="before"));
        assert!(sdk.native_history_url("x","events",None,0).is_err());
        assert!(sdk.native_history_url("x","events",None,41).is_err());
        assert!(sdk.native_history_url("x","execute",None,40).is_err());
    }

    #[test]
    fn sse_handles_partial_frames_multiline_and_comments() {
        let mut buffer = b": keepalive\r\n\r\ndata: {\"seq\":5,\"type\":\"record\"}\r\n".to_vec();
        assert!(decode_sse_frame(&take_sse_frame(&mut buffer).unwrap()).unwrap().is_none());
        assert!(take_sse_frame(&mut buffer).is_none());
        buffer.extend_from_slice(b"\r\ndata: {\"seq\":6,\n");
        let frame = take_sse_frame(&mut buffer).unwrap();
        assert_eq!(decode_sse_frame(&frame).unwrap().unwrap()["seq"], 5);
        assert!(take_sse_frame(&mut buffer).is_none());
    }
    #[test]
    fn canonical_cross_surface_run_view_roundtrips(){
        let view:RunView=serde_json::from_value(json!({
            "ref":{"version":1,"surface":"room","runId":"room-1","parentRunId":null},
            "phase":"waiting_approval","status":"running","terminal":false,
            "reviewRequired":false,"sourceJournal":"execution"
        })).unwrap();
        assert_eq!(view.run_ref.surface,harness::RunSurface::Room);
        assert_eq!(view.phase,harness::RunPhase::WaitingApproval);
        assert!(!view.terminal);
        assert_eq!(serde_json::to_value(&view).unwrap()["ref"]["runId"],"room-1");
        assert_eq!(Dominion::local().url(&["api","runs","a/b"]).unwrap().path(),
            "/api/runs/a%2Fb");
    }
    #[test]
    fn remote_tasks_and_run_ids_use_owner_scoped_existing_rest_routes(){
        let client=Dominion::local();
        assert_eq!(client.url(&["api","remote","tasks","remote:a/b"]).unwrap().path(),
            "/api/remote/tasks/remote:a%2Fb");
        assert_eq!(client.url(&["api","remote","tasks","remote:a/b","cancel"])
            .unwrap().path(),"/api/remote/tasks/remote:a%2Fb/cancel");
        assert_eq!(client.url(&["api","runs","remote:a/b"]).unwrap().path(),
            "/api/runs/remote:a%2Fb");
    }
    #[test] fn sdk_replays_canonical_execution_envelope(){
        let raw=json!({"seq":12,"type":"record","kind":"remote_task",
            "runEvent":{"version":1,"eventSeq":12,
              "runRef":{"version":1,"surface":"external_agent",
                 "runId":"remote:12","parentRunId":null},
              "phase":"needs_review","eventType":"record",
              "sourceJournal":"remote_task"}});
        let event:Event=serde_json::from_value(raw).unwrap();
        let envelope=event.run_event().unwrap().unwrap();
        assert_eq!(envelope.event_seq,event.seq);
        assert_eq!(envelope.run_ref.surface,harness::RunSurface::ExternalAgent);
        assert_eq!(envelope.phase,harness::RunPhase::NeedsReview);
    }
    #[test]
    fn team_review_routes_remain_scoped_to_task_ids(){
        let client=Dominion::local();
        let path=client.url(&["api","task-teams","lead/a","worktrees",
            "task/b","preview"]).unwrap();
        assert_eq!(path.path(),
            "/api/task-teams/lead%2Fa/worktrees/task%2Fb/preview");
    }
    #[test]
    fn terminal_status_is_engine_owned() {
        for status in ["completed","succeeded","failed","denied","exhausted",
            "cancelled","interrupted","needs_review","in_doubt"] {
            let job = Job { id: "j".into(), kind: "prompt".into(), status: status.into(),
                input: Value::Null,result:Value::Null,error:None,session_id:None,
                attempts:0,step_index:0,step_total:0 };
            assert!(job.is_terminal());
        }
    }
}
