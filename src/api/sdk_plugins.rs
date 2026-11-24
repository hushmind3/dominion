//! Local external SDK client enrollment. Tokens grant only the registered
//! client's limited run submission, heartbeat and own-run reads, never
//! approval, owner administration, arbitrary options or another client's data.
use crate::state::AppState;
use axum::{extract::{Path,State},http::{HeaderMap,Method,StatusCode},
    response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use super::{response::error,sdk};

const KIND:&str="sdk_plugin";

pub(super) fn ingress(method:&Method,path:&str)->bool {
    // Only the exact credential-authenticated write routes bypass the owner
    // token. Host and origin checks remain in the existing middleware.
    if method!=Method::POST {return false;}
    let parts=path.split('/').collect::<Vec<_>>();
    matches!(parts.as_slice(),["","api","sdk","plugins",id,"heartbeat"|"runs"]
        if super::remote_registry::valid_id(id))
}
fn digest(s:&str)->String{format!("{:x}",Sha256::digest(s.as_bytes()))}
fn token()->String{format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple())}
fn matches_hash(candidate:&str,expected:&str)->bool{
    expected.len()==64&&candidate.len()==64&&digest(candidate).bytes()
        .zip(expected.bytes()).fold(0u8,|diff,(a,b)|diff|(a^b))==0
}
fn public(mut row:Value)->Value{
    if let Some(fields)=row.as_object_mut(){fields.remove("tokenSha256");}
    row
}
fn authenticate(state:&AppState,id:&str,headers:&HeaderMap)->Result<Value,Response>{
    if !super::remote_registry::valid_id(id){return Err(error(StatusCode::BAD_REQUEST,"Invalid SDK plugin ID"));}
    let secret=headers.get("x-dominion-plugin-token").and_then(|v|v.to_str().ok()).unwrap_or("");
    let Some(row)=state.store.get(KIND,id)else{return Err(error(StatusCode::UNAUTHORIZED,"Invalid SDK plugin credential"));};
    if !matches_hash(secret,row["tokenSha256"].as_str().unwrap_or("")) {
        return Err(error(StatusCode::UNAUTHORIZED,"Invalid SDK plugin credential"));
    }
    if row["status"]!="enabled"{return Err(error(StatusCode::FORBIDDEN,"SDK plugin is disabled"));}
    Ok(row)
}
fn due(last:&Value)->bool{
    last.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_none_or(|time|time<=chrono::Utc::now()-chrono::Duration::seconds(30))
}
fn seen(state:&AppState,id:&str,row:&Value)->Result<(),Response>{
    if !due(&row["lastSeenAt"]){return Ok(());}
    let hash=row["tokenSha256"].clone();
    let saved=state.store.mutate_record(KIND,id,json!({"id":id}),|current|{
        if current["status"]!="enabled"||current["tokenSha256"]!=hash{return Err("credential_changed".into());}
        if !due(&current["lastSeenAt"]){return Err("already_seen".into());}
        current["lastSeenAt"]=json!(chrono::Utc::now().to_rfc3339());
        Ok(vec![])
    });
    match saved {
        Ok(saved)=>{state.bus.record(KIND,&public(saved));Ok(())},
        Err(e) if e=="already_seen"=>Ok(()),
        Err(e) if e=="credential_changed"=>Err(error(StatusCode::FORBIDDEN,"SDK plugin credential changed")),
        Err(_)=>Err(error(StatusCode::INTERNAL_SERVER_ERROR,"Could not record plugin heartbeat"))
    }
}

