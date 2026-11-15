use crate::{engine,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;
pub async fn list(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("job")))}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{match state.store.get("job",&id){Some(j)=>Json(j).into_response(),None=>error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.")}}
/// Client requests describe work, never mutable runtime status or evidence.
pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let kind=body["kind"].as_str().unwrap_or("prompt");let input=&body["input"];
    let clean=match kind{
        "prompt"=>{
            let Some(prompt)=input["prompt"].as_str().filter(|p|!p.trim().is_empty()&&p.len()<=64000)else{return error(StatusCode::BAD_REQUEST,"실행할 작업 지시가 필요합니다.")};
            let mut value=json!({"prompt":prompt,"toolsEnabled":input["toolsEnabled"].as_bool().unwrap_or(true)});
            for key in ["agentId","providerId","model","reasoningEffort"]{if let Some(v)=input[key].as_str().filter(|s|!s.is_empty()){value[key]=json!(v);}}
            if let Some(v)=input["thinking"].as_bool(){value["thinking"]=json!(v);}
            if let Some(v)=input["autonomous"].as_bool(){value["autonomous"]=json!(v);}
            if let Some(v)=input.get("maxAutonomousAttempts"){
                let Some(count)=v.as_u64().filter(|count|(1..=6).contains(count)) else {
                    return error(StatusCode::BAD_REQUEST,"자율 실행 시도 횟수는 1~6이어야 합니다.");
                };
                value["maxAutonomousAttempts"]=json!(count);
            }
            if let Some(v)=input.get("executionStrategy"){
                let Some(strategy)=v.as_str().filter(|s|matches!(*s,"focused"|"conservative")) else {
                    return error(StatusCode::BAD_REQUEST,"실행 전략은 focused 또는 conservative입니다.");
                };
                value["executionStrategy"]=json!(strategy);
            }
            if let Some(id)=value["agentId"].as_str(){if state.store.get("agent",id).is_none(){return error(StatusCode::BAD_REQUEST,"실행 에이전트를 찾을 수 없습니다.");}}
            value
        },
        "workflow"=>{
            let Some(id)=input["workflowId"].as_str().filter(|id|state.store.get("workflow",id).is_some())else{return error(StatusCode::BAD_REQUEST,"워크플로를 찾을 수 없습니다.")};json!({"workflowId":id,"input":input["input"]})
        },
        _=>return error(StatusCode::BAD_REQUEST,"해당 작업은 대화·회의·영상의 실행 API에서 시작하세요."),
    };
    match engine::jobs::enqueue(&state,kind,clean){Some(job)=>(StatusCode::ACCEPTED,Json(job)).into_response(),None=>error(StatusCode::INTERNAL_SERVER_ERROR,"작업을 대기열에 저장할 수 없습니다.")}
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.")};
    if job["kind"]=="rsi_evolve" {
        // Also clean up descendants when a client repeats Stop on a terminal parent.
        engine::jobs::cancel_rsi_children(&state,&id);
        if job["status"]=="cancelled" {return Json(job).into_response();}
    }
    if !matches!(job["status"].as_str(),Some("queued"|"running")){return error(StatusCode::CONFLICT,"진행 중인 작업만 중단할 수 있습니다.");}
    if let Some(session)=job["sessionId"].as_str(){state.cancel_session(session);}
    match state.store.patch_if("job",&id,"status",&job["status"],&json!({"status":"cancelled","cancelRequested":true,"finishedAt":chrono::Utc::now().to_rfc3339()})){
        Some(saved)=>{state.bus.record("job",&saved);Json(saved).into_response()},None=>state.store.get("job",&id).map(|j|Json(j).into_response()).unwrap_or_else(||error(StatusCode::CONFLICT,"작업 상태가 변경되었습니다."))
    }
}


/// Owner steering persists in the same job record; the native Rust cycle
/// reads it before each turn, not through an unrelated model SDK.
pub async fn steer(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    let Some(instruction)=body["instruction"].as_str().map(str::trim)
        .filter(|s|!s.is_empty()&&s.len()<=4000)else{
        return error(StatusCode::BAD_REQUEST,"추가 지시는 1~4000자로 입력하세요.");
    };
    for _ in 0..4 {
        let Some(job)=state.store.get("job",&id)else{
            return error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.");
        };
        if job["kind"]!="prompt" || job["input"]["rsiSourceDir"].is_string(){
            return error(StatusCode::BAD_REQUEST,"일반 자율 실행 작업에서만 추가 지시를 지원합니다.");
        }
        if !matches!(job["status"].as_str(),Some("queued"|"running")){
            return error(StatusCode::CONFLICT,"진행 중인 작업만 수정할 수 있습니다.");
        }
        let mut steering=job["steering"].as_array().cloned().unwrap_or_default();
        if steering.len()>=8 {
            return error(StatusCode::CONFLICT,"작업당 추가 지시는 최대 8회입니다.");
        }
        let revision=job["steeringRevision"].as_u64().unwrap_or(0);
        steering.push(json!({
            "revision":revision+1,"instruction":instruction,
            "at":chrono::Utc::now().to_rfc3339(),
        }));
        if let Some(saved)=state.store.patch_if(
            "job",&id,"steeringRevision",job.get("steeringRevision").unwrap_or(&Value::Null),
            &json!({"steeringRevision":revision+1,"steering":steering}),
        ){
            state.bus.record("job",&saved);
            state.store.audit("job.steer",json!({"jobId":id,"revision":revision+1}));
            return Json(json!({
                "jobId":id,"steeringRevision":revision+1,
                "appliesAt":"next_model_turn"
            })).into_response();
        }
    }
    error(StatusCode::CONFLICT,"작업이 동시에 수정되어 추가 지시를 저장하지 못했습니다.")
}
