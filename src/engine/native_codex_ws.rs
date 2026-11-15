//! Job-scoped Codex Responses WebSocket transport owned by Dominion Rust.
//!
//! Only a completed provider response advances the chain. The job journal
//! owns paid-call uncertainty, while this transport owns only the ephemeral
//! socket and response ancestry; no external agent runner is involved.
use crate::{engine::{gateway, native_model::{StreamProgress, tool_text}, native_stream::{Assembler,Packet,Wire}},state::AppState};
use dominion_sdk::harness::Receipt;
use futures_util::{SinkExt,StreamExt};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{HashMap,HashSet},sync::{Arc,LazyLock,Mutex},time::{Duration,Instant}};
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::{tungstenite::{client::IntoClientRequest,http::HeaderValue,Message},
    WebSocketStream,MaybeTlsStream};

type Socket=WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
const MAX_AGE:Duration=Duration::from_secs(50*60);
const IDLE_TIMEOUT:Duration=Duration::from_secs(180);
const MAX_JOB_CHAINS:usize=64;

#[derive(Debug)]
pub(crate) enum WsError {
    /// Failed before any request could reach the provider; HTTP is safe.
    Unavailable(String),
    /// Provider explicitly rejected a request, not a transport disconnection.
    Rejected(String),
    /// Model inference may already have happened; no automatic generation retry.
    Uncertain(String),
}

#[derive(Debug,PartialEq,Eq)]
enum Plan { Full, Delta{input:Vec<Value>,received:usize} }

struct JobChain {
    socket:Option<Socket>,
    opened:Option<Instant>,
    touched:Instant,
    response_id:Option<String>,
    stable_contract:Option<String>,
    acknowledged_receipts:Vec<String>,
    outstanding_calls:Vec<String>,
    /// Observed provider context usage after the last completed response.
    /// None means the provider did not report it; never invent a cache hit.
    context_tokens:Option<u64>,
}
impl JobChain {
    fn new()->Self {
        Self{socket:None,opened:None,touched:Instant::now(),
            response_id:None,stable_contract:None,
            acknowledged_receipts:Vec::new(),outstanding_calls:Vec::new(),
            context_tokens:None}
    }
    fn reset(&mut self) {
        self.socket=None;
        self.opened=None;
        self.response_id=None;
        self.stable_contract=None;
        self.acknowledged_receipts.clear();
        self.outstanding_calls.clear();
        self.context_tokens=None;
    }
}

static CHAINS:LazyLock<Mutex<HashMap<String,Arc<AsyncMutex<JobChain>>>>>=
    LazyLock::new(||Mutex::new(HashMap::new()));

fn key(route:&gateway::Route,account:&str,run_id:&str,access:&str)->String {
    // An OAuth refresh must not silently carry forward a socket authenticated
    // under an earlier token. Only a non-reversible digest enters this key.
    let credential=format!("{:x}",Sha256::digest(access.as_bytes()));
    format!("{run_id}\0{}\0{}\0{}\0{account}\0{credential}",
        route.id,route.model,route.base_url)
}
fn job_live(state:&AppState,session:&str,job_id:&str)->bool {
    !state.is_cancelled(session)
        && state.store.get("job",job_id).is_some_and(|job|
            job["status"]=="running" && job["cancelRequested"]!=true)
}
fn chain_for(key:&str)->Arc<AsyncMutex<JobChain>> {
    let mut guard=CHAINS.lock().unwrap();
    guard.retain(|_,chain|
        chain.try_lock().map(|c|c.touched.elapsed()<Duration::from_secs(3600))
            .unwrap_or(true));
    if guard.len()>=MAX_JOB_CHAINS && !guard.contains_key(key) {
        if let Some(oldest)=guard.iter().min_by_key(|(_,chain)|
            chain.try_lock().map(|c|c.touched)
                .unwrap_or_else(|_|Instant::now()))
            .map(|(key,_)|key.clone()) {guard.remove(&oldest);}
    }
    guard.entry(key.to_string())
        .or_insert_with(||Arc::new(AsyncMutex::new(JobChain::new()))).clone()
}

