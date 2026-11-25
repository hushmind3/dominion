//! Versioned, provider-neutral native embedding API shared by DOMINION branches.
//! All iteration remains in the selected native engine. The application owns
//! provider I/O, approval/sandbox policy, independent verification and durable storage.
use serde_json::{json, Value};
use std::{collections::HashSet, future::Future, pin::Pin, sync::Mutex, time::{Duration, Instant}};
mod backend;

pub const API_VERSION: u32 = 1;
pub type NativeFuture<'a, T> = Pin<Box<dyn Future<Output=Result<T,String>> + Send + 'a>>;

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub read_only: bool,
}
impl ToolSpec {
    pub fn provider_schema(&self) -> Value {
        json!({"type":"function","function":{"name":self.name,"description":self.description,
            "parameters":self.parameters}})
    }
}
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_model_calls: usize,
    pub max_tool_calls: usize,
    /// Admission deadline; in-flight provider/tool timeouts belong to the host.
    pub max_elapsed: Duration,
    /// If set, missing usage or exceeding the cap fails closed before effects.
    pub max_tokens: Option<u64>,
}
impl Default for Limits {
    fn default() -> Self {
        Self { max_model_calls: 24, max_tool_calls: 96,
            max_elapsed: Duration::from_secs(900), max_tokens: None }
    }
}
#[derive(Clone, Debug)]
pub struct RunRequest {
    pub id: String,
    pub instruction: String,
    pub system: String,
    pub tools: Vec<ToolSpec>,
    pub limits: Limits,
}
impl RunRequest {
    pub fn new(id: impl Into<String>, instruction: impl Into<String>) -> Self {
        Self { id:id.into(), instruction:instruction.into(), system:String::new(),
            tools:vec![], limits:Limits::default() }
    }
    fn validate(&self) -> Result<(),String> {
        if self.id.trim().is_empty() || self.id.len()>128 || self.instruction.trim().is_empty()
            || self.instruction.len()>64000 || self.system.len()>32000
            || !(1..=100).contains(&self.limits.max_model_calls)
            || self.limits.max_tool_calls>4096 || self.limits.max_elapsed.is_zero()
            || self.limits.max_tokens.is_some_and(|n| !(1..=20_000_000).contains(&n))
            || self.tools.len()>1024 {
            return Err("invalid native run request or limits".into());
        }
        let mut names=HashSet::new();
        for t in &self.tools {
            if t.name.is_empty() || t.name.len()>128 || !t.name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'_'|b'-'|b'.'))
                || !names.insert(t.name.as_str()) || !t.parameters.is_object() {
                return Err("invalid or duplicate native tool schema".into());
            }
        }
        Ok(())
    }
    fn catalog(&self) -> Vec<Value> {
        if self.limits.max_tool_calls==0 { return vec![]; }
        self.tools.iter().map(ToolSpec::provider_schema).collect()
    }
    fn initial_messages(&self) -> Vec<Value> {
        let mut messages=Vec::new();
        if !self.system.is_empty() { messages.push(json!({"role":"system","content":self.system})); }
        messages.push(json!({"role":"user","content":self.instruction}));
        messages
    }
}
#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone, Debug)]
pub struct ModelRequest {
    pub run_id: String,
    pub round: usize,
    /// Native provider transcript or the engine's projected planner prompt.
    /// Never replace assistant/tool pairs with a receipt-only reconstruction.
    pub messages: Vec<Value>,
    /// The exact currently advertised provider tool schema catalogue.
    pub tools: Vec<Value>,
    /// Full native observations when the underlying engine supplies them.
    /// Separate from model-visible context; the host may inspect without truncating storage.
    pub observations: Vec<Value>,
}
#[derive(Clone, Debug)]
pub struct ModelResponse {
    pub content: String,
    pub calls: Vec<ToolCall>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct ToolOutcome {
    pub value: Value,
    pub success: bool,
}
impl ToolOutcome {
    fn observation(&self) -> Value {
        if self.success { return self.value.clone(); }
        match self.value.clone() {
            Value::Object(mut obj) => { obj.insert("success".into(),Value::Bool(false)); Value::Object(obj) }
            value => json!({"success":false,"result":value}),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verification { Accept, Reject(String) }
#[derive(Clone, Debug)]
pub enum Event {
    ModelStarted { round: usize },
    ModelFinished { round: usize, input_tokens: Option<u64>, output_tokens: Option<u64> },
    ToolStarted { round: usize, id: String, name: String },
    ToolFinished { round: usize, id: String, success: bool },
    Verified,
}
/// The host must exclusively lease a run ID, enforce permissions/approvals,
/// persist snapshots atomically and never retry unknown external effects.
/// No provider, HTTP server, database, scheduler or shell is supplied here.
pub trait NativeHost: Send + Sync {
    fn complete(&self, request: ModelRequest) -> NativeFuture<'_, ModelResponse>;
    fn dispatch(&self, run_id: &str, call: ToolCall) -> NativeFuture<'_, ToolOutcome>;
    fn verify(&self, run_id: &str, answer: String) -> NativeFuture<'_, Verification>;
    /// Optional independent evidence for native file/acceptance criteria.
    /// Unsupported checks fail closed, not true.
    fn check_evidence(&self, _run_id: &str, _criterion: Value) -> NativeFuture<'_, bool> {
        Box::pin(async { Err("host does not implement independent evidence checks".into()) })
    }
    fn load_checkpoint(&self, run_id: &str) -> Result<Option<Value>,String>;
    fn save_checkpoint(&self, run_id: &str, checkpoint: &Value) -> Result<(),String>;
    fn cancelled(&self, _run_id: &str) -> bool { false }
    /// Turn/tool lifecycle events; this is not token-delta streaming.
    fn event(&self, _run_id: &str, _event: Event) {}
}
#[derive(Clone, Debug)]
pub struct RunResult {
    pub answer: String,
    pub model_calls: usize,
    pub tool_calls: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub elapsed_ms: u64,
}
impl RunResult {
    pub fn total_tokens(&self) -> Option<u64> {
        Some(self.input_tokens?.saturating_add(self.output_tokens?))
    }
}
#[derive(Clone, Copy)]
struct Meter {
    model_calls: usize,
    tool_calls: usize,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}
struct Session<'a> {
    host: &'a dyn NativeHost,
    request: &'a RunRequest,
    started: Instant,
    meter: Mutex<Meter>,
    state: Mutex<Value>,
}
impl<'a> Session<'a> {
    fn check(&self) -> Result<(),String> {
        if self.host.cancelled(&self.request.id) { return Err("native run cancelled".into()); }
        if self.started.elapsed() >= self.request.limits.max_elapsed { return Err("native admission deadline exceeded".into()); }
        Ok(())
    }
    fn cancelled(&self) -> bool { self.check().is_err() }
    fn emit(&self,event:Event) { self.host.event(&self.request.id,event); }
    fn save(&self,state:Value) -> Result<(),String> {
        let mut current=self.state.lock().map_err(|_|"checkpoint lock poisoned".to_string())?;
        self.host.save_checkpoint(&self.request.id,&json!({"apiVersion":API_VERSION,"state":state,"verified":false}))?;
        *current=state;
        Ok(())
    }
    async fn model(&self,messages:Vec<Value>,tools:Vec<Value>,observations:Vec<Value>) -> Result<ModelResponse,String> {
        self.check()?;
        let round={
            let mut m=self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?;
            if m.model_calls>=self.request.limits.max_model_calls { return Err("model-call budget exhausted".into()); }
            if let Some(cap)=self.request.limits.max_tokens {
                let used=m.input_tokens.zip(m.output_tokens).map(|(a,b)|a.saturating_add(b))
                    .ok_or("provider usage is required by the token cap")?;
                if used>=cap { return Err("token budget exhausted".into()); }
            }
            m.model_calls+=1; m.model_calls
        };
        self.emit(Event::ModelStarted{round});
        let response=self.host.complete(ModelRequest{run_id:self.request.id.clone(),round,messages,tools,observations}).await?;
        {
            let mut m=self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?;
            m.input_tokens=m.input_tokens.zip(response.input_tokens).map(|(a,b)|a.saturating_add(b));
            m.output_tokens=m.output_tokens.zip(response.output_tokens).map(|(a,b)|a.saturating_add(b));
            if let Some(cap)=self.request.limits.max_tokens {
                let used=m.input_tokens.zip(m.output_tokens).map(|(a,b)|a.saturating_add(b))
                    .ok_or("provider usage is required by the token cap")?;
                if used>cap { return Err("provider usage exceeded token budget; no effects admitted".into()); }
            }
            if response.calls.len()>self.request.limits.max_tool_calls.saturating_sub(m.tool_calls) {
                return Err("tool batch exceeds remaining host tool budget".into());
            }
        }
        self.emit(Event::ModelFinished{round,input_tokens:response.input_tokens,output_tokens:response.output_tokens});
        let mut ids=HashSet::new();
        if response.calls.iter().any(|c|c.id.trim().is_empty() || !ids.insert(c.id.as_str()) || !c.arguments.is_object()) {
            return Err("invalid native tool call identity or arguments".into());
        }
        if response.calls.is_empty() && response.content.trim().is_empty() {
            return Err("empty model answer".into());
        }
        self.check()?;
        Ok(response)
    }
    async fn tool(&self,call:ToolCall) -> Result<ToolOutcome,String> {
        self.check()?;
        if !self.request.tools.iter().any(|t|t.name==call.name) { return Err("tool not authorized by host catalogue".into()); }
        let round={
            let mut m=self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?;
            if m.tool_calls>=self.request.limits.max_tool_calls { return Err("tool budget exhausted".into()); }
            m.tool_calls+=1; m.model_calls
        };
        let id=call.id.clone();
        self.emit(Event::ToolStarted{round,id:id.clone(),name:call.name.clone()});
        let result=self.host.dispatch(&self.request.id,call).await?;
        self.emit(Event::ToolFinished{round,id,success:result.success});
        Ok(result)
    }
    async fn verify(&self,answer:&str) -> Result<(),String> {
        self.check()?;
        match self.host.verify(&self.request.id,answer.to_owned()).await? {
            Verification::Accept => { self.check()?; Ok(()) }
            Verification::Reject(reason) => Err(format!("independent verification rejected: {reason}")),
        }
    }
    fn finish(&self,answer:String) -> Result<RunResult,String> {
        self.check()?;
        let m=*self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?;
        let result=RunResult{answer,model_calls:m.model_calls,tool_calls:m.tool_calls,
            input_tokens:m.input_tokens,output_tokens:m.output_tokens,
            elapsed_ms:self.started.elapsed().as_millis().min(u64::MAX as u128) as u64};
        let state=self.state.lock().map_err(|_|"checkpoint lock poisoned".to_string())?;
        self.host.save_checkpoint(&self.request.id,&json!({"apiVersion":API_VERSION,
            "state":&*state,"verified":true,"result":{"answer":result.answer,
            "modelCalls":result.model_calls,"toolCalls":result.tool_calls,
            "inputTokens":result.input_tokens,"outputTokens":result.output_tokens,
            "elapsedMs":result.elapsed_ms}}))?;
        drop(state);
        self.emit(Event::Verified);
        Ok(result)
    }
}
/// Execute a fresh task through the selected branch's original native engine.
/// Success means independently verified completion, not just an LLM answer.
/// A pre-existing checkpoint is rejected; no implicit resume/replay.
pub async fn run(host:&dyn NativeHost, request:RunRequest) -> Result<RunResult,String> {
    request.validate()?;
    if host.load_checkpoint(&request.id)?.is_some() { return Err("run ID already exists; inspect checkpoint before recovery".into()); }
    let session=Session{host,request:&request,started:Instant::now(),
        meter:Mutex::new(Meter{model_calls:0,tool_calls:0,input_tokens:Some(0),output_tokens:Some(0)}),
        state:Mutex::new(Value::Null)};
    session.check()?;
    session.save(json!({"phase":"created"}))?;
    let answer=backend::execute(&session).await?;
    session.finish(answer)
}

/// Sequential batch uses the same engine and individual durable identities.
/// Stops on first error; previous verified outcomes remain committed.
pub async fn run_batch(host:&dyn NativeHost, requests:Vec<RunRequest>) -> Result<Vec<RunResult>,String> {
    let mut ids=HashSet::new();
    for r in &requests {
        r.validate()?;
        if !ids.insert(r.id.clone()) || host.load_checkpoint(&r.id)?.is_some() {
            return Err("duplicate or previously used batch run ID".into());
        }
    }
    let mut results=Vec::with_capacity(requests.len());
    for r in requests { results.push(run(host,r).await?); }
    Ok(results)
}
#[cfg(test)]
mod tests;