pub async fn list(State(state):State<AppState>)->Json<Value>{
    Json(json!(state.store.all(KIND).into_iter().map(public).collect::<Vec<_>>()))
}
pub async fn enroll(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let Some(id)=body["pluginId"].as_str().filter(|id|super::remote_registry::valid_id(id))else{
        return error(StatusCode::BAD_REQUEST,"Plugin ID must be 1..96 ASCII letters, digits, -_.:");
    };
    let Some(name)=body["name"].as_str().map(str::trim).filter(|n|!n.is_empty()&&n.len()<=200&&!n.chars().any(char::is_control))else{
        return error(StatusCode::BAD_REQUEST,"Plugin name is required (max 200 bytes)");
    };
    let Some(agent)=body["agentId"].as_str().filter(|id|state.store.get("agent",id).is_some())else{
        return error(StatusCode::BAD_REQUEST,"Select an existing agent for this plugin");
    };
    if state.store.count_kind(KIND)>=128{return error(StatusCode::TOO_MANY_REQUESTS,"SDK plugin registry is full");}
    let secret=token();
    let row=json!({"id":id,"name":name,"agentId":agent,"status":"enabled","protocolVersion":1,
        "registeredAt":chrono::Utc::now().to_rfc3339(),"lastSeenAt":null,"tokenSha256":digest(&secret),
        "maxRounds":8,"tokenBudget":20000,"dailyLimit":20});
    let Some(saved)=state.store.put_if_absent(KIND,&row)else{return error(StatusCode::CONFLICT,"SDK plugin ID already registered");};
    let saved=public(saved);
    state.bus.record(KIND,&saved);
    (StatusCode::CREATED,Json(json!({"plugin":saved,"token":secret}))).into_response()
}
pub async fn set_state(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(enabled)=body["enabled"].as_bool()else{return error(StatusCode::BAD_REQUEST,"enabled must be boolean");};
    match state.store.mutate_record(KIND,&id,json!({"id":id}),|row|{
        if row["tokenSha256"].is_null(){return Err("not enrolled".into());}
        row["status"]=json!(if enabled{"enabled"}else{"disabled"});
        row["lastSeenAt"]=Value::Null;
        Ok(vec![])
    }){
        Ok(saved)=>{let saved=public(saved);state.bus.record(KIND,&saved);Json(saved).into_response()},
        Err(_)=>error(StatusCode::NOT_FOUND,"SDK plugin not found")
    }
}
pub async fn rotate(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let secret=token();
    match state.store.mutate_record(KIND,&id,json!({"id":id}),|row|{
        if row["tokenSha256"].is_null(){return Err("not enrolled".into());}
        row["tokenSha256"]=json!(digest(&secret));
        row["lastSeenAt"]=Value::Null;
        Ok(vec![])
    }){
        Ok(saved)=>{let saved=public(saved);state.bus.record(KIND,&saved);
            Json(json!({"plugin":saved,"token":secret})).into_response()},
        Err(_)=>error(StatusCode::NOT_FOUND,"SDK plugin not found")
    }
}
pub async fn heartbeat(State(state):State<AppState>,Path(id):Path<String>,headers:HeaderMap)->Response{
    let row=match authenticate(&state,&id,&headers){Ok(r)=>r,Err(r)=>return r};
    if let Err(r)=seen(&state,&id,&row){return r;}
    Json(json!({"pluginId":id,"authenticated":true,"protocolVersion":1})).into_response()
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Submit{instruction:String,idempotency_key:String}

pub async fn start(State(state):State<AppState>,Path(id):Path<String>,headers:HeaderMap,Json(body):Json<Submit>)->Response{
    let plugin=match authenticate(&state,&id,&headers){Ok(r)=>r,Err(r)=>return r};
    let instruction=body.instruction.trim();
    if instruction.is_empty()||instruction.len()>20000{return error(StatusCode::BAD_REQUEST,"Instruction must be 1..20000 bytes");}
    if body.idempotency_key.is_empty()||body.idempotency_key.len()>128
        ||!body.idempotency_key.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'|b':')){
        return error(StatusCode::BAD_REQUEST,"A valid unique idempotencyKey is required");
    }
    let key=digest(&json!([id,body.idempotency_key]).to_string());
    let fingerprint=digest(instruction);
    // The source is bound to authenticated identity. The caller cannot
    // override workspace, agent, verification shell command or budgets.
    let input:sdk::Start=match serde_json::from_value(json!({"instruction":instruction,
        "agentId":plugin["agentId"],"source":{"kind":"sdk_plugin","id":id},
        "maxRounds":8,"tokenBudget":20000,"shellSandbox":"required"})){
        Ok(input)=>input,Err(_)=>return error(StatusCode::INTERNAL_SERVER_ERROR,"Invalid plugin configuration")
    };
    // Resolve earlier requests even if the selected agent is currently gone.
    // A retry never starts another run; credential check still applies.
    if let Some(claim)=state.store.get("sdk_plugin_claim",&key){
        if claim["pluginId"]!=id||claim["fingerprint"]!=fingerprint{
            return error(StatusCode::CONFLICT,"Idempotency key already used for another instruction");
        }
        return read_own(&state,&id,claim["runId"].as_str().unwrap_or(""));
    }
    let (session,mut run)=match sdk::prepare_start(&state,&input){
        Ok(pair)=>pair,Err(message)=>return error(StatusCode::BAD_REQUEST,&message)
    };
    run["sdkPluginId"]=json!(id);
    let hash=plugin["tokenSha256"].as_str().unwrap_or("");
    let (run_id,created)=match state.store.claim_plugin_sdk_run(&id,hash,&key,&fingerprint,&session,&run){
        Ok(result)=>result,
        Err(e) if e=="credential_changed"=>return error(StatusCode::FORBIDDEN,"SDK plugin credential changed"),
        Err(e) if e=="retry_conflict"=>return error(StatusCode::CONFLICT,"Idempotency key already used"),
        Err(e) if e=="active_limit"||e=="daily_limit"=>return error(StatusCode::TOO_MANY_REQUESTS,"SDK plugin task limit reached"),
        Err(_)=>return error(StatusCode::INTERNAL_SERVER_ERROR,"Could not persist plugin SDK task")
    };
    if created {
        if let Some(row)=state.store.get(KIND,&id){state.bus.record(KIND,&public(row));}
        state.bus.record("sdk_run",&run);
        sdk::launch(state.clone(),run,false);
    }
    let Some(row)=state.store.get("sdk_run",&run_id).filter(|r|r["sdkPluginId"]==id)else{
        return error(StatusCode::CONFLICT,"Existing SDK task missing; will not replay");
    };
    (if created{StatusCode::ACCEPTED}else{StatusCode::OK},Json(sdk::public_view(&state,row))).into_response()
}
fn read_own(state:&AppState,id:&str,run_id:&str)->Response{
    match state.store.get("sdk_run",run_id).filter(|r|r["sdkPluginId"]==id){
        Some(row)=>Json(sdk::public_view(state,row)).into_response(),
        None=>error(StatusCode::NOT_FOUND,"SDK task not found")
    }
}
pub async fn one(State(state):State<AppState>,Path((id,run_id)):Path<(String,String)>,headers:HeaderMap)->Response{
    let row=match authenticate(&state,&id,&headers){Ok(r)=>r,Err(r)=>return r};
    if let Err(r)=seen(&state,&id,&row){return r;}
    read_own(&state,&id,&run_id)
}