/// Release process-local socket state when the Rust job ends. Durable receipts
/// remain owned by the SDK and are never removed here.
pub(crate) fn forget_job(job_id:&str) {
    CHAINS.lock().unwrap().retain(|key,_|key.split('\0').next()!=Some(job_id));
}

fn digest(value:&Value)->String {
    format!("{:x}",Sha256::digest(value.to_string().as_bytes()))
}
fn receipt_stamp(receipt:&Receipt)->String {
    digest(&serde_json::to_value(receipt).unwrap_or(Value::Null))
}
fn contract(body:&Value)->String {
    digest(&json!({"model":body["model"],"instructions":body["instructions"],
        "tools":body["tools"],"toolChoice":body["tool_choice"],
        "parallelToolCalls":body["parallel_tool_calls"]}))
}
fn call_ids(response:&Value)->Result<Vec<String>,String> {
    let mut ids=Vec::new();
    let mut seen=HashSet::new();
    for item in response["output"].as_array().into_iter().flatten() {
        if item["type"]!="function_call" {continue;}
        let id=item["call_id"].as_str().filter(|s|!s.is_empty())
            .ok_or("Codex completed function call has no call_id")?;
        if !seen.insert(id.to_string()) {
            return Err("Codex completed response repeated function call ID".into());
        }
        ids.push(id.to_string());
    }
    Ok(ids)
}

/// Continue only when the entire immutable receipt prefix is unchanged and
/// every tool call in the last *completed* response has a matching host result.
/// Otherwise send the host's canonical full input on a fresh response chain.
fn plan(chain:&JobChain,body:&Value,receipts:&[Receipt])->Plan {
    let Some(_)=chain.socket.as_ref() else {return Plan::Full};
    if chain.response_id.is_none() || chain.stable_contract.as_deref()!=Some(contract(body).as_str())
        || receipts.len()<chain.acknowledged_receipts.len() {
        return Plan::Full;
    }
    if receipts.iter().take(chain.acknowledged_receipts.len())
        .map(receipt_stamp).ne(chain.acknowledged_receipts.iter().cloned()) {
        return Plan::Full;
    }
    let new=&receipts[chain.acknowledged_receipts.len()..];
    if new.len()!=chain.outstanding_calls.len() {return Plan::Full;}
    let mut by_id=HashMap::new();
    for receipt in new {
        if by_id.insert(receipt.id.as_str(),receipt).is_some() {return Plan::Full;}
    }
    let mut inputs=Vec::new();
    for id in &chain.outstanding_calls {
        let Some(receipt)=by_id.get(id.as_str()) else {return Plan::Full};
        inputs.push(json!({"type":"function_call_output","call_id":id,
            "output":tool_text(receipt)}));
    }
    let Some(current_prompt)=body["input"].as_array().and_then(|input|input.last()) else {
        return Plan::Full;
    };
    if current_prompt["role"]!="user" {return Plan::Full;}
    inputs.push(current_prompt.clone());
    Plan::Delta{input:inputs,received:new.len()}
}

/// Rebase a growing server-side response chain before it exceeds the
/// owner's configured provider context capacity. The canonical full input
/// still follows the SDK's existing source/receipt projection and admission.
fn exceeds_context_budget(
    chain:&JobChain,route:&gateway::Route,planned:&Plan,body:&Value
)->bool {
    let (Some(capacity),Some(prior),Plan::Delta{input,..})=
        (route.context_tokens,chain.context_tokens,planned) else {return false};
    let reserved=route.max_output_tokens.unwrap_or(capacity/8)
        .min(capacity/4).max(128).min(capacity);
    let next=crate::engine::chat::estimate_text_tokens(&serde_json::to_string(input).unwrap_or_default())
        .saturating_add(crate::engine::chat::estimate_text_tokens(
            &body["instructions"].to_string()));
    prior.saturating_add(next).saturating_add(reserved)
        >=capacity.saturating_mul(9)/10
}

