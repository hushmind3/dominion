//! P3 optional distributed control plane over the existing native Rust engine.
 //! Every remote device pulls a job and executes it through its *own*
 //! dominion-rust Job/Harness, not through an independent agent loop.
 //! Runs on a separate, opt-in TCP listener; never exposes the desktop UI.
use crate::state::AppState;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap,StatusCode},
    response::{IntoResponse,Response},
    routing::post,
    Json,
};
use serde_json::{json,Value};

#[path = "remote_worker.rs"]
mod worker;

const LEASE_MS:i64=45_000;
const NODE_TTL_MS:i64=90_000;
const MAX_AGENTS:usize=32;
const MAX_PROMPT:usize=8_000;

fn now_ms()->i64{chrono::Utc::now().timestamp_millis()}
fn valid_id(id:&str)->bool{
    !id.is_empty() && id.len()<=80 &&
        id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
fn configured_token()->Option<String>{
    std::env::var("DOMINION_REMOTE_CLUSTER_TOKEN").ok()
        .filter(|value|value.len()>=32 && value.len()<=512)
}
fn constant_time_equal(a:&[u8],b:&[u8])->bool{
    if a.len()!=b.len(){return false;}
    let mut diff=0u8;
    for (x,y) in a.iter().zip(b){diff|=x^y;}
    diff==0
}
fn authorized(headers:&HeaderMap)->bool{
    let Some(secret)=configured_token()else{return false;};
    let given=headers.get("x-dominion-remote-token")
        .and_then(|h|h.to_str().ok()).unwrap_or("");
    constant_time_equal(secret.as_bytes(),given.as_bytes())
}
fn rejected(code:StatusCode,reason:&str)->Response{
    (code,Json(json!({"error":reason}))).into_response()
}
fn reply(value:Result<Value,String>)->Response{
    match value{
        Ok(value)=>Json(value).into_response(),
        Err(err)=>(StatusCode::CONFLICT,Json(json!({"error":err}))).into_response(),
    }
}
fn authenticated(headers:&HeaderMap)->Result<(),Response>{
    if authorized(headers){Ok(())}else{Err(rejected(StatusCode::UNAUTHORIZED,
        "Remote cluster credentials missing, invalid or not configured"))}
}
pub fn routes()->Router<AppState>{
    Router::<AppState>::new()
        .route("/v1/remote/register",post(register_http))
        .route("/v1/remote/heartbeat",post(heartbeat_http))
        .route("/v1/remote/claim",post(claim_http))
        .route("/v1/remote/renew",post(renew_http))
        .route("/v1/remote/settle",post(settle_http))
}
pub async fn start_control_plane(state:AppState)->Result<(),String>{
    let Some(bind)=std::env::var("DOMINION_REMOTE_LISTEN").ok()
        .filter(|s|!s.trim().is_empty()) else{return Ok(());};
    if configured_token().is_none(){
        return Err("Remote listener requires DOMINION_REMOTE_CLUSTER_TOKEN (32+ random bytes)".into());
    }
    let addr=bind.parse::<std::net::SocketAddr>()
        .map_err(|e|format!("DOMINION_REMOTE_LISTEN must be host:port: {e}"))?;
    let listener=tokio::net::TcpListener::bind(addr).await
        .map_err(|e|format!("remote control listener failed: {e}"))?;
    println!("DOMINION remote worker control plane listening at {}",listener.local_addr()
        .map_err(|e|e.to_string())?);
    tokio::spawn(async move{
        let _=axum::serve(listener,routes().with_state(state)).await;
    });
    Ok(())
}
pub async fn run_worker()->Result<(),String>{worker::run().await}

async fn register_http(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(err)=authenticated(&headers){return err;}
    reply(register(&state,&body))
}
async fn heartbeat_http(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(err)=authenticated(&headers){return err;}
    reply(heartbeat(&state,&body))
}
async fn claim_http(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(err)=authenticated(&headers){return err;}
    reply(claim(&state,&body,now_ms()))
}
async fn renew_http(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(err)=authenticated(&headers){return err;}
    reply(renew(&state,&body,now_ms()))
}
async fn settle_http(
    State(state):State<AppState>,headers:HeaderMap,Json(body):Json<Value>
)->Response{
    if let Err(err)=authenticated(&headers){return err;}
    reply(settle(&state,&body,now_ms()))
}
fn identity(body:&Value)->Result<(&str,&str),String>{
    let id=body["workerId"].as_str().filter(|s|valid_id(s))
        .ok_or("workerId must be 1..80 ASCII letters/digits/dash/underscore")?;
    let epoch=body["workerEpoch"].as_str().filter(|s|uuid::Uuid::parse_str(s).is_ok())
        .ok_or("workerEpoch must be a native UUID")?;
    Ok((id,epoch))
}
fn owned_worker(state:&AppState,body:&Value)->Result<Value,String>{
    let (id,epoch)=identity(body)?;
    let node=state.store.get("remote_worker",id).ok_or("Remote worker not registered")?;
    if node["workerEpoch"]!=epoch{return Err("Stale remote worker epoch".into());}
    if node["status"]!="online" || now_ms()-node["lastHeartbeatMs"].as_i64().unwrap_or(0)>NODE_TTL_MS{
        return Err("Remote worker is offline; register/heartbeat before claiming".into());
    }
    Ok(node)
}
fn register(state:&AppState,body:&Value)->Result<Value,String>{
    let (id,epoch)=identity(body)?;
    let agents=body["agents"].as_array().ok_or("agents must be an array")?;
    if agents.is_empty() || agents.len()>MAX_AGENTS ||
        agents.iter().any(|v|v.as_str().is_none_or(|s|!valid_id(s))){
        return Err("Register 1..32 valid local agent IDs".into());
    }
    let allow_writes=body["allowWrites"].as_bool().unwrap_or(false);
    let value=json!({
        "id":id,"kind":"remote_worker","workerEpoch":epoch,"status":"online",
        "lastHeartbeatMs":now_ms(),"agents":agents,"allowWrites":allow_writes,
        "version":1,"protocol":"dominion-rust-native-job",
    });
    let saved=if let Some(saved)=state.store.put_if_absent("remote_worker",&value){
        saved
    }else{
        let previous=state.store.get("remote_worker",id).ok_or("Worker disappeared")?;
        state.store.patch_if("remote_worker",id,"workerEpoch",
            &previous["workerEpoch"],&value)
            .ok_or("Concurrent worker registration; retry")?
    };
    state.bus.record("remote_worker",&saved);
    Ok(json!({"workerId":id,"workerEpoch":epoch,"registered":true,
        "leaseMs":LEASE_MS,"heartbeatMs":15_000,"protocolVersion":1}))
}
fn heartbeat(state:&AppState,body:&Value)->Result<Value,String>{
    let (id,epoch)=identity(body)?;
    let saved=state.store.patch_if("remote_worker",id,"workerEpoch",
        &json!(epoch),&json!({"status":"online","lastHeartbeatMs":now_ms()}))
        .ok_or("Cannot heartbeat a replaced or unregistered remote worker")?;
    Ok(json!({"workerId":id,"workerEpoch":saved["workerEpoch"],"alive":true}))
}
pub fn worker_available(
    state:&AppState,worker_id:&str,agent_id:&str,read_only:bool,
)->Result<(),String>{
    if configured_token().is_none(){return Err("Remote execution not configured".into());}
    let node=state.store.get("remote_worker",worker_id).ok_or("Remote Worker not registered")?;
    if node["status"]!="online" ||
        now_ms()-node["lastHeartbeatMs"].as_i64().unwrap_or(0)>NODE_TTL_MS{
        return Err("Remote Worker offline or heartbeat expired".into());
    }
    if !node["agents"].as_array().is_some_and(|agents|
        agents.iter().any(|a|a.as_str()==Some(agent_id))){
        return Err("Remote Worker does not advertise that agent".into());
    }
    if !read_only && node["allowWrites"]!=true{
        return Err("Remote Worker did not explicitly allow file-changing jobs".into());
    }
    Ok(())
}
pub fn visible_workers(state:&AppState)->Vec<Value>{
    let now=now_ms();
    let mut records=state.store.all("remote_worker").into_iter().filter(|node|
        node["status"]=="online" &&
        now-node["lastHeartbeatMs"].as_i64().unwrap_or(0)<=NODE_TTL_MS
    ).map(|node|json!({
        "workerId":node["id"],"agents":node["agents"],
        "allowWrites":node["allowWrites"],"protocol":node["protocol"],
    })).collect::<Vec<_>>();
    records.sort_by(|a,b|a["workerId"].as_str().cmp(&b["workerId"].as_str()));
    records.truncate(64);
    records
}
/// Commits one deterministic task. A retry with the same immutable identity
/// never queues a second remote effect. Work is never replayed on expiry.
pub fn submit(
    state:&AppState,team_id:&str,node:&Value,prompt:&str,
)->Result<Value,String>{
    let worker_id=node["remoteWorkerId"].as_str().ok_or("remoteWorkerId required")?;
    let agent_id=node["agentId"].as_str().ok_or("agentId required")?;
    let node_id=node["id"].as_str().filter(|s|valid_id(s)).ok_or("Invalid node ID")?;
    let read_only=node["readOnly"]!=false;
    worker_available(state,worker_id,agent_id,read_only)?;
    if prompt.is_empty()||prompt.len()>MAX_PROMPT{
        return Err("Remote task prompt must be 1..8000 bytes".into());
    }
    if let Some(criteria)=node.get("successCriteria").filter(|v|!v.is_null()){
        crate::engine::verification::validate_criteria(criteria)?;
    }
    let id=format!("remote-task-{}",crate::engine::memory::stable_id(&[
        team_id,node_id,worker_id,agent_id,prompt,
    ]));
    let saved=state.store.put_if_absent("remote_task",&json!({
        "id":id,"kind":"remote_task","teamId":team_id,"nodeId":node_id,
        "workerId":worker_id,"agentId":agent_id,"prompt":prompt,
        "readOnly":read_only,"successCriteria":node["successCriteria"],
        "status":"queued","attempts":0,
        "createdAtMs":now_ms(),
    }));
    if let Some(ref task)=saved{state.bus.record("remote_task",task);}
    let task=saved.or_else(||state.store.get("remote_task",&id))
        .ok_or("Unable to persist remote work")?;
    if task["teamId"]!=team_id ||task["nodeId"]!=node_id ||
        task["workerId"]!=worker_id ||task["agentId"]!=agent_id ||
        task["prompt"]!=prompt ||task["readOnly"]!=read_only ||
        task["successCriteria"]!=node["successCriteria"]{
        return Err("Remote task ID was reused with different inputs".into());
    }
    Ok(task)
}
/// Turn expired running jobs into review states: a different machine may
/// still have committed file effects even if its heartbeat disappeared.
fn expire(state:&AppState,now:i64){
    for task in state.store.all("remote_task"){
        if task["status"]!="running" ||
            task["leaseUntilMs"].as_i64().unwrap_or(i64::MAX)>=now{
            continue;
        }
        let Some(id)=task["id"].as_str()else{continue;};
        let guards=[
            ("status",&task["status"]),
            ("workerEpoch",&task["workerEpoch"]),
            ("leaseId",&task["leaseId"]),
        ];
        if let Some(saved)=state.store.patch_if_all_merge("remote_task",id,&guards,
            &json!({"status":"needs_review","outcomeUnknown":true,
                "error":"remote worker lease expired; inspect effects before retry"}),&[]){
            state.bus.record("remote_task",&saved);
        }
    }
}
fn claim(state:&AppState,body:&Value,now:i64)->Result<Value,String>{
    let node=owned_worker(state,body)?;
    let (worker_id,epoch)=identity(body)?;
    expire(state,now);
    let mut candidates=state.store.all("remote_task");
    candidates.sort_by_key(|t|t["createdAtMs"].as_i64().unwrap_or(0));
    // A restarted poller reattaches to an existing, still-valid lease. The
    // native local job's deterministic ID fences a duplicate subprocess.
    for task in &candidates{
        if task["workerId"]==worker_id &&task["workerEpoch"]==epoch &&
            task["status"]=="running" &&
            task["leaseUntilMs"].as_i64().unwrap_or(0)>=now{
            return Ok(json!({"task":task,"reattached":true}));
        }
    }
    for task in &candidates{
        if task["workerId"]!=worker_id ||task["status"]!="queued"{continue;}
        if task["readOnly"]==false && node["allowWrites"]!=true{continue;}
        if !node["agents"].as_array().is_some_and(|agents|
            agents.iter().any(|a|a==&task["agentId"])){continue;}
        let Some(id)=task["id"].as_str()else{continue;};
        let lease_id=uuid::Uuid::new_v4().to_string();
        if let Some(saved)=state.store.patch_if("remote_task",id,"status",
            &json!("queued"),&json!({"status":"running",
                "workerEpoch":epoch,"leaseId":lease_id,
                "leaseUntilMs":now+LEASE_MS,"lastHeartbeatMs":now,
                "startedAtMs":now,"attempts":1})){
            state.bus.record("remote_task",&saved);
            return Ok(json!({"task":saved,"reattached":false}));
        }
    }
    Ok(json!({"task":null}))
}
fn owned_lease<'a>(
    state:&AppState,body:&'a Value,now:i64,
)->Result<(String,Vec<(&'static str,Value)>),String>{
    let (worker_id,epoch)=identity(body)?;
    let id=body["taskId"].as_str().filter(|s|valid_id(s))
        .ok_or("taskId required")?;
    let lease=body["leaseId"].as_str().filter(|s|uuid::Uuid::parse_str(s).is_ok())
        .ok_or("leaseId required")?;
    let task=state.store.get("remote_task",id).ok_or("Remote task missing")?;
    if task["status"]!="running" ||task["workerId"]!=worker_id ||
        task["workerEpoch"]!=epoch ||task["leaseId"]!=lease{
        return Err("Stale lease or wrong worker identity".into());
    }
    if task["leaseUntilMs"].as_i64().unwrap_or(0)<now{
        expire(state,now);
        return Err("Remote lease expired; review uncertain effects".into());
    }
    Ok((id.to_string(),vec![
        ("status",json!("running")),
        ("workerId",json!(worker_id)),
        ("workerEpoch",json!(epoch)),
        ("leaseId",json!(lease)),
    ]))
}
fn renew(state:&AppState,body:&Value,now:i64)->Result<Value,String>{
    let (id,owned)=owned_lease(state,body,now)?;
    let refs=owned.iter().map(|(k,v)|(*k,v)).collect::<Vec<_>>();
    let saved=state.store.patch_if_all_merge("remote_task",&id,&refs,
        &json!({"leaseUntilMs":now+LEASE_MS,"lastHeartbeatMs":now}),&[])
        .ok_or("Remote worker lease changed during renewal")?;
    Ok(json!({"taskId":id,"leaseUntilMs":saved["leaseUntilMs"],"renewed":true}))
}
fn settle(state:&AppState,body:&Value,now:i64)->Result<Value,String>{
    let task_id=body["taskId"].as_str().ok_or("taskId required")?;
    if let Some(task)=state.store.get("remote_task",task_id){
        if matches!(task["status"].as_str(),Some("completed"|"needs_review")) &&
            task["workerId"]==body["workerId"] &&
            task["workerEpoch"]==body["workerEpoch"] &&
            task["leaseId"]==body["leaseId"]{
            return Ok(json!({"taskId":task_id,"status":task["status"],"replayed":true}));
        }
    }
    let (id,owned)=owned_lease(state,body,now)?;
    let status=body["jobStatus"].as_str().ok_or("jobStatus required")?;
    if !matches!(status,"completed"|"failed"|"cancelled"|"needs_review"|"interrupted"){
        return Err("Native job is not terminal".into());
    }
    let agent_job_id=body["localJobId"].as_str()
        .filter(|s|valid_id(s)).ok_or("localJobId required")?;
    let answer=body["answer"].as_str().unwrap_or("")
        .chars().take(2_000).collect::<String>();
    let verification=body["verification"].clone();
    let verified=status=="completed" && body["verified"]==true;
    let refs=owned.iter().map(|(k,v)|(*k,v)).collect::<Vec<_>>();
    let saved=state.store.patch_if_all_merge("remote_task",&id,&refs,
        &json!({
            "status":if status=="completed"{"completed"}else{"needs_review"},
            "localJobId":agent_job_id,
            "reportedJobStatus":status,
            "answer":answer,
            "verification":verification,
            "verified":verified,"evidenceOrigin":"remote_worker_reported",
            "operationCount":body["operationCount"].as_u64().unwrap_or(0).min(100),
            "completedAtMs":now,"outcomeUnknown":status!="completed",
        }),&[]).ok_or("Remote task lease changed during settlement")?;
    state.bus.record("remote_task",&saved);
    Ok(json!({"taskId":id,"status":saved["status"],"verified":verified}))
}
pub fn task_status(state:&AppState,id:&str)->Result<Value,String>{
    expire(state,now_ms());
    state.store.get("remote_task",id).ok_or("Remote task not found".into())
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("distributed-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        (AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path())),dir)
    }
    #[test]
    fn remote_job_claim_is_atomic_and_old_epoch_cannot_settle(){
        let (state,dir)=state();
        let old=uuid::Uuid::new_v4().to_string();
        register(&state,&json!({"workerId":"n1","workerEpoch":old,
            "agents":["reader"],"allowWrites":false})).unwrap();
        assert!(constant_time_equal(b"abcd",b"abcd"));
        assert!(!constant_time_equal(b"abcd",b"abce"));
        // In production, submit verifies the operator's configured secret
        // before it admits a remote assignment. This fixture uses a direct
        // repository task to isolate the lease-fencing state machine.
        state.store.put("remote_task",&json!({
            "id":"t1","workerId":"n1","agentId":"reader","status":"queued",
            "prompt":"Read the file","readOnly":true,"createdAtMs":1,
        })).unwrap();
        let first=claim(&state,&json!({"workerId":"n1","workerEpoch":old}),now_ms()).unwrap();
        let lease=first["task"]["leaseId"].as_str().unwrap().to_string();
        let again=claim(&state,&json!({"workerId":"n1","workerEpoch":old}),now_ms()).unwrap();
        assert_eq!(again["reattached"],true);
        assert_eq!(again["task"]["leaseId"],lease);
        let late=uuid::Uuid::new_v4().to_string();
        register(&state,&json!({"workerId":"n1","workerEpoch":late,
            "agents":["reader"],"allowWrites":false})).unwrap();
        assert!(settle(&state,&json!({"workerId":"n1","workerEpoch":late,
            "taskId":"t1","leaseId":lease,"localJobId":"job-1",
            "jobStatus":"completed"}),now_ms()).is_err());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn writer_restriction_lease_expiry_and_stale_success_are_fenced(){
        let (state,dir)=state();
        let epoch=uuid::Uuid::new_v4().to_string();
        register(&state,&json!({"workerId":"n2","workerEpoch":epoch,
            "agents":["coder"],"allowWrites":false})).unwrap();
        state.store.put("remote_task",&json!({
            "id":"write-1","workerId":"n2","agentId":"coder","status":"queued",
            "prompt":"Write file","readOnly":false,"createdAtMs":1,
        })).unwrap();
        assert!(claim(&state,&json!({"workerId":"n2","workerEpoch":epoch}),
            now_ms()).unwrap()["task"].is_null());
        let now=now_ms();
        state.store.patch_if("remote_worker","n2","workerEpoch",&json!(epoch),
            &json!({"allowWrites":true})).unwrap();
        let claimed=claim(&state,&json!({"workerId":"n2","workerEpoch":epoch}),now).unwrap();
        let lease=claimed["task"]["leaseId"].as_str().unwrap();
        let request=json!({"workerId":"n2","workerEpoch":epoch,"taskId":"write-1",
            "leaseId":lease,"localJobId":"local-1","jobStatus":"completed","verified":true});
        assert!(settle(&state,&request,now+LEASE_MS+1).is_err());
        assert_eq!(task_status(&state,"write-1").unwrap()["status"],"running");
        expire(&state,now+LEASE_MS+2);
        assert_eq!(state.store.get("remote_task","write-1").unwrap()["status"],"needs_review");
        assert!(settle(&state,&request,now+LEASE_MS+3).is_err());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn real_task_settlement_is_one_time_and_source_addressed(){
        let (state,dir)=state();
        let epoch=uuid::Uuid::new_v4().to_string();
        register(&state,&json!({"workerId":"n3","workerEpoch":epoch,
            "agents":["research"],"allowWrites":false})).unwrap();
        state.store.put("remote_task",&json!({"id":"task3","workerId":"n3",
            "agentId":"research","prompt":"Research","readOnly":true,
            "status":"queued","createdAtMs":1})).unwrap();
        let now=now_ms();
        let claim=claim(&state,&json!({"workerId":"n3","workerEpoch":epoch}),now).unwrap();
        let receipt=json!({"workerId":"n3","workerEpoch":epoch,"taskId":"task3",
            "leaseId":claim["task"]["leaseId"],"localJobId":"local3",
            "jobStatus":"completed","answer":"answer","verified":true});
        let completed=settle(&state,&receipt,now+5).unwrap();
        assert_eq!(completed["status"],"completed");
        assert!(settle(&state,&receipt,now+6).unwrap()["replayed"]==true);
        assert_eq!(state.store.get("remote_task","task3").unwrap()["answer"],"answer");
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
}