#[cfg(test)]
mod tests{
    use super::*;
    fn fixture()->AppState{
        let root=std::env::temp_dir().join(format!("sdk-plugin-{}",uuid::Uuid::new_v4()));
        let cfg=crate::config::Config{port:0,data_dir:root.clone(),web_dir:root.clone(),workspace:root};
        let state=AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),crate::vault::Vault::open(&cfg.key_path()));
        state.store.put("agent",&json!({"id":"agent","name":"Test agent"})).unwrap();
        state
    }
    fn headers(secret:&str)->HeaderMap{
        let mut h=HeaderMap::new();
        h.insert("x-dominion-plugin-token",axum::http::HeaderValue::from_str(secret).unwrap());h
    }
    async fn body(response:Response)->Value{
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(),65536).await.unwrap()).unwrap()
    }
    #[tokio::test]
    async fn plugin_credentials_are_private_scoped_and_revocable(){
        let app=fixture();
        let response=enroll(State(app.clone()),Json(json!({"pluginId":"one","name":"One","agentId":"agent"}))).await;
        assert_eq!(response.status(),StatusCode::CREATED);
        let receipt=body(response).await;
        let secret=receipt["token"].as_str().unwrap();
        assert!(receipt["plugin"].get("tokenSha256").is_none());
        assert!(list(State(app.clone())).await.0[0].get("tokenSha256").is_none());
        assert!(app.store.get(KIND,"one").unwrap()["lastSeenAt"].is_null());
        assert_eq!(heartbeat(State(app.clone()),Path("one".into()),headers(secret)).await.status(),StatusCode::OK);
        assert!(app.store.get(KIND,"one").unwrap()["lastSeenAt"].is_string());
        assert!(authenticate(&app,"two",&headers(secret)).is_err());
        app.store.put("sdk_run",&json!({"id":"other","sdkPluginId":"two"})).unwrap();
        assert_eq!(one(State(app.clone()),Path(("one".into(),"other".into())),headers(secret)).await.status(),StatusCode::NOT_FOUND);
        let rotated=body(rotate(State(app.clone()),Path("one".into())).await).await;
        assert!(authenticate(&app,"one",&headers(secret)).is_err());
        let new_secret=rotated["token"].as_str().unwrap();
        assert!(authenticate(&app,"one",&headers(new_secret)).is_ok());
        assert_eq!(set_state(State(app.clone()),Path("one".into()),Json(json!({"enabled":false}))).await.status(),StatusCode::OK);
        assert!(authenticate(&app,"one",&headers(new_secret)).is_err());
        let _=std::fs::remove_dir_all(&app.config.data_dir);
    }
    #[test]
    fn admission_is_atomic_bounded_and_cannot_cross_revocation(){
        let app=fixture();
        app.store.put(KIND,&json!({"id":"p","status":"enabled","tokenSha256":"hash"})).unwrap();
        let session=json!({"id":"s","nativeSdkRunId":"r"});
        let run=json!({"id":"r","sdkPluginId":"p","status":"queued"});
        assert_eq!(app.store.claim_plugin_sdk_run("p","hash","key","fp",&session,&run).unwrap(),("r".into(),true));
        assert_eq!(app.store.claim_plugin_sdk_run("p","hash","key","fp",&session,&run).unwrap(),("r".into(),false));
        assert_eq!(app.store.claim_plugin_sdk_run("p","hash","key","different",&session,&run).unwrap_err(),"retry_conflict");
        assert_eq!(app.store.claim_plugin_sdk_run("p","hash","key2","fp",&session,&run).unwrap_err(),"active_limit");
        app.store.patch_if(KIND,"p","status",&json!("enabled"),&json!({"status":"disabled"})).unwrap();
        assert_eq!(app.store.claim_plugin_sdk_run("p","hash","key3","fp",&session,&run).unwrap_err(),"credential_changed");
        assert_eq!(app.store.count_kind("sdk_run"),1);
        let _=std::fs::remove_dir_all(&app.config.data_dir);
    }
    #[test]
    fn only_scoped_ingress_bypasses_owner_write_token(){
        assert!(ingress(&Method::POST,"/api/sdk/plugins/p/heartbeat"));
        assert!(ingress(&Method::POST,"/api/sdk/plugins/p/runs"));
        for path in ["/api/sdk/plugins","/api/sdk/plugins/p/state","/api/sdk/plugins/p/rotate",
            "/api/sdk/plugins/p/runs/r","/api/sdk/runs","/api/sdk/runs/r/approve"]{
            assert!(!ingress(&Method::POST,path));
        }
        assert!(!ingress(&Method::DELETE,"/api/sdk/plugins/p/runs"));
        assert!(serde_json::from_value::<Submit>(json!({"instruction":"x","idempotencyKey":"a","agentId":"other"})).is_err());
    }
}
