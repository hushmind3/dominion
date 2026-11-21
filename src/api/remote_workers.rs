//! P3 remote worker control plane. Never expose the owner product API remotely.
//! Workers have one capability: bounded TEXT inference on their own device.
//! They never receive provider secrets, writable workspaces or tool dispatch.
use crate::state::AppState;
use axum::{extract::{Path,State},http::{HeaderMap,StatusCode},
    response::{IntoResponse,Response},routing::{get,post},Json,Router};
use serde_json::{json,Value};

fn gateway_enabled()->bool{
    std::env::var("DOMINION_REMOTE_WORKERS").ok().as_deref()==Some("1")
}
fn json_error(status:StatusCode,message:&str)->Response{
    (status,Json(json!({"error":message}))).into_response()
}
fn control_ready()->Result<(),Response>{
    if !gateway_enabled(){return Err(json_error(StatusCode::SERVICE_UNAVAILABLE,
        "Remote workers are disabled (DOMINION_REMOTE_WORKERS=1)"));}
    Ok(())
}
fn valid_worker_token(token:&str)->bool{
    token.len()>=32&&token.len()<=1024&&token.is_ascii()
        &&!token.chars().any(char::is_whitespace)
}
fn worker_authorized(headers:&HeaderMap)->bool{
    let Ok(expected)=std::env::var("DOMINION_REMOTE_WORKER_TOKEN") else{return false;};
    if !valid_worker_token(&expected){return false;}
    let actual=headers.get("x-dominion-worker-token")
        .and_then(|value|value.to_str().ok()).unwrap_or("");
    // Constant-work comparison for all candidate lengths; no equality
    // shortcut based on a matched prefix.
    let mut mismatch=expected.len()^actual.len();
    for i in 0..expected.len(){
        mismatch|=usize::from(expected.as_bytes()[i]
            ^actual.as_bytes().get(i).copied().unwrap_or(0));
    }
    mismatch==0
}
fn require_worker(headers:&HeaderMap)->Result<(),Response>{
    control_ready()?;
    if worker_authorized(headers){Ok(())}
    else{Err(json_error(StatusCode::UNAUTHORIZED,"Invalid remote worker credentials"))}
}
fn worker_id(value:&Value)->Result<&str,Response>{
    let Some(id)=value["workerId"].as_str().filter(|id|{
        !id.is_empty()&&id.len()<=128&&id.bytes().all(|b|
            b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'|b'.'))
    }) else{return Err(json_error(StatusCode::BAD_REQUEST,"Invalid remote worker identity"));};
    Ok(id)
}
fn lease_id(value:&Value)->Result<&str,Response>{
    value["leaseId"].as_str().filter(|id|id.len()==36&&id.is_ascii())
        .ok_or_else(||json_error(StatusCode::BAD_REQUEST,"Invalid lease ID"))
}
fn task_id(value:&Value)->Result<&str,Response>{
    value["taskId"].as_str().filter(|id|!id.is_empty()&&id.len()<=128)
        .ok_or_else(||json_error(StatusCode::BAD_REQUEST,"Invalid remote task ID"))
}

/// The only listener exposed to an explicit SSH/TLS private tunnel.
/// No generic /api/chat, tool or asset access on this router.
pub fn gateway_router(state:AppState)->Router{
    Router::new()
        .route("/v1/worker/health",get(worker_health))
        .route("/v1/worker/hello",post(worker_hello))
        .route("/v1/worker/claim",post(worker_claim))
        .route("/v1/worker/heartbeat",post(worker_heartbeat))
        .route("/v1/worker/complete",post(worker_complete))
        .layer(axum::extract::DefaultBodyLimit::max(32*1024))
        .with_state(state)
}
async fn worker_health(headers:HeaderMap)->Response{
    if let Err(e)=require_worker(&headers){return e;}
    Json(json!({"protocol":"dominion-remote-v1","capabilities":["text_inference"],
        "sandbox":"no_tools","heartbeat":true})).into_response()
}
/// Durable worker registry. A machine advertises only the model ID and
/// permitted capability set, never its API key or private local endpoint.
/// Claim and heartbeat refresh the lease to survive coordinator restarts.
fn register_worker(state:&AppState,worker:&str,model:Option<&str>)
    ->Result<Value,String>{
    let current=state.store.get("remote_worker",worker);
    let model=model.or_else(||current.as_ref()
        .and_then(|row|row["model"].as_str()));
    let model=model.filter(|m|!m.is_empty()&&m.len()<=160)
        .unwrap_or("unspecified");
    let now=chrono::Utc::now();
    let row=json!({"id":worker,"kind":"inference_worker",
        "model":model,"capabilities":["text_inference"],
        "lastSeenAt":now.to_rfc3339(),
        "lastSeenAtMs":now.timestamp_millis()});
    state.store.put("remote_worker",&row).ok_or(
        "Could not durably record remote worker presence".into())
}
async fn worker_hello(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(e)=require_worker(&headers){return e;}
    let worker=match worker_id(&body){Ok(id)=>id,Err(e)=>return e};
    match register_worker(&state,worker,body["model"].as_str()){
        Ok(row)=>Json(json!({"workerId":worker,
            "capabilities":row["capabilities"],
            "protocol":"dominion-remote-v1","registered":true})).into_response(),
        Err(_)=>json_error(StatusCode::INTERNAL_SERVER_ERROR,
            "Remote worker registration unavailable")
    }
}
async fn worker_claim(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(e)=require_worker(&headers){return e;}
    let worker=match worker_id(&body){Ok(id)=>id,Err(e)=>return e};
    if register_worker(&state,worker,body["model"].as_str()).is_err(){
        return json_error(StatusCode::INTERNAL_SERVER_ERROR,
            "Remote worker registry unavailable");
    }
    match state.store.claim_remote_task(worker) {
        Ok(task)=>{
            if let Some(value)=&task {state.store.audit("remote.claim",
                json!({"taskId":value["taskId"],"workerId":worker}));}
            Json(json!({"task":task,"protocol":"dominion-remote-v1"})).into_response()
        }
        Err(_)=>json_error(StatusCode::INTERNAL_SERVER_ERROR,"Remote claim unavailable")
    }
}
async fn worker_heartbeat(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(e)=require_worker(&headers){return e;}
    let worker=match worker_id(&body){Ok(id)=>id,Err(e)=>return e};
    let lease=match lease_id(&body){Ok(id)=>id,Err(e)=>return e};
    let task=match task_id(&body){Ok(id)=>id,Err(e)=>return e};
    if register_worker(&state,worker,None).is_err(){
        return json_error(StatusCode::INTERNAL_SERVER_ERROR,
            "Remote worker heartbeat registry unavailable");
    }
    match state.store.heartbeat_remote_task(task,worker,lease){
        Ok(true)=>Json(json!({"ok":true})).into_response(),
        _=>json_error(StatusCode::CONFLICT,"Stale, cancelled or expired worker lease")
    }
}
async fn worker_complete(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(e)=require_worker(&headers){return e;}
    let worker=match worker_id(&body){Ok(id)=>id,Err(e)=>return e};
    let lease=match lease_id(&body){Ok(id)=>id,Err(e)=>return e};
    let task=match task_id(&body){Ok(id)=>id,Err(e)=>return e};
    let Some(status)=body["status"].as_str()
        .filter(|status|matches!(*status,"completed"|"needs_review"))
    else{return json_error(StatusCode::BAD_REQUEST,"Invalid result status");};
    let answer=body["answer"].as_str().unwrap_or("");
    if answer.chars().count()>16_384||body["usage"].to_string().len()>2048 {
        return json_error(StatusCode::PAYLOAD_TOO_LARGE,"Remote result too large");
    }
    if status=="completed"&&answer.trim().is_empty(){
        return json_error(StatusCode::BAD_REQUEST,"Empty remote completion");
    }
    match state.store.finish_remote_task(task,worker,lease,status,answer,&body["usage"]){
        Ok(Some(saved))=>{
            state.bus.record("remote_task",&saved);
            Json(json!({"ok":true,"taskId":task,"status":saved["status"],
                "evidenceGrade":"remote_unverified"})).into_response()
        }
        _=>json_error(StatusCode::CONFLICT,"Remote result rejected: lease or owner state changed")
    }
}

/// Owner-only product-facing remote task API; protected by normal local
/// origin/admin-token rules, and disabled unless explicitly opted in.
pub async fn owner_create(
    State(state):State<AppState>,Json(body):Json<Value>
)->Response{
    if let Err(e)=control_ready(){return e;}
    let Some(prompt)=body["prompt"].as_str().filter(|p|
        !p.trim().is_empty()&&p.len()<=16_384)
    else{return json_error(StatusCode::BAD_REQUEST,"Remote prompt must be 1-16384 bytes");};
    let id=format!("remote:{}",uuid::Uuid::new_v4());
    let parent=body["parentRunId"].as_str().filter(|p|p.len()<=128).unwrap_or("");
    let record=json!({"id":id,"kind":"remote_inference",
        "status":"queued","phase":"queued",
        "prompt":prompt,"parentRunId":parent,"capability":"text_inference",
        "queuedAt":chrono::Utc::now().to_rfc3339(),
        "attempts":0,"evidenceGrade":"remote_unverified",
        "maxTokens":body["maxTokens"].as_u64().unwrap_or(2048).clamp(64,4096)});
    match state.store.put("remote_task",&record){
        Some(saved)=>{
            state.bus.record("remote_task",&saved);
            (StatusCode::ACCEPTED,Json(json!({
                "id":id,"status":"queued","runId":id,
                "sourceJournal":"remote_task","capability":"text_inference"
            }))).into_response()
        }
        None=>json_error(StatusCode::INTERNAL_SERVER_ERROR,"Remote queue unavailable")
    }
}
/// The owner can see which machines can actually receive work without
/// adding another UI navigation item or exposing their secrets.
pub async fn owner_workers(State(state):State<AppState>)->Response{
    if let Err(e)=control_ready(){return e;}
    let now=chrono::Utc::now().timestamp_millis();
    let mut workers=state.store.all("remote_worker");
    workers.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str()));
    let result=workers.into_iter().map(|worker|{
        let last=worker["lastSeenAtMs"].as_i64().unwrap_or(0);
        let online=last>0&&now.saturating_sub(last)<30_000;
        json!({"workerId":worker["id"],"model":worker["model"],
            "capabilities":worker["capabilities"],"lastSeenAt":worker["lastSeenAt"],
            "online":online})
    }).collect::<Vec<_>>();
    Json(json!(result)).into_response()
}
pub async fn owner_list(State(state):State<AppState>)->Response{
    if let Err(e)=control_ready(){return e;}
    // Keep the existing oldest-to-newest REST order (the React panel uses
    // slice(-30).reverse()), but never load unbounded history every 5 seconds.
    let mut items=match state.store.recent_remote_tasks(100){
        Ok(items)=>items,
        Err(_)=>return json_error(StatusCode::INTERNAL_SERVER_ERROR,
            "Remote task history unavailable"),
    };
    items.reverse();
    Json(json!(items.into_iter().map(|item|json!({
        "id":item["id"],"status":item["status"],"phase":item["phase"],
        "workerId":item["workerId"],"queuedAt":item["queuedAt"],
        "startedAt":item["startedAt"],"finishedAt":item["finishedAt"],
        "result":item["result"],"evidenceGrade":item["evidenceGrade"]
    })).collect::<Vec<_>>())).into_response()
}
pub async fn owner_one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if let Err(e)=control_ready(){return e;}
    match state.store.get("remote_task",&id){
        Some(task)=>Json(json!({
            "id":task["id"],"status":task["status"],"phase":task["phase"],
            "workerId":task["workerId"],"result":task["result"],
            "queuedAt":task["queuedAt"],"startedAt":task["startedAt"],
            "finishedAt":task["finishedAt"],
            "evidenceGrade":task["evidenceGrade"]
        })).into_response(),
        None=>json_error(StatusCode::NOT_FOUND,"Remote task not found")
    }
}
pub async fn owner_cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if let Err(e)=control_ready(){return e;}
    match state.store.cancel_remote_task(&id){
        Ok(Some(task))=>{
            state.bus.record("remote_task",&task);
            Json(json!({"id":id,"status":task["status"]})).into_response()
        }
        _=>json_error(StatusCode::CONFLICT,"Remote task already settled or missing")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn remote_http_roundtrip_requires_token_and_fences_owner_cancel(){
        let dir=std::env::temp_dir().join(format!(
            "dominion-remote-http-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let token=format!("test-{}-{}",uuid::Uuid::new_v4(),uuid::Uuid::new_v4());
        let previous_enabled=std::env::var("DOMINION_REMOTE_WORKERS").ok();
        let previous_token=std::env::var("DOMINION_REMOTE_WORKER_TOKEN").ok();
        std::env::set_var("DOMINION_REMOTE_WORKERS","1");
        std::env::set_var("DOMINION_REMOTE_WORKER_TOKEN",&token);
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let address=listener.local_addr().unwrap();
        let gateway=gateway_router(state.clone());
        let server=tokio::spawn(async move { axum::serve(listener,gateway).await });
        let client=reqwest::Client::new();
        let base=format!("http://{address}");
        let rejected=client.post(format!("{base}/v1/worker/claim"))
            .json(&json!({"workerId":"w"})).send().await.unwrap();
        assert_eq!(rejected.status(),StatusCode::UNAUTHORIZED);
        let unauthorized_registration=client.post(format!("{base}/v1/worker/hello"))
            .json(&json!({"workerId":"w","model":"local-27b"})).send().await.unwrap();
        assert_eq!(unauthorized_registration.status(),StatusCode::UNAUTHORIZED);
        let hello=client.post(format!("{base}/v1/worker/hello"))
            .header("x-dominion-worker-token",&token)
            .json(&json!({"workerId":"w","model":"local-27b"})).send().await.unwrap();
        assert_eq!(hello.status(),StatusCode::OK);
        let visible=owner_workers(State(state.clone())).await;
        let bytes=axum::body::to_bytes(visible.into_body(),8192).await.unwrap();
        let machines:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(machines[0]["workerId"],"w");
        assert_eq!(machines[0]["model"],"local-27b");
        assert_eq!(machines[0]["online"],true);
        assert_eq!(machines[0]["capabilities"],json!(["text_inference"]));
        let owner=owner_create(State(state.clone()),
            Json(json!({"prompt":"Remote text reasoning"}))).await;
        assert_eq!(owner.status(),StatusCode::ACCEPTED);
        let body=axum::body::to_bytes(owner.into_body(),8192).await.unwrap();
        let ticket:Value=serde_json::from_slice(&body).unwrap();
        let id=ticket["id"].as_str().unwrap();
        let claim=client.post(format!("{base}/v1/worker/claim"))
            .header("x-dominion-worker-token",&token)
            .json(&json!({"workerId":"w"})).send().await.unwrap();
        assert!(claim.status().is_success());
        let claim:Value=claim.json().await.unwrap();
        assert_eq!(claim["task"]["taskId"],id);
        let lease=claim["task"]["leaseId"].as_str().unwrap();
        let fake=client.post(format!("{base}/v1/worker/complete"))
            .header("x-dominion-worker-token",&token)
            .json(&json!({"workerId":"forged","taskId":id,
                "leaseId":lease,"status":"completed","answer":"fake"}))
            .send().await.unwrap();
        assert_eq!(fake.status(),StatusCode::CONFLICT);
        let cancelled=owner_cancel(State(state.clone()),Path(id.into())).await;
        assert_eq!(cancelled.status(),StatusCode::OK);
        let late=client.post(format!("{base}/v1/worker/complete"))
            .header("x-dominion-worker-token",&token)
            .json(&json!({"workerId":"w","taskId":id,
                "leaseId":lease,"status":"completed","answer":"late"}))
            .send().await.unwrap();
        assert_eq!(late.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("remote_task",id).unwrap()["status"],"cancelled");
        state.store.patch_if("remote_worker","w","id",&json!("w"),
            &json!({"lastSeenAtMs":0})).unwrap();
        let visible=owner_workers(State(state.clone())).await;
        let bytes=axum::body::to_bytes(visible.into_body(),8192).await.unwrap();
        let machines:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(machines[0]["online"],false);
        server.abort();
        match previous_enabled{Some(v)=>std::env::set_var("DOMINION_REMOTE_WORKERS",v),
            None=>std::env::remove_var("DOMINION_REMOTE_WORKERS")}
        match previous_token{Some(v)=>std::env::set_var("DOMINION_REMOTE_WORKER_TOKEN",v),
            None=>std::env::remove_var("DOMINION_REMOTE_WORKER_TOKEN")}
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn worker_identity_rejects_path_and_metacharacters(){
        assert!(worker_id(&json!({"workerId":"workstation-1"})).is_ok());
        assert!(worker_id(&json!({"workerId":"a/../b"})).is_err());
        assert!(worker_id(&json!({"workerId":"a b"})).is_err());
    }
    #[test]
    fn credentials_must_be_long_and_not_whitespace_separated(){
        assert!(!valid_worker_token("test"));
        assert!(!valid_worker_token(&"a".repeat(31)));
        assert!(valid_worker_token(&"a".repeat(64)));
        assert!(!valid_worker_token(&format!("{} ","a".repeat(64))));
    }
}