fn websocket_url(base:&str)->String {
    let url=format!("{}/responses",base.trim_end_matches('/'));
    if let Some(rest)=url.strip_prefix("https://"){format!("wss://{rest}")}
    else if let Some(rest)=url.strip_prefix("http://"){format!("ws://{rest}")}
    else {url}
}
async fn connect(route:&gateway::Route,account:&str,access:&str,transport_session:&str)
    ->Result<Socket,String> {
    let url=websocket_url(&route.base_url);
    let mut request=url.as_str().into_client_request().map_err(|e|e.to_string())?;
    let headers=request.headers_mut();
    for (name,value) in [
        ("Authorization",format!("Bearer {access}")),
        ("chatgpt-account-id",account.to_string()),
        ("OpenAI-Beta","responses_websockets=2026-02-06".into()),
        ("originator","codex_cli_rs".into()),
        ("User-Agent","codex_cli_rs/0.0.0".into()),
        ("session_id",transport_session.to_string()),
    ] {
        let header=HeaderValue::from_str(&value).map_err(|e|e.to_string())?;
        headers.insert(name,header);
    }
    tokio::time::timeout(Duration::from_secs(20),
        tokio_tungstenite::connect_async(request)).await
        .map_err(|_|"Codex native WebSocket connect timeout".to_string())?
        .map(|(socket,_)|socket).map_err(|e|format!("Codex native WebSocket connect: {e}"))
}

enum Received {Completed(Value),ChainRejected(String)}
async fn receive(state:&AppState,session:&str,job_id:&str,chain:&mut JobChain)
    ->Result<Received,WsError> {
    let socket=chain.socket.as_mut().ok_or_else(||WsError::Unavailable("no socket".into()))?;
    let mut assembler=Assembler::new(Wire::Codex);
    let mut progress=StreamProgress::new(Some(state),session,Some(job_id),"codex_ws");
    progress.record("receiving").map_err(WsError::Uncertain)?;
    loop {
        let message=tokio::time::timeout(IDLE_TIMEOUT,socket.next());
        tokio::pin!(message);
        let message=loop {
            tokio::select! {
                result=&mut message=>break result.map_err(|_|
                    WsError::Uncertain("Codex native WebSocket idle timeout after send".into()))?,
                _=tokio::time::sleep(Duration::from_millis(100))=>{
                    progress.check_live().map_err(WsError::Uncertain)?;
                }
            }
        };
        let Some(message)=message else {
            let _=progress.record("interrupted");
            return Err(WsError::Uncertain("Codex socket closed without settled response".into()));
        };
        let message=message.map_err(|e|WsError::Uncertain(format!("Codex socket receive: {e}")))?;
        let text=match message {
            Message::Text(value)=>value.to_string(),
            Message::Binary(bytes)=>String::from_utf8(bytes.to_vec())
                .map_err(|_|WsError::Uncertain("Codex socket non-UTF8 frame".into()))?,
            Message::Ping(payload)=>{
                socket.send(Message::Pong(payload)).await
                    .map_err(|e|WsError::Uncertain(format!("Codex socket pong: {e}")))?;
                continue;
            },
            Message::Pong(_)=>continue,
            Message::Close(_)=>{
                let _=progress.record("interrupted");
                return Err(WsError::Uncertain("Codex socket closed before response.completed".into()));
            },
            _=>continue,
        };
        let event:Value=serde_json::from_str(&text).map_err(|e|
            WsError::Uncertain(format!("Codex socket malformed event: {e}")))?;
        let kind=event["type"].as_str().unwrap_or("");
        if kind=="error" && (event["error"]["code"]=="previous_response_not_found"
            || event["error"]["param"]=="previous_response_id") {
            let _=progress.record("rejected_chain");
            return Ok(Received::ChainRejected(
                event["error"]["message"].as_str().unwrap_or("previous response unavailable").into()));
        }
        if kind=="response.completed" {
            let updates=assembler.feed(Packet::Data(event)).map_err(WsError::Rejected)?;
            progress.updates(updates).map_err(WsError::Uncertain)?;
            let response=assembler.finish().map_err(WsError::Rejected)?;
            if response["id"].as_str().is_none_or(str::is_empty) {
                return Err(WsError::Uncertain(
                    "Codex terminal response is missing its response ID".into()));
            }
            progress.record("settled").map_err(WsError::Uncertain)?;
            return Ok(Received::Completed(response));
        }
        let updates=assembler.feed(Packet::Data(event)).map_err(WsError::Rejected)?;
        progress.updates(updates).map_err(WsError::Uncertain)?;
    }
}

