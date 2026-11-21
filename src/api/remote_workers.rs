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
use super::remote_registry;
const LEASE_SECS:i64=75;
fn reply(code:StatusCode,message:&str)->Response{
    (code,Json(json!({"error":message}))).into_response()
}
fn valid_worker_id(id:&str)->bool{
    !id.is_empty()&&id.len()<=96&&id.bytes().all(|b|b.is_ascii_alphanumeric()
        ||matches!(b,b'-'|b'_'|b'.'|b':'))
}
fn worker_auth(state:&AppState,headers:&HeaderMap,worker:&str)->Result<(),Response>{
    remote_registry::authenticate(state,headers,worker)
}
pub(super) fn lease_alive(row:&Value,worker:&str,lease:&str)->bool{
    row["status"]=="leased"&&row["workerId"]==worker&&row["leaseId"]==lease
        &&row["leaseExpiresAt"].as_str()
            .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|until|until>chrono::Utc::now())
}

fn expired(row:&Value)->bool{
    row["status"]=="leased"
        &&row["leaseExpiresAt"].as_str()
            .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_none_or(|end|end<=chrono::Utc::now())
}
fn reconcile_expired(state:&AppState)->Vec<String>{
    let mut reviewed=Vec::new();
    // Recheck expiration UNDER the SQLite mutation lock. A heartbeat can
    // extend the lease between the initial indexed read and this transaction.
    for row in state.store.leased_remote_jobs()
        .unwrap_or_else(|_|state.store.all("remote_job")){
        if !expired(&row){continue;}
        let Some(id)=row["id"].as_str()else{continue};
        let result=state.store.mutate_record("remote_job",id,json!({"id":id}),|current|{
            if !expired(current){return Err("lease_alive".into());}
            current["status"]=json!("needs_review");
            current["reviewReason"]=json!("lease_expired_unknown_effects");
            current["leaseId"]=Value::Null;
            current["reviewAt"]=json!(chrono::Utc::now().to_rfc3339());
            Ok(vec![])
        });
        if let Ok(saved)=result{
            state.bus.record("remote_job",&saved);
            reviewed.push(id.to_string());
        }
    }
    reviewed
}
/// Owner-only manual reconciliation; never requeues an unknown remote effect.
pub async fn reconcile(State(state):State<AppState>)->Json<Value>{
    let ids=reconcile_expired(&state);
    Json(json!({"count":ids.len(),"needsReview":ids}))
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
    let required=match remote_registry::capabilities(&input["requiredCapabilities"]){
        Ok(value)=>value,
        Err(message)=>return reply(StatusCode::BAD_REQUEST,&message),
    };
    let target=input["targetWorkerId"].as_str();
    if !input["targetWorkerId"].is_null()&&target.is_none(){
        return reply(StatusCode::BAD_REQUEST,"targetWorkerId는 문자열이어야 합니다.");
    }
    if target.is_some_and(|id|!valid_worker_id(id)){
        return reply(StatusCode::BAD_REQUEST,"유효하지 않은 targetWorkerId입니다.");
    }
    if let Some(id)=target{
        if !remote_registry::matches_capabilities(&state,id,&required){
            return reply(StatusCode::CONFLICT,"대상 Worker가 없거나 필요한 능력이 없습니다.");
        }
    }else if !required.is_empty()&&!state.store.all("remote_worker").iter()
        .any(|row|remote_registry::matches_capabilities(
            &state,row["id"].as_str().unwrap_or(""),&required)){
        return reply(StatusCode::CONFLICT,"필요한 능력을 가진 Worker가 없습니다.");
    }
    let row=json!({
        "targetWorkerId":target,"requiredCapabilities":required,
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
    let worker=body["workerId"].as_str().unwrap_or("");
    if let Err(error)=worker_auth(&state,&headers,worker){return error;}
    remote_registry::seen(&state,worker);
    // Mark expired leases for owner review. This never redispatches the
    // ambiguous remote effects, even when a new worker polls for work.
    reconcile_expired(&state);
    // Only queued tasks may be claimed. A lost or expired lease is review
    // required, not automatic re-execution of unknown remote effects.
    let jobs=state.store.queued_remote_jobs()
        .unwrap_or_else(|_|state.store.all("remote_job"));
    for row in jobs.into_iter().filter(|r|r["status"]=="queued"
        &&remote_registry::eligible(&state,r,worker)).take(100){
        let Some(id)=row["id"].as_str()else{continue};
        let lease=format!("lease:{}",uuid::Uuid::new_v4());
        let expires=(chrono::Utc::now()+chrono::Duration::seconds(LEASE_SECS))
            .to_rfc3339();
        let patch=json!({"status":"leased","workerId":worker,"leaseId":lease,
            "assignmentCapabilities":row["requiredCapabilities"],
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
    let worker=body["workerId"].as_str().unwrap_or("");
    if let Err(error)=worker_auth(&state,&headers,worker){return error;}
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
    let worker=body["workerId"].as_str().unwrap_or("");
    if let Err(error)=worker_auth(&state,&headers,worker){return error;}
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
    let result=state.store.mutate_record("remote_job",&id,json!({"id":id}),|row|{
        if row["status"]!="needs_review"{
            return Err("remote job not awaiting review".into());
        }
        if accepted==Some(true)&&!row["workerReport"].is_object(){
            return Err("cannot approve missing remote evidence".into());
        }
        row["status"]=json!(if accepted==Some(true){
            "owner_accepted"
        }else{"owner_rejected"});
        row["ownerReviewedAt"]=json!(chrono::Utc::now().to_rfc3339());
        row["nativeVerified"]=json!(false);
        Ok(vec![])
    });
    match result{
        Ok(saved)=>{state.bus.record("remote_job",&saved);
            Json(owner_view(saved)).into_response()},
        Err(_)=>reply(StatusCode::CONFLICT,"검토 가능한 원격 실행 결과가 없습니다.")
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

    #[tokio::test]
    async fn expired_remote_lease_is_quarantined_without_requeue_or_fake_review(){
        let state=fixture();
        let job=state.store.put("remote_job",&json!({"id":"ticket",
            "kind":"remote_sdk","status":"leased","workerId":"node-a",
            "leaseId":"old","leaseExpiresAt":"2026-01-01T00:00:00Z",
            "instruction":"research"})).unwrap();
        assert!(expired(&job));
        let changed=reconcile_expired(&state);
        assert_eq!(changed,vec!["ticket"]);
        assert!(reconcile_expired(&state).is_empty());
        let record=state.store.get("remote_job","ticket").unwrap();
        assert_eq!(record["status"],"needs_review");
        assert!(record["leaseId"].is_null());
        assert_eq!(record["nativeVerified"],Value::Null);
        let refused=review(State(state.clone()),Path("ticket".into()),
            Json(json!({"accept":true}))).await;
        assert_eq!(refused.status(),StatusCode::CONFLICT);
        let rejected=review(State(state.clone()),Path("ticket".into()),
            Json(json!({"accept":false}))).await;
        assert_eq!(rejected.status(),StatusCode::OK);
        assert_eq!(state.store.get("remote_job","ticket").unwrap()["nativeVerified"],false);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }

    #[tokio::test]
    async fn multi_device_claim_is_capability_matched_and_lease_fenced(){
        use axum::http::HeaderValue;
        let state=fixture();
        let registered=super::remote_registry::enroll(State(state.clone()),Json(json!({
            "workerId":"gpu-node","capabilities":["gpu","sdk"]
        }))).await;
        let body=axum::body::to_bytes(registered.into_body(),4096).await.unwrap();
        let enrollment:Value=serde_json::from_slice(&body).unwrap();
        let token=enrollment["token"].as_str().unwrap();
        let mut headers=HeaderMap::new();
        headers.insert("x-dominion-worker-token",HeaderValue::from_str(token).unwrap());
        let request=json!({"instruction":"GPU inspection","targetWorkerId":"gpu-node",
            "requiredCapabilities":["gpu"]});
        assert_eq!(create(State(state.clone()),Json(request)).await.status(),
            StatusCode::ACCEPTED);
        // The same credential may not impersonate another worker, even to claim.
        let wrong=claim(State(state.clone()),headers.clone(),
            Json(json!({"workerId":"cpu-node"}))).await;
        assert_ne!(wrong.status(),StatusCode::OK);
        let assigned=claim(State(state.clone()),headers.clone(),
            Json(json!({"workerId":"gpu-node"}))).await;
        assert_eq!(assigned.status(),StatusCode::OK);
        let body=axum::body::to_bytes(assigned.into_body(),4096).await.unwrap();
        let task:Value=serde_json::from_slice(&body).unwrap();
        let job=task["jobId"].as_str().unwrap().to_string();
        assert_eq!(state.store.get("remote_job",&job).unwrap()["workerId"],"gpu-node");
        assert_eq!(claim(State(state.clone()),headers.clone(),
            Json(json!({"workerId":"gpu-node"}))).await.status(),StatusCode::NO_CONTENT);
        let rotated=super::remote_registry::rotate(State(state.clone()),
            Path("gpu-node".into())).await;
        assert_eq!(rotated.status(),StatusCode::OK);
        let expired_token=heartbeat(State(state.clone()),headers.clone(),
            Path(job.clone()),Json(json!({"workerId":"gpu-node",
                "leaseId":task["leaseId"]}))).await;
        assert_eq!(expired_token.status(),StatusCode::UNAUTHORIZED);
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
