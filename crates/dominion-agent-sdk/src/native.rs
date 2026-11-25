//! Versioned, provider-neutral native embedding API shared by DOMINION branches.
//! All iteration remains in the selected native engine. The application owns
//! provider I/O, approval/sandbox policy, independent verification and durable storage.
use serde_json::{json, Value};
use std::{collections::HashSet, future::Future, pin::Pin, sync::Mutex, time::{Duration, Instant}};
mod backend;
mod control;
mod context;
pub use context::ProviderContext;
pub use control::*;
use std::sync::atomic::{AtomicBool,Ordering};
pub const CHECKPOINT_VERSION:u32=2;
const SUSPENDED:&str="native run suspended at a durable model boundary";

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
    fn binding(&self,options:&RunOptions<'_>)->Value{
        let mut binding=json!({"id":self.id,"instruction":self.instruction,"system":self.system,
            "tools":self.tools.iter().map(|t|json!({"name":t.name,"description":t.description,"parameters":t.parameters,"readOnly":t.read_only})).collect::<Vec<_>>(),
            "limits":{"model":self.limits.max_model_calls,"tools":self.limits.max_tool_calls,
            "elapsedSeconds":self.limits.max_elapsed.as_secs(),"elapsedNanos":self.limits.max_elapsed.subsec_nanos(),
            "tokens":self.limits.max_tokens,"verificationRetries":options.max_verification_retries}});
        if let Some(policy)=options.provider_context{
            binding["providerContext"]=json!({"version":1,"historyBytes":policy.history_bytes,"repeatGuard":policy.repeat_guard});
        }
        binding
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
    ModelDelta { round:usize, delta:ModelDelta },
    ContextPrepared { round:usize, elapsed_us:u64 },
    VerificationStarted,
    VerificationRejected { reason:String, will_retry:bool },
    Suspended,
    Failed { error:String },
    Verified,
}
/// The host must exclusively lease a run ID, enforce permissions/approvals,
/// persist snapshots atomically and never retry unknown external effects.
/// No provider, HTTP server, database, scheduler or shell is supplied here.
pub trait NativeHost: Send + Sync {
    fn complete(&self, request: ModelRequest) -> NativeFuture<'_, ModelResponse>;
    fn supports_streaming(&self)->bool{false}
    /// Must forward actual provider deltas; never synthesize a stream from final text.
    fn complete_streamed<'a>(&'a self,_request:ModelRequest,_emit:&'a(dyn Fn(ModelDelta)+Send+Sync))->NativeFuture<'a,ModelResponse>{
        Box::pin(async{Err("host has no streaming model port".into())})
    }
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
    /// Lifecycle events and actual provider deltas when streaming is enabled.
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
    rejected:usize,
}
impl Meter{
    fn fresh()->Self{Self{model_calls:0,tool_calls:0,input_tokens:Some(0),output_tokens:Some(0),rejected:0}}
    fn value(self)->Value{json!({"modelCalls":self.model_calls,"toolCalls":self.tool_calls,"inputTokens":self.input_tokens,"outputTokens":self.output_tokens,"rejected":self.rejected})}
    fn parse(v:&Value)->Result<Self,String>{
        fn count(v:&Value,k:&str)->Result<usize,String>{usize::try_from(v[k].as_u64().ok_or_else(||format!("invalid checkpoint {k}"))?).map_err(|_|format!("checkpoint {k} overflow"))}
        fn tokens(v:&Value,k:&str)->Result<Option<u64>,String>{match v.get(k){
            Some(Value::Null)=>Ok(None),Some(n)=>n.as_u64().map(Some).ok_or_else(||format!("invalid checkpoint {k}")),None=>Err(format!("missing checkpoint {k}"))}}
        Ok(Self{model_calls:count(v,"modelCalls")?,tool_calls:count(v,"toolCalls")?,input_tokens:tokens(v,"inputTokens")?,output_tokens:tokens(v,"outputTokens")?,rejected:count(v,"rejected")?})
    }
}
struct Session<'a> {
    host: &'a dyn NativeHost,
    request: &'a RunRequest,
    started: Instant,
    meter: Mutex<Meter>,
    state: Mutex<Value>,
    options:RunOptions<'a>,
    elapsed_before:Duration,
    suspended:AtomicBool,
    uncertain_effect:AtomicBool,
}
impl<'a> Session<'a> {
    fn check(&self) -> Result<(),String> {
        if self.suspended.load(Ordering::Acquire){return Err(SUSPENDED.into());}
        if self.uncertain_effect.load(Ordering::Acquire){return Err("external effect outcome uncertain; reconcile before further execution".into());}
        if self.host.cancelled(&self.request.id)||self.options.control.as_ref().is_some_and(RunControl::is_cancelled){return Err("native run cancelled".into());}
        if self.elapsed()>=self.request.limits.max_elapsed{return Err("native admission deadline exceeded".into());}
        Ok(())
    }
    fn cancelled(&self) -> bool { self.check().is_err() }
    fn elapsed(&self)->Duration{self.elapsed_before.saturating_add(self.started.elapsed())}
    fn elapsed_ms(&self)->u64{self.elapsed().as_millis().min(u64::MAX as u128) as u64}
    fn meter(&self)->Result<Meter,String>{Ok(*self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?)}
    fn emit(&self,event:Event){
        self.host.event(&self.request.id,event.clone());
        for hook in &self.options.hooks{hook.event(&self.request.id,&event);}
        if let Some(tx)=&self.options.events{let _=tx.send(event);}
    }
    fn envelope(&self,state:&Value,meter:Meter,verified:bool,suspended:bool)->Value{
        json!({"apiVersion":API_VERSION,"checkpointVersion":CHECKPOINT_VERSION,"engine":backend::ENGINE_ID,
            "request":self.request.binding(&self.options),"elapsedMs":self.elapsed_ms(),"meter":meter.value(),
            "state":state,"verified":verified,"suspended":suspended,
            "uncertainEffect":self.uncertain_effect.load(Ordering::Acquire)})
    }
    fn save(&self,state:Value)->Result<(),String>{
        let mut current=self.state.lock().map_err(|_|"checkpoint lock poisoned".to_string())?;
        if self.suspended.load(Ordering::Acquire){return Err(SUSPENDED.into());}
        self.host.save_checkpoint(&self.request.id,&self.envelope(&state,self.meter()?,false,false))?;
        *current=state;
        Ok(())
    }
    // Only at the live boundary BEFORE the provider future is invoked.
    fn suspend(&self)->Result<(),String>{
        let mut current=self.state.lock().map_err(|_|"checkpoint lock poisoned".to_string())?;
        let meter=self.meter()?;
        let settled=backend::suspend_state(&current,meter)?;
        self.host.save_checkpoint(&self.request.id,&self.envelope(&settled,meter,false,true))?;
        *current=settled;
        self.suspended.store(true,Ordering::Release);
        drop(current);
        self.emit(Event::Suspended);
        Err(SUSPENDED.into())
    }
    async fn admit(&self,operation:Operation)->Result<(),String>{
        self.check()?;
        if let Some(control)=&self.options.control{
            if control.wait(&operation,||self.check()).await?{self.check()?;return self.suspend();}
        }
        for hook in &self.options.hooks{
            let m=self.meter()?;
            match hook.before(HookContext{run_id:self.request.id.clone(),operation:operation.clone(),model_calls:m.model_calls,tool_calls:m.tool_calls}).await?{
                HookDecision::Allow=>{},
                HookDecision::Reject(reason)=>return Err(format!("native hook rejected operation: {reason}")),
            }
        }
        self.check()
    }
    async fn model(&self,messages:Vec<Value>,tools:Vec<Value>,observations:Vec<Value>) -> Result<ModelResponse,String> {
        self.admit(Operation::Model).await?;
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
        let mut request=ModelRequest{run_id:self.request.id.clone(),round,messages,tools,observations};
        if let Some(policy)=self.options.provider_context{
            let started=Instant::now();
            context::prepare(&mut request,&self.request.tools,policy);
            self.emit(Event::ContextPrepared{round,elapsed_us:started.elapsed().as_micros().min(u64::MAX as u128) as u64});
        }
        self.check()?;
        let response=if self.options.streaming{
            let emit=|delta|self.emit(Event::ModelDelta{round,delta});
            self.host.complete_streamed(request,&emit).await?
        }else{self.host.complete(request).await?};
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
        let spec=self.request.tools.iter().find(|t|t.name==call.name).ok_or("tool not authorized by host catalogue")?;
        let read_only=spec.read_only;
        self.admit(Operation::Tool{id:call.id.clone(),name:call.name.clone(),arguments:call.arguments.clone()}).await?;
        let round={
            let mut m=self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?;
            if m.tool_calls>=self.request.limits.max_tool_calls { return Err("tool budget exhausted".into()); }
            m.tool_calls+=1; m.model_calls
        };
        let id=call.id.clone();
        self.emit(Event::ToolStarted{round,id:id.clone(),name:call.name.clone()});
        let result=match self.host.dispatch(&self.request.id,call).await{
            Ok(outcome)=>outcome,
            Err(error)=>{
                // An error from an effectful port does not establish that the
                // effect failed. Some engines turn errors into observations:
                // fence the portable session as well, preventing any new I/O
                // or verified completion even on that path.
                if !read_only{
                    self.uncertain_effect.store(true,Ordering::Release);
                    return Err(format!("external effect outcome uncertain: {error}"));
                }
                return Err(error);
            }
        };
        self.emit(Event::ToolFinished{round,id,success:result.success});
        Ok(result)
    }
    async fn verify_candidate(&self,answer:&str)->Result<Verification,String>{
        self.admit(Operation::Verify).await?;
        self.emit(Event::VerificationStarted);
        let verdict=self.host.verify(&self.request.id,answer.to_owned()).await?;
        self.check()?;
        match verdict{
            Verification::Accept=>Ok(Verification::Accept),
            Verification::Reject(reason)=>{
                let reason=reason.chars().take(8192).collect::<String>();
                let retry={
                    let mut m=self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?;
                    m.rejected=m.rejected.saturating_add(1);
                    m.rejected<=self.options.max_verification_retries&&m.model_calls<self.request.limits.max_model_calls
                };
                self.emit(Event::VerificationRejected{reason:reason.clone(),will_retry:retry});
                if retry{Ok(Verification::Reject(reason))}else{Err(format!("independent verification rejected: {reason}"))}
            }
        }
    }
    async fn evidence(&self,criterion:Value)->Result<bool,String>{
        self.admit(Operation::Evidence{criterion:criterion.clone()}).await?;
        let result=self.host.check_evidence(&self.request.id,criterion).await?;
        self.check()?;
        Ok(result)
    }
    fn finish(&self,answer:String) -> Result<RunResult,String> {
        self.check()?;
        let m=*self.meter.lock().map_err(|_|"meter lock poisoned".to_string())?;
        let result=RunResult{answer,model_calls:m.model_calls,tool_calls:m.tool_calls,
            input_tokens:m.input_tokens,output_tokens:m.output_tokens,
            elapsed_ms:self.elapsed_ms()};
        let state=self.state.lock().map_err(|_|"checkpoint lock poisoned".to_string())?;
        let mut envelope=self.envelope(&state,m,true,false);
        envelope["result"]=json!({"answer":result.answer,"modelCalls":result.model_calls,"toolCalls":result.tool_calls,
            "inputTokens":result.input_tokens,"outputTokens":result.output_tokens,"elapsedMs":result.elapsed_ms});
        self.host.save_checkpoint(&self.request.id,&envelope)?;
        drop(state);
        self.emit(Event::Verified);
        Ok(result)
    }
}
/// Fresh run through the selected native engine. Never reuses a run ID.
pub async fn run(host:&dyn NativeHost,request:RunRequest)->Result<RunResult,String>{
    run_with_options(host,request,RunOptions::default()).await
}
pub async fn run_with_options(host:&dyn NativeHost,request:RunRequest,options:RunOptions<'_>)->Result<RunResult,String>{
    run_internal(host,request,options,false).await
}
/// Continue an explicit safe suspension, or return an already verified result.
/// Arbitrary crash/unknown-effect checkpoints are not safe suspensions.
pub async fn resume(host:&dyn NativeHost,request:RunRequest)->Result<RunResult,String>{
    resume_with_options(host,request,RunOptions::default()).await
}
pub async fn resume_with_options(host:&dyn NativeHost,request:RunRequest,options:RunOptions<'_>)->Result<RunResult,String>{
    run_internal(host,request,options,true).await
}
async fn run_internal(host:&dyn NativeHost,request:RunRequest,options:RunOptions<'_>,resume:bool)->Result<RunResult,String>{
    request.validate()?;
    if options.max_verification_retries>100{return Err("invalid verification retry budget".into());}
    if options.provider_context.is_some_and(|p|p.history_bytes==0||p.history_bytes>1_048_576){
        return Err("invalid provider context byte budget".into());
    }
    if options.streaming&&!host.supports_streaming(){return Err("host has no streaming model port".into());}
    let previous=host.load_checkpoint(&request.id)?;
    let (meter,state,elapsed,restored)=match(resume,previous){
        (false,None)=>(Meter::fresh(),Value::Null,Duration::ZERO,None),
        (false,Some(_))=>return Err("run ID already exists; use explicit resume".into()),
        (true,None)=>return Err("no checkpoint to resume".into()),
        (true,Some(saved))=>{
            if saved["apiVersion"]!=API_VERSION||saved["checkpointVersion"]!=CHECKPOINT_VERSION
                ||saved["engine"]!=backend::ENGINE_ID||saved["request"]!=request.binding(&options){
                return Err("checkpoint version, engine or request mismatch".into());
            }
            if saved["uncertainEffect"]==true{return Err("external effect outcome uncertain; reconcile manually".into());}
            let meter=Meter::parse(&saved["meter"])?;
            if meter.model_calls>request.limits.max_model_calls||meter.tool_calls>request.limits.max_tool_calls
                ||meter.rejected>options.max_verification_retries.saturating_add(1){return Err("checkpoint exceeds limits".into());}
            let ms=saved["elapsedMs"].as_u64().ok_or("missing checkpoint elapsed time")?;
            if saved["verified"]==true{
                if saved["suspended"]==true{return Err("invalid verified suspension".into());}
                let result=&saved["result"];
                let answer=result["answer"].as_str().filter(|s|!s.trim().is_empty()).ok_or("missing verified answer")?;
                return Ok(RunResult{answer:answer.into(),model_calls:meter.model_calls,tool_calls:meter.tool_calls,
                    input_tokens:meter.input_tokens,output_tokens:meter.output_tokens,elapsed_ms:result["elapsedMs"].as_u64().ok_or("missing verified elapsed time")?});
            }
            if saved["suspended"]!=true{return Err("checkpoint is not an explicit safe suspension; reconcile uncertain work manually".into());}
            let state=saved.get("state").filter(|v|!v.is_null()).ok_or("missing suspended state")?.clone();
            (meter,state.clone(),Duration::from_millis(ms),Some(state))
        }
    };
    let session=Session{host,request:&request,started:Instant::now(),meter:Mutex::new(meter),state:Mutex::new(state),
        options,elapsed_before:elapsed,suspended:AtomicBool::new(false),uncertain_effect:AtomicBool::new(false)};
    session.check()?;
    if restored.is_none(){session.save(json!({"phase":"created"}))?;}
    let result=backend::execute(&session,restored).await;
    if session.suspended.load(Ordering::Acquire){return Err(SUSPENDED.into());}
    if session.uncertain_effect.load(Ordering::Acquire){
        // The native engine has already journaled its own transition if it
        // could; persist the additional fence against portable replay.
        let state=session.state.lock().map_err(|_|"checkpoint lock poisoned".to_string())?;
        session.host.save_checkpoint(&session.request.id,&session.envelope(&state,session.meter()?,false,false))?;
        return Err("external effect outcome uncertain; reconcile before further execution".into());
    }
    session.finish(result?)
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
