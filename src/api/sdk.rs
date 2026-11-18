//! Compatibility API for historical /api/harness/runs clients.
//! New work goes to the same durable Rust kernel as /api/sdk/runs.
//! Old sdk_run/FileJournal entries remain inspectable, never re-executable.
use crate::state::AppState;
use axum::{
    body::to_bytes, extract::{Path,State}, http::StatusCode,
    response::{IntoResponse,Response}, Json,
};
use serde::Deserialize;
use serde_json::{json,Value};
use super::{response::error,sdk_runs};

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Start {
    instruction:String,
    provider_id:Option<String>,
    model:Option<String>,
    verify_command:Option<String>,
    max_rounds:Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Approve { call_id:String }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ReconcileInput {applied:bool,details:String}

fn native_job(state:&AppState,id:&str)->Option<Value>{
    state.store.get("job",id).filter(|job|
        job["kind"]=="prompt"
            && job["input"]["clientSurface"]=="native_agent_panel"
            && job["input"]["harnessCompat"]==true)
}

fn native_view(state:&AppState,job:&Value)->Value {
    let mut view=sdk_runs::project(state,job);
    view["providerId"]=job["input"]["providerId"].clone();
    view["model"]=job["input"]["model"].clone();
    view["maxRounds"]=job["input"]["limits"]["maxSteps"].clone();
    view["verifyCommand"]=job["input"]["checks"].as_array()
        .and_then(|checks|checks.iter().find_map(|check|
            (check["type"]=="tool_succeeded" && check["tool"]=="shell_exec")
                .then(||check["contains"].clone())))
        .unwrap_or(Value::Null);
    view
}

fn legacy_checkpoint(state:&AppState,id:&str)->Value {
    // Existing legacy IDs were UUIDs. Never construct filesystem paths from
    // arbitrary record IDs, nor read unbounded past transcripts into an API.
    if uuid::Uuid::parse_str(id).is_err(){return Value::Null}
    let path=state.config.data_dir.join("sdk-runs").join(format!("{id}.json"));
    if !std::fs::metadata(&path).is_ok_and(|meta|
        meta.is_file() && meta.len()<=2_000_000) {return Value::Null}
    let Ok(text)=std::fs::read_to_string(path) else {return Value::Null};
    let Ok(snapshot)=serde_json::from_str::<Value>(&text) else {return Value::Null};
    json!({
        "state":snapshot["status"],
        "rounds":snapshot["rounds"],
        "toolCalls":snapshot["tool_calls"],
        "usage":snapshot["usage"],
        "events":snapshot["events"],
        "pendingCall":snapshot["pending_calls"].as_array().and_then(|calls|calls.first()),
        "answer":Value::Null,
        "archiveOnly":true
    })
}

fn legacy_view(state:&AppState,mut record:Value)->Value {
    let id=record["id"].as_str().unwrap_or("").to_string();
    record["checkpoint"]=legacy_checkpoint(state,&id);
    if record["status"]=="running" || record["status"]=="queued" {
        record["status"]=json!("interrupted");
    }
    record["archiveOnly"]=json!(true);
    record["recoveryPolicy"]=json!("Recreate as a new native job. The retired runner cannot replay an uncertain effect.");
    record
}

fn native_reply(state:&AppState,id:&str,response:Response)->Response {
    let status=response.status();
    if status.is_success() {
        if let Some(job)=native_job(state,id){
            return (status,Json(native_view(state,&job))).into_response()
        }
    }
    response
}

pub async fn list(State(state):State<AppState>)->Json<Value>{
    let mut entries=state.store.all("job").into_iter()
        .filter(|job|job["kind"]=="prompt"
            && job["input"]["clientSurface"]=="native_agent_panel"
            && job["input"]["harnessCompat"]==true)
        .map(|job|native_view(&state,&job)).collect::<Vec<_>>();
    entries.extend(state.store.all("sdk_run").into_iter()
        .map(|entry|legacy_view(&state,entry)));
    entries.truncate(30);
    Json(json!(entries))
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response {
    if let Some(record)=state.store.get("sdk_run",&id){
        return Json(legacy_view(&state,record)).into_response()
    }
    match native_job(&state,&id){
        Some(job)=>Json(native_view(&state,&job)).into_response(),
        None=>error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.")
    }
}
pub async fn start(State(state):State<AppState>,Json(input):Json<Start>)->Response{
    let response=sdk_runs::create(State(state.clone()),Json(json!({
        "instruction":input.instruction,
        "providerId":input.provider_id,
        "model":input.model,
        "verifyCommand":input.verify_command,
        "maxRounds":input.max_rounds.unwrap_or(24).clamp(1,80),
        "harnessCompat":true
    }))).await;
    let status=response.status();
    if !status.is_success(){return response}
    let bytes=match to_bytes(response.into_body(),1024*1024).await{
        Ok(bytes)=>bytes,
        Err(_)=>return error(StatusCode::INTERNAL_SERVER_ERROR,
            "작업 생성 결과를 읽지 못했습니다.")
    };
    let view:Value=match serde_json::from_slice(&bytes){
        Ok(view)=>view,
        Err(_)=>return error(StatusCode::INTERNAL_SERVER_ERROR,
            "작업 생성 결과 형식이 잘못됐습니다.")
    };
    let Some(id)=view["id"].as_str() else {
        return error(StatusCode::INTERNAL_SERVER_ERROR,"새 작업 ID가 없습니다.")
    };
    match native_job(&state,id){
        Some(job)=>(status,Json(native_view(&state,&job))).into_response(),
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,
            "네이티브 작업이 저장되지 않았습니다.")
    }
}
pub async fn approve(State(state):State<AppState>,Path(id):Path<String>,
    Json(input):Json<Approve>)->Response{
    if native_job(&state,&id).is_some(){
        let response=sdk_runs::approve(State(state.clone()),Path(id.clone()),
            Json(json!({"callId":input.call_id}))).await;
        return native_reply(&state,&id,response)
    }
    legacy_closed(&state,&id)
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if native_job(&state,&id).is_some(){
        let response=sdk_runs::cancel(State(state.clone()),Path(id.clone())).await;
        return native_reply(&state,&id,response)
    }
    legacy_closed(&state,&id)
}
pub async fn resume(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if native_job(&state,&id).is_some(){
        let response=sdk_runs::resume(State(state.clone()),Path(id.clone())).await;
        return native_reply(&state,&id,response)
    }
    legacy_closed(&state,&id)
}
pub async fn reconcile(State(state):State<AppState>,Path(id):Path<String>,
    Json(input):Json<ReconcileInput>)->Response{
    if native_job(&state,&id).is_some(){
        let response=sdk_runs::reconcile(State(state.clone()),Path(id.clone()),
            Json(json!({"applied":input.applied,"details":input.details}))).await;
        return native_reply(&state,&id,response)
    }
    legacy_closed(&state,&id)
}
fn legacy_closed(state:&AppState,id:&str)->Response{
    if state.store.get("sdk_run",id).is_some(){
        error(StatusCode::CONFLICT,
            "이전 하네스 실행기는 종료되었습니다. 기록은 보존되며, 새 작업은 공통 Rust 커널에서 시작하세요.")
    } else {
        error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!(
            "sub3-sdk-cutover-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.join("work")
        };
        std::fs::create_dir_all(&config.workspace).unwrap();
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        (state,root)
    }
    #[tokio::test]
    async fn new_harness_api_jobs_use_one_kernel_and_no_second_checkpoint(){
        let (state,root)=state();
        state.store.put("provider",&json!({
            "id":"compat-provider","name":"Compatibility fixture",
            "kind":"openai","enabled":true,"toolMode":"native","model":"test-model",
            "baseUrl":"http://127.0.0.1:9/v1"
        })).unwrap();
        let response=start(State(state.clone()),Json(Start{
            instruction:"Inspect a workspace file".into(),
            provider_id:Some("compat-provider".into()),model:Some("test-model".into()),
            verify_command:None,max_rounds:Some(3),
        })).await;
        assert_eq!(response.status(),StatusCode::ACCEPTED);
        let body=to_bytes(response.into_body(),1024*1024).await.unwrap();
        let view:Value=serde_json::from_slice(&body).unwrap();
        let id=view["id"].as_str().unwrap();
        let job=state.store.get("job",id).unwrap();
        assert_eq!(job["input"]["harnessCompat"],true);
        assert_eq!(job["input"]["limits"]["maxSteps"],3);
        assert_eq!(job["input"]["model"],"test-model");
        assert_eq!(job["input"]["limits"]["maxToolCalls"],160);
        let allowed=job["input"]["allowedTools"].as_array().unwrap();
        assert!(allowed.iter().any(|tool|tool=="workspace_read"));
        assert!(!allowed.iter().any(|tool|tool=="browser_control"
            ||tool.as_str().is_some_and(|name|name.starts_with("mcp__"))));
        assert!(state.store.get("sdk_run",id).is_none());
        assert_eq!(one(State(state.clone()),Path(id.into())).await.status(),StatusCode::OK);
        assert_eq!(list(State(state.clone())).await.0.as_array().unwrap().len(),1);
        assert!(sdk_runs::list(State(state.clone())).await.0.as_array().unwrap().is_empty());
        if let Some(session)=job["sessionId"].as_str(){state.cancel_session(session);}
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn legacy_records_are_readable_but_never_executed_again(){
        let (state,root)=state();
        let id=uuid::Uuid::new_v4().to_string();
        state.store.put("sdk_run",&json!({
            "id":id,"instruction":"Old task","status":"running"
        })).unwrap();
        let snapshot=state.config.data_dir.join("sdk-runs").join(format!("{id}.json"));
        std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        std::fs::write(&snapshot,json!({
            "status":{"state":"in_doubt","detail":{"call_id":"x","tool":"shell"}},
            "rounds":2,"tool_calls":1,"pending_calls":[],
            "events":[],"usage":{"input_tokens":10}
        }).to_string()).unwrap();
        let result=one(State(state.clone()),Path(id.clone())).await;
        assert_eq!(result.status(),StatusCode::OK);
        let bytes=to_bytes(result.into_body(),1024*1024).await.unwrap();
        let view:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view["status"],"interrupted");
        assert_eq!(view["archiveOnly"],true);
        assert_eq!(view["checkpoint"]["rounds"],2);
        assert_eq!(resume(State(state.clone()),Path(id.clone())).await.status(),
            StatusCode::CONFLICT);
        assert_eq!(cancel(State(state.clone()),Path(id.clone())).await.status(),
            StatusCode::CONFLICT);
        assert!(state.store.get("sdk_run",&id).is_some());
        let _=std::fs::remove_dir_all(root);
    }
}
