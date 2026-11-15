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
            if let Some(raw)=input.get("workspaceDir"){
                let Some(path)=raw.as_str() else {
                    return error(StatusCode::BAD_REQUEST,"작업 폴더는 절대경로 문자열이어야 합니다.");
                };
                let selected=match engine::workspace_scope::resolve(&state,path){
                    Ok(path)=>path,
                    Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
                };
                value["workspaceDir"]=json!(selected.to_string_lossy().to_string());
            }
            if let Some(command)=input.get("verifyCommand"){
                let Some(command)=command.as_str().filter(|command|command.len()<=2048)else{
                    return error(StatusCode::BAD_REQUEST,"검증 명령은 최대 2048자 문자열입니다.");
                };
                if !command.trim().is_empty(){value["verifyCommand"]=json!(command.trim());}
            }
            if let Some(raw)=input.get("tokenBudget"){
                let Some(budget)=raw.as_u64().filter(|v|(1024..=2_000_000).contains(v)) else{
                    return error(StatusCode::BAD_REQUEST,
                        "작업 토큰 예산은 1024~2000000 사이의 정수여야 합니다.");
                };
                value["tokenBudget"]=json!(budget);
            }
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


/// HTTP is only an adapter to native Rust steering and authorization.
pub async fn steer(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    let mode=match body.get("mode"){
        None=>"append",
        Some(Value::String(mode)) if matches!(mode.as_str(),"append"|"replace")=>mode.as_str(),
        _=>return error(StatusCode::BAD_REQUEST,"지시 방식은 append 또는 replace입니다."),
    };
    match engine::jobs::steer_with_mode(
        &state,&id,body["instruction"].as_str().unwrap_or(""),mode
    ){
        Ok(record)=>Json(record).into_response(),
        Err(engine::jobs::SteeringError::BadInput(message))=>
            error(StatusCode::BAD_REQUEST,&message),
        Err(engine::jobs::SteeringError::NotFound)=>
            error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다."),
        Err(engine::jobs::SteeringError::Conflict(message))=>
            error(StatusCode::CONFLICT,&message),
    }
}
