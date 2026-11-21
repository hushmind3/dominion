//! Optional distributed worker ingress. Remote workers run their OWN local
//! DOMINION Rust SDK. The coordinator only leases instructions and journals
//! reports; it never treats remote self-report as native verified evidence.
use crate::state::AppState;
use axum::{
    extract::{Path,State},
    http::{HeaderMap,StatusCode},
    response::{IntoResponse,Response},
    Json
};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
const LEASE_SECS:i64=75;
fn reply(code:StatusCode,message:&str)->Response{
    (code,Json(json!({"error":message}))).into_response()
}
fn valid_worker_id(id:&str)->bool{
    !id.is_empty()&&id.len()<=96&&id.bytes().all(|b|b.is_ascii_alphanumeric()
        ||matches!(b,b'-'|b'_'|b'.'|b':'))
}
fn worker_auth(headers:&HeaderMap)->Result<(),Response>{
    let secret=std::env::var("DOMINION_REMOTE_WORKER_TOKEN").unwrap_or_default();
    if secret.len()<32{return Err(reply(StatusCode::SERVICE_UNAVAILABLE,
        "원격 Worker 토큰(32자 이상)이 설정되지 않았습니다."));}
    let candidate=headers.get("x-dominion-worker-token").and_then(|s|s.to_str().ok())
        .unwrap_or("");
    let expected=Sha256::digest(secret.as_bytes());
    let actual=Sha256::digest(candidate.as_bytes());
    let difference=expected.iter().zip(actual.iter()).fold(0u8,|acc,(a,b)|acc|(a^b));
    if difference!=0{return Err(reply(StatusCode::UNAUTHORIZED,"원격 Worker 인증이 필요합니다."));}
    Ok(())
}
fn lease_alive(row:&Value,worker:&str,lease:&str)->bool{
    row["status"]=="leased"&&row["workerId"]==worker&&row["leaseId"]==lease
        &&row["leaseExpiresAt"].as_str()
            .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|until|until>chrono::Utc::now())
}
fn owner_view(mut row:Value)->Value{
    // The remote report and authorization are not equivalent to verification
    // by the coordinator's own native Tool Gateway.
    row["nativeVerified"]=json!(false);
    row
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    let mut jobs=state.store.all("remote_job");
    jobs.sort_by(|a,b|b["createdAt"].as_str().unwrap_or("")
        .cmp(a["createdAt"].as_str().unwrap_or("")));
    jobs.truncate(100);
    Json(json!(jobs.into_iter().map(owner_view).collect::<Vec<_>>()))
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match state.store.get("remote_job",&id){
        Some(row)=>Json(owner_view(row)).into_response(),
        None=>reply(StatusCode::NOT_FOUND,"원격 작업을 찾을 수 없습니다.")
    }
}
pub async fn create(State(state):State<AppState>,Json(input):Json<Value>)->Response{
    if state.store.count("remote_job")>1000{
        return reply(StatusCode::TOO_MANY_REQUESTS,"원격 작업 보관 한도를 초과했습니다.");
    }
    let Some(instruction)=input["instruction"].as_str()
        .filter(|s|!s.trim().is_empty()&&s.len()<=16_000)else{
        return reply(StatusCode::BAD_REQUEST,"원격 작업 지시(최대 16000바이트)가 필요합니다.");
    };
    let verify=input["verifyCommand"].as_str();
    if verify.is_some_and(|s|s.len()>2048){
        return reply(StatusCode::BAD_REQUEST,"검증 명령이 너무 깁니다.");
    }
    let row=json!({
        "id":format!("remote:{}",uuid::Uuid::new_v4()),
        "kind":"remote_sdk","status":"queued","attempts":0,
        "instruction":instruction,"verifyCommand":verify,
        "maxRounds":input["maxRounds"].as_u64().unwrap_or(24).clamp(1,80),
        "createdAt":chrono::Utc::now().to_rfc3339(),
        "nativeVerified":false
    });
    match state.store.put("remote_job",&row){
        Some(saved)=>{state.bus.record("remote_job",&saved);
            (StatusCode::ACCEPTED,Json(owner_view(saved))).into_response()},
        None=>reply(StatusCode::INTERNAL_SERVER_ERROR,"원격 작업을 저장하지 못했습니다.")
    }
}
pub async fn claim(State(state):State<AppState>,headers:HeaderMap,
    Json(body):Json<Value>)->Response{
    if let Err(error)=worker_auth(&headers){return error;}
    let Some(worker)=body["workerId"].as_str().filter(|w|valid_worker_id(w))else{
        return reply(StatusCode::BAD_REQUEST,"유효한 Worker ID가 필요합니다.");
    };
    // Only queued tasks may be claimed. A lost or expired lease is review
    // required, not automatic re-execution of unknown remote effects.
    let mut jobs=state.store.all("remote_job");
    jobs.sort_by(|a,b|a["createdAt"].as_str().unwrap_or("")
        .cmp(b["createdAt"].as_str().unwrap_or("")));
    for row in jobs.into_iter().filter(|r|r["status"]=="queued").take(100){
        let Some(id)=row["id"].as_str()else{continue};
        let lease=format!("lease:{}",uuid::Uuid::new_v4());
        let expires=(chrono::Utc::now()+chrono::Duration::seconds(LEASE_SECS))
            .to_rfc3339();
        let patch=json!({"status":"leased","workerId":worker,"leaseId":lease,
            "leaseExpiresAt":expires,"claimedAt":chrono::Utc::now().to_rfc3339(),
            "attempts":1});
        if let Some(saved)=state.store.patch_if("remote_job",id,"status",
            &json!("queued"),&patch){
            state.bus.record("remote_job",&saved);
            return Json(json!({"jobId":id,"leaseId":lease,
                "leaseExpiresAt":expires,"instruction":saved["instruction"],
                "verifyCommand":saved["verifyCommand"],
                "maxRounds":saved["maxRounds"]})).into_response();
        }
    }
    (StatusCode::NO_CONTENT,Json(Value::Null)).into_response()
}
pub async fn heartbeat(State(state):State<AppState>,headers:HeaderMap,
    Path(id):Path<String>,Json(body):Json<Value>)->Response{
    if let Err(error)=worker_auth(&headers){return error;}
    let worker=body["workerId"].as_str().unwrap_or("");
    let lease=body["leaseId"].as_str().unwrap_or("");
    let until=(chrono::Utc::now()+chrono::Duration::seconds(LEASE_SECS)).to_rfc3339();
    let result=state.store.mutate_record("remote_job",&id,json!({"id":id}),
        |row|{
            if !lease_alive(row,worker,lease){return Err("lease_invalid".into());}
            row["leaseExpiresAt"]=json!(until);
            row["heartbeatAt"]=json!(chrono::Utc::now().to_rfc3339());
            Ok(vec![])
        });
    match result{
        Ok(saved)=>{state.bus.record("remote_job",&saved);
            Json(json!({"ok":true,"leaseExpiresAt":saved["leaseExpiresAt"]})).into_response()},
        Err(_)=>reply(StatusCode::CONFLICT,"임대가 만료되었거나 작업이 중단됐습니다.")
    }
}
pub async fn finish(State(state):State<AppState>,headers:HeaderMap,
    Path(id):Path<String>,Json(body):Json<Value>)->Response{
    if let Err(error)=worker_auth(&headers){return error;}
    let worker=body["workerId"].as_str().unwrap_or("");
    let lease=body["leaseId"].as_str().unwrap_or("");
    let Some(result)=body.get("result").filter(|v|v.is_object()
        &&v.to_string().len()<=32_000)else{
        return reply(StatusCode::BAD_REQUEST,"제한된 결과 JSON이 필요합니다.");
    };
    let evidence=result.clone();
    let changed=state.store.mutate_record("remote_job",&id,json!({"id":id}),
        |row|{
            if !lease_alive(row,worker,lease){return Err("lease_invalid".into());}
            row["status"]=json!("needs_review");
            row["workerReport"]=evidence.clone();
            row["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
            row["leaseId"]=Value::Null;
            row["nativeVerified"]=json!(false);
            Ok(vec![])
        });
    match changed{
        Ok(saved)=>{state.bus.record("remote_job",&saved);
            Json(owner_view(saved)).into_response()},
        Err(_)=>reply(StatusCode::CONFLICT,"이미 처리되었거나 소유하지 않은 임대입니다.")
    }
}
pub async fn review(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let accepted=body["accept"].as_bool();
    if accepted.is_none(){return reply(StatusCode::BAD_REQUEST,"accept 불리언 값이 필요합니다.");}
    let result=state.store.patch_if("remote_job",&id,"status",&json!("needs_review"),
        &json!({"status":if accepted==Some(true){"owner_accepted"}else{"owner_rejected"},
            "ownerReviewedAt":chrono::Utc::now().to_rfc3339(),
            "nativeVerified":false}));
    match result{
        Some(saved)=>{state.bus.record("remote_job",&saved);
            Json(owner_view(saved)).into_response()},
        None=>reply(StatusCode::CONFLICT,"검토할 원격 결과가 없습니다.")
    }
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let result=state.store.mutate_record("remote_job",&id,json!({"id":id}),
        |row|{
            if !matches!(row["status"].as_str(),Some("queued"|"leased"|"needs_review")){
                return Err("already_terminal".into());
            }
            row["status"]=json!("cancelled");
            row["cancelRequested"]=json!(true);
            row["leaseId"]=Value::Null;
            row["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
            Ok(vec![])
        });
    match result{
        Ok(saved)=>{state.bus.record("remote_job",&saved);
            Json(owner_view(saved)).into_response()},
        Err(_)=>reply(StatusCode::CONFLICT,"이미 종료된 원격 작업입니다.")
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn fixture()->AppState{
        let root=std::env::temp_dir().join(format!("dominion-remote-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()))
    }
    #[tokio::test]
    async fn cancelled_remote_lease_cannot_heartbeat_or_report_unverified_as_native(){
        let state=fixture();
        let created=create(State(state.clone()),Json(json!({
            "instruction":"inspect source","maxRounds":8}))).await;
        assert_eq!(created.status(),StatusCode::ACCEPTED);
        let row=state.store.all("remote_job").remove(0);
        let id=row["id"].as_str().unwrap().to_owned();
        state.store.patch_if("remote_job",&id,"status",&json!("queued"),&json!({
            "status":"leased","workerId":"worker","leaseId":"token",
            "leaseExpiresAt":(chrono::Utc::now()+chrono::Duration::seconds(60)).to_rfc3339()
        })).unwrap();
        assert!(lease_alive(&state.store.get("remote_job",&id).unwrap(),"worker","token"));
        assert!(!lease_alive(&state.store.get("remote_job",&id).unwrap(),"other","token"));
        let cancel=cancel(State(state.clone()),Path(id.clone())).await;
        assert_eq!(cancel.status(),StatusCode::OK);
        assert!(!lease_alive(&state.store.get("remote_job",&id).unwrap(),"worker","token"));
        assert_eq!(state.store.get("remote_job",&id).unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("remote_job",&id).unwrap()["nativeVerified"],false);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn expired_lease_fails_closed_and_auth_requires_explicit_secret(){
        let sample=json!({"status":"leased","leaseId":"lease",
            "workerId":"worker","leaseExpiresAt":"2026-01-01T00:00:00Z"});
        assert!(!lease_alive(&sample,"worker","lease"));
        assert!(!valid_worker_id("../worker"));
        assert!(valid_worker_id("worker-a"));
    }
}