/// Update the existing pending display without erasing the last streamed text
/// or its non-executable pending tool labels when a generation aborts.
fn mark_interrupted(state:&AppState,job_id:&str) {
    let Some(job)=state.store.get("job",job_id) else {return};
    let mut progress=job["nativeStream"].clone();
    if !progress.is_object(){return;}
    progress["stage"]=json!("interrupted");
    progress["executable"]=json!(false);
    if let Some(saved)=state.store.patch_if("job",job_id,"status",&json!("running"),
        &json!({"nativeStream":progress})) {
        state.bus.record("job",&saved);
    }
}

/// The only owner of Codex socket continuity for a native /api/jobs run.
/// HTTP fallback is allowed solely for a failure to connect before sending.
pub(crate) async fn generate(
    state:&AppState,route:&gateway::Route,account:&str,access:&str,
    body:&Value,session:&str,job_id:&str,receipts:&[Receipt],
)->Result<Value,WsError> {
    if std::env::var("DOMINION_NATIVE_CODEX_WS").is_ok_and(|v|
        matches!(v.as_str(),"off"|"http")) {
        return Err(WsError::Unavailable("owner selected Codex HTTP".into()));
    }
    if !job_live(state,session,job_id) {
        return Err(WsError::Rejected("Owner cancelled the job before Codex generation".into()));
    }
    let key=key(route,account,job_id,access);
    let shared=chain_for(&key);
    let mut chain=shared.lock().await;
    if chain.opened.is_some_and(|t|t.elapsed()>MAX_AGE) {chain.reset();}
    chain.touched=Instant::now();
    let stamps=receipts.iter().map(receipt_stamp).collect::<Vec<_>>();
    for attempt in 0..2 {
        if chain.socket.is_none() {
            let socket=connect(route,account,access,session).await
                .map_err(WsError::Unavailable)?;
            chain.socket=Some(socket);
            chain.opened=Some(Instant::now());
        }
        let mut planned=plan(&chain,body,receipts);
        if exceeds_context_budget(&chain,route,&planned,body) {
            state.store.audit("native.codex_ws.context_rebase",json!({
                "jobId":job_id,"sessionId":session,
                "reportedPriorContextTokens":chain.context_tokens,
                "configuredContextTokens":route.context_tokens,
                "reason":"server_chain_context_budget",
                "oldToolResultsRetainedInJournal":true
            }));
            planned=Plan::Full;
        }
        let (input,previous,mode,new_receipts)=match planned {
            Plan::Full=>(body["input"].clone(),None,"full",0usize),
            Plan::Delta{input,received}=>(
                json!(input),chain.response_id.clone(),"delta",received),
        };
        let mut wire=body.clone();
        wire["type"]=json!("response.create");
        wire["input"]=input.clone();
        wire["stream"]=json!(true);
        wire["store"]=json!(false);
        wire["client_metadata"]=json!({"session_id":session,"thread_id":session});
        if let Some(id)=previous {
            wire["previous_response_id"]=json!(id);
        }else if let Some(map)=wire.as_object_mut() {
            map.remove("previous_response_id");
        }
        // Cancellation may arrive while opening a socket or rebuilding input.
        // Check again immediately before requesting any billable generation.
        if !job_live(state,session,job_id) {
            chain.reset();
            return Err(WsError::Rejected("Owner cancelled before the Codex request was sent".into()));
        }
        // The next request may generate billable tokens even before its first
        // delta. Durable kernel model_in_flight is already committed.
        gateway::reserve_generation(&route.base_url,&route.model,route.requests_per_minute)
            .map_err(WsError::Unavailable)?;
        let socket=chain.socket.as_mut().ok_or_else(||
            WsError::Unavailable("Codex native socket vanished before send".into()))?;
        if let Err(error)=socket.send(Message::Text(wire.to_string().into())).await {
            chain.reset();
            state.store.audit("native.codex_ws.uncertain",json!({
                "jobId":job_id,"sessionId":session,"reason":error.to_string(),
                "replayed":false,"modelBilling":"unknown"
            }));
            return Err(WsError::Uncertain(format!("Codex request send result uncertain: {error}")));
        }
        match receive(state,session,job_id,&mut chain).await {
            Ok(Received::ChainRejected(why)) if mode=="delta" && attempt==0=>{
                chain.reset();
                state.store.audit("native.codex_ws.chain_reset",json!({
                    "jobId":job_id,"sessionId":session,"reason":why,
                    "replay":"full_context_after_explicit_rejection"
                }));
                continue;
            },
            Ok(Received::ChainRejected(why))=>{
                chain.reset();
                mark_interrupted(state,job_id);
                return Err(WsError::Rejected(format!("Codex chain rejected: {why}")));
            },
            Ok(Received::Completed(response))=>{
                let id=response["id"].as_str().filter(|id|!id.is_empty())
                    .ok_or_else(||WsError::Uncertain("Settled Codex response has no ID".into()))?;
                let outstanding=call_ids(&response).map_err(WsError::Rejected)?;
                chain.response_id=Some(id.to_string());
                chain.stable_contract=Some(contract(body));
                chain.acknowledged_receipts=stamps.clone();
                chain.outstanding_calls=outstanding;
                // Responses usage input_tokens includes prior response context.
                // It is the observed context signal, NOT a network-savings claim.
                chain.context_tokens=response["usage"]["input_tokens"].as_u64()
                    .map(|input|input.saturating_add(
                        response["usage"]["output_tokens"].as_u64().unwrap_or(0)));
                chain.touched=Instant::now();
                state.store.audit("native.codex_ws.turn",json!({
                    "jobId":job_id,"sessionId":session,"mode":mode,
                    "newToolReceipts":new_receipts,
                    "wireInputTokensEstimated":crate::engine::chat::estimate_text_tokens(&input.to_string()),
                    "canonicalInputTokensEstimated":crate::engine::chat::estimate_text_tokens(&body["input"].to_string()),
                    "providerInputTokens":response["usage"]["input_tokens"],
                    "providerOutputTokens":response["usage"]["output_tokens"],
                    "measuredSavings":false
                }));
                return Ok(response);
            },
            Err(error)=>{
                chain.reset();
                mark_interrupted(state,job_id);
                let uncertainty=matches!(&error,WsError::Uncertain(_));
                if uncertainty {
                    state.store.audit("native.codex_ws.uncertain",json!({
                        "jobId":job_id,"sessionId":session,
                        "reason":format!("{error:?}"),
                        "modelBilling":"unknown","replayed":false
                    }));
                }
                return Err(error);
            }
        }
    }
    Err(WsError::Rejected("Codex chain retry exhausted".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    use std::path::PathBuf;

    fn route(base_url:String)->gateway::Route {
        gateway::Route{
            id:"ws-native-fixture".into(),name:"fixture".into(),base_url,
            api_key:"not-used".into(),model:"test-model".into(),kind:"codex".into(),
            tool_mode:"native".into(),reasoning_protocol:"openai".into(),local:true,
            disable_thinking:false,context_tokens:None,max_output_tokens:None,
            requests_per_minute:None,account_id:Some("account".into()),
        }
    }
    fn state()->(AppState,PathBuf,String) {
        let job_id=format!("native-ws-job-{}",uuid::Uuid::new_v4());
        let root=std::env::temp_dir().join(format!("dominion-native-ws-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone()};
        let store=Store::open(&cfg.db_path()).unwrap();
        let vault=Vault::open(&cfg.key_path());
        let app=AppState::new(cfg,store,vault);
        app.store.put("job",&json!({"id":job_id,"kind":"prompt",
            "status":"running","cancelRequested":false}));
        (app,root,job_id)
    }
    fn body(text:&str,receipts:&[Receipt])->Value {
        json!({"model":"test-model","instructions":"owner directives",
            "tool_choice":"auto","tools":[{"type":"function","name":"workspace_read"}],
            "input":crate::engine::native_model::codex_input(text,receipts),
            "stream":true,"store":false})
    }
    fn receipt(id:&str)->Receipt {
        Receipt{id:id.into(),step:1,tool:"workspace_read".into(),
            arguments:json!({"path":"file.txt"}),success:true,
            result:json!({"content":"real observed text"})}
    }
    async fn listener()->(tokio::net::TcpListener,String) {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url=format!("http://{}",listener.local_addr().unwrap());
        (listener,base_url)
    }
    async fn accept_socket(listener:&tokio::net::TcpListener)->SocketFixture {
        let (tcp,_)=listener.accept().await.unwrap();
        tokio_tungstenite::accept_async(tcp).await.unwrap()
    }
    type SocketFixture=tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;
    fn completed(id:&str,call_id:Option<&str>)->Value {
        let output=call_id.map(|call_id|vec![json!({"type":"function_call",
            "call_id":call_id,"name":"workspace_read",
            "arguments":"{\"path\":\"file.txt\"}"})]).unwrap_or_else(||
            vec![json!({"type":"message","role":"assistant",
                "content":[{"type":"output_text","text":"done"}]})]);
        json!({"type":"response.completed","response":{
            "id":id,"status":"completed","output":output,
            "usage":{"input_tokens":150,"output_tokens":10}
        }})
    }

    #[test]
    fn continuation_needs_exact_receipt_prefix_and_completed_call_pairing() {
        let mut chain=JobChain::new();
        // plan is full without a live socket even if a response ID is present.
        chain.response_id=Some("resp-1".into());
        chain.stable_contract=Some(contract(&body("now",&[])));
        assert_eq!(plan(&chain,&body("now",&[]),&[]),Plan::Full);
        assert_eq!(websocket_url("https://chatgpt.com/backend-api/codex"),
            "wss://chatgpt.com/backend-api/codex/responses");
    }

    #[test]
    fn provider_context_usage_rebases_an_overgrown_socket_chain() {
        let mut chain=JobChain::new();
        chain.context_tokens=Some(13_000);
        let mut provider=route("http://127.0.0.1:1".into());
        provider.context_tokens=Some(16_000);
        provider.max_output_tokens=Some(2048);
        let plan=Plan::Delta{input:vec![json!({
            "role":"user","content":[{"type":"input_text","text":"continue"}]
        })],received:0};
        assert!(exceeds_context_budget(&chain,&provider,&plan,
            &json!({"instructions":"owner instructions"})));
        chain.context_tokens=Some(200);
        assert!(!exceeds_context_budget(&chain,&provider,&plan,
            &json!({"instructions":"owner instructions"})));
        provider.context_tokens=None;
        assert!(!exceeds_context_budget(&chain,&provider,&plan,
            &json!({"instructions":"owner instructions"})),
            "unknown configured capacity must not fabricate a token limit");
    }

    #[tokio::test]
    async fn native_tool_roundtrip_reuses_one_socket_and_only_new_receipt() {
        let (listener,base)=listener().await;
        let route=route(base);
        let (state,root,job_id)=state();
        let server=tokio::spawn(async move {
            let mut socket=accept_socket(&listener).await;
            let first=match socket.next().await.unwrap().unwrap(){
                Message::Text(t)=>serde_json::from_str::<Value>(&t).unwrap(),_=>panic!()};
            assert!(first.get("previous_response_id").is_none());
            assert_eq!(first["input"].as_array().unwrap().len(),2);
            socket.send(Message::Text(completed("resp-1",Some("call-1")).to_string().into())).await.unwrap();
            let second=match socket.next().await.unwrap().unwrap(){
                Message::Text(t)=>serde_json::from_str::<Value>(&t).unwrap(),_=>panic!()};
            assert_eq!(second["previous_response_id"],"resp-1");
            assert_eq!(second["input"].as_array().unwrap().len(),2);
            assert_eq!(second["input"][0]["type"],"function_call_output");
            assert_eq!(second["input"][0]["call_id"],"call-1");
            assert_eq!(second["input"][1]["role"],"user");
            socket.send(Message::Text(completed("resp-2",None).to_string().into())).await.unwrap();
        });
        let first=generate(&state,&route,"account","token",&body("step1",&[]),
            "session",&job_id,&[]).await.unwrap();
        assert_eq!(first["id"],"resp-1");
        let receipts=[receipt("call-1")];
        let second=generate(&state,&route,"account","token",&body("step2",&receipts),
            "session",&job_id,&receipts).await.unwrap();
        assert_eq!(second["id"],"resp-2");
        server.await.unwrap();
        let audits=state.store.recent_audit(10);
        assert!(audits.iter().any(|a|a["event"]=="native.codex_ws.turn"
            && a["detail"]["mode"]=="delta"
            && a["detail"]["newToolReceipts"]==1));
        forget_job(&job_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn eof_before_first_delta_has_unknown_billing_and_no_retry() {
        let (listener,base)=listener().await;
        let route=route(base);
        let (state,root,job_id)=state();
        let (tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
        let server=tokio::spawn(async move{
            let mut socket=accept_socket(&listener).await;
            let request=socket.next().await.unwrap().unwrap();
            tx.send(request).unwrap();
            socket.close(None).await.unwrap();
        });
        let result=generate(&state,&route,"account","token",&body("hello",&[]),
            "session",&job_id,&[]).await;
        assert!(matches!(result,Err(WsError::Uncertain(_))));
        rx.recv().await.unwrap();
        assert!(rx.try_recv().is_err(),"unknown delivery must not replay the model");
        assert!(state.store.recent_audit(10).iter().any(|a|
            a["event"]=="native.codex_ws.uncertain"
            && a["detail"]["replayed"]==false));
        server.await.unwrap();
        forget_job(&job_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn settled_response_without_identity_is_uncertain_and_never_retried() {
        let (listener,base)=listener().await;
        let route=route(base);
        let (state,root,job_id)=state();
        let (tx,mut requests)=tokio::sync::mpsc::unbounded_channel();
        let server=tokio::spawn(async move {
            let mut socket=accept_socket(&listener).await;
            tx.send(socket.next().await.unwrap().unwrap()).unwrap();
            socket.send(Message::Text(json!({"type":"response.completed",
                "response":{"status":"completed","output":[]}})
                .to_string().into())).await.unwrap();
        });
        let result=generate(&state,&route,"account","token",&body("first",&[]),
            "session",&job_id,&[]).await;
        assert!(matches!(result,Err(WsError::Uncertain(_))));
        requests.recv().await.unwrap();
        assert!(requests.try_recv().is_err(),"missing ID cannot trigger paid replay");
        assert!(state.store.recent_audit(10).iter().any(|a|
            a["event"]=="native.codex_ws.uncertain" && a["detail"]["modelBilling"]=="unknown"));
        assert_eq!(state.store.get("job",&job_id).unwrap()["nativeStream"]["stage"],
            "interrupted");
        server.await.unwrap();
        forget_job(&job_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn already_cancelled_job_never_sends_or_connects_to_codex() {
        let (listener,base)=listener().await;
        let route=route(base);
        let (state,root,job_id)=state();
        state.cancel_session("session");
        let result=generate(&state,&route,"account","token",&body("cancelled",&[]),
            "session",&job_id,&[]).await;
        assert!(matches!(result,Err(WsError::Rejected(_))));
        assert!(tokio::time::timeout(Duration::from_millis(120),
            listener.accept()).await.is_err(),
            "cancelled work should not open a billable model socket");
        forget_job(&job_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn stalled_native_socket_obeys_owner_cancellation_without_retry() {
        let (listener,base)=listener().await;
        let route=route(base);
        let (state,root,job_id)=state();
        let (notice,mut sent)=tokio::sync::mpsc::unbounded_channel();
        let server=tokio::spawn(async move {
            let mut socket=accept_socket(&listener).await;
            notice.send(()).unwrap();
            let _=socket.next().await;
            std::future::pending::<()>().await;
        });
        let request=body("slow task",&[]);
        let run=generate(&state,&route,"account","token",&request,
            "session",&job_id,&[]);
        let cancel=async {
            sent.recv().await.unwrap();
            // Also let the socket finish its initial send before cancellation.
            tokio::time::sleep(Duration::from_millis(100)).await;
            state.cancel_session("session");
        };
        let (result,())=tokio::time::timeout(Duration::from_secs(3),async{
            tokio::join!(run,cancel)
        }).await.expect("cancellation must interrupt the silent socket promptly");
        assert!(matches!(result,Err(WsError::Uncertain(_))));
        assert!(state.store.recent_audit(10).iter().any(|a|
            a["event"]=="native.codex_ws.uncertain"));
        server.abort();
        forget_job(&job_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn explicit_missing_previous_response_recovers_by_full_replay() {
        let (listener,base)=listener().await;
        let route=route(base);
        let (state,root,job_id)=state();
        let server=tokio::spawn(async move {
            let mut socket=accept_socket(&listener).await;
            let _first=socket.next().await.unwrap().unwrap();
            socket.send(Message::Text(completed("resp-old",Some("call-1")).to_string().into())).await.unwrap();
            let delta=match socket.next().await.unwrap().unwrap(){
                Message::Text(t)=>serde_json::from_str::<Value>(&t).unwrap(),_=>panic!()};
            assert_eq!(delta["previous_response_id"],"resp-old");
            socket.send(Message::Text(json!({"type":"error","error":{
                "code":"previous_response_not_found","message":"evicted"}}).to_string().into())).await.unwrap();
            drop(socket);
            let mut new_socket=accept_socket(&listener).await;
            let full=match new_socket.next().await.unwrap().unwrap(){
                Message::Text(t)=>serde_json::from_str::<Value>(&t).unwrap(),_=>panic!()};
            assert!(full.get("previous_response_id").is_none());
            assert_eq!(full["input"].as_array().unwrap().len(),4);
            new_socket.send(Message::Text(completed("resp-new",None).to_string().into())).await.unwrap();
        });
        generate(&state,&route,"account","token",&body("first",&[]),
            "session",&job_id,&[]).await.unwrap();
        let receipts=[receipt("call-1")];
        let done=generate(&state,&route,"account","token",&body("second",&receipts),
            "session",&job_id,&receipts).await.unwrap();
        assert_eq!(done["id"],"resp-new");
        server.await.unwrap();
        forget_job(&job_id);
        std::fs::remove_dir_all(root).unwrap();
    }
}
