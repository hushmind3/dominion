//! Owner-controlled multi-device worker registry and capability scheduler.
//! Credentials are scoped to a single worker identity; the token hash, not
//! plaintext, is durable. A remote self-report never becomes a Core receipt.
use crate::state::AppState;
use axum::{
    extract::{Path,State},http::{HeaderMap,StatusCode},
    response::{IntoResponse,Response},Json
};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn error(code:StatusCode,message:&str)->Response{
    (code,Json(json!({"error":message}))).into_response()
}
pub fn valid_id(id:&str)->bool{
    !id.is_empty()&&id.len()<=96&&id.bytes().all(|b|
        b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'|b':'))
}
fn digest(secret:&str)->String{
    format!("{:x}",Sha256::digest(secret.as_bytes()))
}
fn match_secret(candidate:&str,expected_hash:&str)->bool{
    let actual=digest(candidate);
    expected_hash.len()==64
        &&actual.bytes().zip(expected_hash.bytes()).fold(0u8,|diff,(a,b)|diff|(a^b))==0
}
fn new_secret()->String{
    format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple())
}
pub fn capabilities(value:&Value)->Result<Vec<String>,String>{
    if value.is_null(){return Ok(vec![]);}
    let Some(array)=value.as_array()else{return Err("capabilities must be an array".into());};
    if array.len()>16{return Err("too many capability tags".into());}
    let mut unique=BTreeSet::new();
    for item in array{
        let name=item.as_str().ok_or("capability must be a string")?;
        if name.is_empty()||name.len()>48||!name.bytes().all(|b|
            b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.')){
            return Err("invalid capability tag".into());
        }
        unique.insert(name.to_string());
    }
    Ok(unique.into_iter().collect())
}
fn public(mut row:Value)->Value{
    if let Some(fields)=row.as_object_mut(){fields.remove("tokenSha256");}
    row
}
/// Registered identities always require their own token: the legacy shared
/// secret cannot impersonate them after an owner has enrolled them.
pub fn authenticate(state:&AppState,headers:&HeaderMap,worker:&str)->Result<(),Response>{
    if !valid_id(worker){return Err(error(StatusCode::BAD_REQUEST,"유효하지 않은 Worker ID입니다."));}
    let token=headers.get("x-dominion-worker-token")
        .and_then(|h|h.to_str().ok()).unwrap_or("");
    if let Some(row)=state.store.get("remote_worker",worker){
        if row["status"]!="enabled"{
            return Err(error(StatusCode::FORBIDDEN,"비활성화된 Worker입니다."));
        }
        if !match_secret(token,row["tokenSha256"].as_str().unwrap_or("")){
            return Err(error(StatusCode::UNAUTHORIZED,"Worker 토큰이 일치하지 않습니다."));
        }
        return Ok(());
    }
    // Compatibility only for pre-enrollment workers without constraints.
    let legacy=std::env::var("DOMINION_REMOTE_WORKER_TOKEN").unwrap_or_default();
    if legacy.len()<32{
        return Err(error(StatusCode::SERVICE_UNAVAILABLE,
            "Worker 등록 또는 기존 공유 토큰 설정이 필요합니다."));
    }
    if !match_secret(token,&digest(&legacy)){
        return Err(error(StatusCode::UNAUTHORIZED,"원격 Worker 인증이 필요합니다."));
    }
    Ok(())
}
pub fn matches_capabilities(state:&AppState,worker:&str,required:&[String])->bool{
    let Some(row)=state.store.get("remote_worker",worker)else{return false;};
    row["status"]=="enabled"&&required.iter().all(|tag|
        row["capabilities"].as_array().is_some_and(|labels|
            labels.iter().any(|v|v.as_str()==Some(tag.as_str()))))
}
/// Jobs without machine pinning and without required tags remain eligible for
/// older agents; constrained jobs require an enabled, owner-enrolled machine.
pub fn eligible(state:&AppState,row:&Value,worker:&str)->bool{
    if row["targetWorkerId"].as_str().is_some_and(|id|id!=worker){return false;}
    let required=row["requiredCapabilities"].as_array().map(|v|v.iter()
        .filter_map(|tag|tag.as_str().map(str::to_owned)).collect::<Vec<_>>())
        .unwrap_or_default();
    if required.is_empty()&&row["targetWorkerId"].is_null(){return true;}
    matches_capabilities(state,worker,&required)
}
fn liveness_due(last:&Value)->bool{
    last.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_none_or(|stamp|stamp<=chrono::Utc::now()-chrono::Duration::seconds(30))
}
pub fn seen(state:&AppState,worker:&str){
    // Polling every three seconds must not append one durable SSE event per
    // machine per poll. Double-check freshness under the SQLite CAS lock.
    let Some(snapshot)=state.store.get("remote_worker",worker)else{return;};
    if !liveness_due(&snapshot["lastSeenAt"]){return;}
    if let Ok(saved)=state.store.mutate_record("remote_worker",worker,
        json!({"id":worker}),|row|{
            if row["status"]!="enabled"||!liveness_due(&row["lastSeenAt"]){
                return Err("worker liveness already committed".into());
            }
            row["lastSeenAt"]=json!(chrono::Utc::now().to_rfc3339());
            Ok(vec![])
        }){
        state.bus.record("remote_worker",&public(saved));
    }
}
/// POST /api/remote/workers is owner-only. Return the credential exactly once.
pub async fn enroll(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let Some(id)=body["workerId"].as_str().filter(|id|valid_id(id))else{
        return error(StatusCode::BAD_REQUEST,"유효한 Worker ID가 필요합니다.");
    };
    let tags=match capabilities(&body["capabilities"]){
        Ok(tags) if tags.is_empty()=>vec!["sdk".to_string()],
        Ok(tags)=>tags,
        Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
    };
    if state.store.count("remote_worker")>=128{
        return error(StatusCode::TOO_MANY_REQUESTS,"등록 가능한 Worker가 가득 찼습니다.");
    }
    let secret=new_secret();
    let row=json!({"id":id,"kind":"remote_worker","status":"enabled",
        "protocolVersion":1,"capabilities":tags,"tokenSha256":digest(&secret),
        "registeredAt":chrono::Utc::now().to_rfc3339(),"lastSeenAt":null});
    match state.store.put_if_absent("remote_worker",&row){
        Some(saved)=>{
            state.bus.record("remote_worker",&public(saved.clone()));
            (StatusCode::CREATED,Json(json!({
                "worker":public(saved),"token":secret
            }))).into_response()
        },
        None=>error(StatusCode::CONFLICT,"이미 등록된 Worker ID입니다."),
    }
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    Json(json!(state.store.all("remote_worker").into_iter()
        .map(public).collect::<Vec<_>>()))
}
/// Owner may pause a machine; existing leases will be quarantined for review
/// instead of silently migrated to a different machine.
pub async fn state(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let Some(enabled)=body["enabled"].as_bool()else{
        return error(StatusCode::BAD_REQUEST,"enabled 불리언 값이 필요합니다.");
    };
    match state.store.mutate_record("remote_worker",&id,json!({"id":id}),|row|{
        if row["tokenSha256"].is_null(){return Err("not enrolled".into());}
        row["status"]=json!(if enabled{"enabled"}else{"disabled"});
        row["updatedAt"]=json!(chrono::Utc::now().to_rfc3339());
        Ok(vec![])
    }){
        Ok(saved)=>{
            state.bus.record("remote_worker",&public(saved.clone()));
            Json(public(saved)).into_response()
        },
        Err(_)=>error(StatusCode::NOT_FOUND,"등록된 Worker를 찾을 수 없습니다."),
    }
}
/// Old tokens immediately fail, including heartbeats for an earlier lease.
pub async fn rotate(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let token=new_secret();
    let hash=digest(&token);
    match state.store.mutate_record("remote_worker",&id,json!({"id":id}),|row|{
        if row["tokenSha256"].is_null(){return Err("not enrolled".into());}
        row["tokenSha256"]=json!(hash);
        row["rotatedAt"]=json!(chrono::Utc::now().to_rfc3339());
        Ok(vec![])
    }){
        Ok(saved)=>{
            state.bus.record("remote_worker",&public(saved.clone()));
            Json(json!({"worker":public(saved),"token":token})).into_response()
        },
        Err(_)=>error(StatusCode::NOT_FOUND,"등록된 Worker를 찾을 수 없습니다."),
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use axum::http::HeaderValue;
    use crate::{config::Config,db::Store,vault::Vault};
    fn sample()->AppState{
        let root=std::env::temp_dir().join(format!("remote-identity-{}",uuid::Uuid::new_v4()));
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.clone(),workspace:root};
        AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()))
    }
    fn headers(token:&str)->HeaderMap{
        let mut h=HeaderMap::new();
        h.insert("x-dominion-worker-token",HeaderValue::from_str(token).unwrap());
        h
    }
    #[tokio::test]
    async fn per_device_credentials_cannot_cross_worker_ids_or_survive_rotation(){
        let state=sample();
        let response=enroll(State(state.clone()),Json(json!({
            "workerId":"gpu","capabilities":["sdk","gpu","gpu"]}))).await;
        assert_eq!(response.status(),StatusCode::CREATED);
        let raw=axum::body::to_bytes(response.into_body(),4096).await.unwrap();
        let enrolled:Value=serde_json::from_slice(&raw).unwrap();
        let token=enrolled["token"].as_str().unwrap();
        assert_eq!(enrolled["worker"]["capabilities"],json!(["gpu","sdk"]));
        assert!(enrolled["worker"].get("tokenSha256").is_none());
        assert!(authenticate(&state,&headers(token),"gpu").is_ok());
        assert!(authenticate(&state,&headers(token),"cpu").is_err());
        assert!(matches_capabilities(&state,"gpu",&["gpu".to_string()]));
        assert!(!matches_capabilities(&state,"gpu",&["vision".to_string()]));
        assert_eq!(list(State(state.clone())).await.0[0].get("tokenSha256"),None);
        let response=rotate(State(state.clone()),Path("gpu".into())).await;
        assert_eq!(response.status(),StatusCode::OK);
        let raw=axum::body::to_bytes(response.into_body(),4096).await.unwrap();
        let rotated:Value=serde_json::from_slice(&raw).unwrap();
        assert!(authenticate(&state,&headers(token),"gpu").is_err());
        assert!(authenticate(&state,&headers(rotated["token"].as_str().unwrap()),"gpu").is_ok());
        let disabled=state_fn_for_test(&state).await;
        assert_eq!(disabled.status(),StatusCode::OK);
        assert!(authenticate(&state,&headers(rotated["token"].as_str().unwrap()),"gpu").is_err());
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    async fn state_fn_for_test(app:&AppState)->Response{
        super::state(State(app.clone()),Path("gpu".into()),
            Json(json!({"enabled":false}))).await
    }
    #[test]
    fn liveness_is_throttled_and_only_registered_workers_emit_durable_updates(){
        let state=sample();
        assert!(liveness_due(&json!("2026-01-01T00:00:00Z")));
        seen(&state,"unknown");
        assert!(state.store.get("remote_worker","unknown").is_none());
        state.store.put("remote_worker",&json!({"id":"node","status":"enabled",
            "lastSeenAt":"2026-01-01T00:00:00Z"})).unwrap();
        seen(&state,"node");
        let first=state.store.get("remote_worker","node").unwrap();
        assert!(!liveness_due(&first["lastSeenAt"]));
        let stamp=first["lastSeenAt"].clone();
        seen(&state,"node");
        assert_eq!(state.store.get("remote_worker","node").unwrap()["lastSeenAt"],stamp);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn scheduler_filters_pinned_and_capability_jobs_without_trusting_self_report(){
        let state=sample();
        state.store.put("remote_worker",&json!({"id":"gpu","status":"enabled",
            "capabilities":["sdk","gpu"]})).unwrap();
        let gpu=json!({"targetWorkerId":"gpu","requiredCapabilities":["gpu"]});
        assert!(eligible(&state,&gpu,"gpu"));
        assert!(!eligible(&state,&gpu,"other"));
        assert!(!eligible(&state,&json!({"requiredCapabilities":["vision"]}),"gpu"));
        assert!(eligible(&state,&json!({}),"legacy"));
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
}
