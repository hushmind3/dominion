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
            let options=match engine::job_control::options(&state,input){Ok(v)=>v,Err(e)=>return error(StatusCode::BAD_REQUEST,&e)};
            for (key,item) in options.as_object().unwrap(){value[key]=item.clone();}
            if let Some(id)=input["sessionId"].as_str(){
                if state.store.get("session",id).is_none(){return error(StatusCode::BAD_REQUEST,"작업 세션을 찾을 수 없습니다.");}
                value["sessionId"]=json!(id);
            }
            if let Some(v)=input["thinking"].as_bool(){value["thinking"]=json!(v);}
            if let Some(id)=value["agentId"].as_str(){if state.store.get("agent",id).is_none(){return error(StatusCode::BAD_REQUEST,"실행 에이전트를 찾을 수 없습니다.");}}
            value
        },
        _=>return error(StatusCode::BAD_REQUEST,"해당 작업은 대화·회의의 실행 API에서 시작하세요."),
    };
    match engine::jobs::enqueue(&state,kind,clean){Some(job)=>(StatusCode::ACCEPTED,Json(job)).into_response(),None=>error(StatusCode::INTERNAL_SERVER_ERROR,"작업을 대기열에 저장할 수 없습니다.")}
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.")};
    if !matches!(job["status"].as_str(),Some("queued"|"running")){return error(StatusCode::CONFLICT,"진행 중인 작업만 중단할 수 있습니다.");}
    if let Some(session)=job["sessionId"].as_str(){state.cancel_session(session);}
    match state.store.patch_if("job",&id,"status",&job["status"],&json!({"status":"cancelled","cancelRequested":true,"finishedAt":chrono::Utc::now().to_rfc3339()})){
        Some(saved)=>{state.bus.record("job",&saved);Json(saved).into_response()},None=>state.store.get("job",&id).map(|j|Json(j).into_response()).unwrap_or_else(||error(StatusCode::CONFLICT,"작업 상태가 변경되었습니다."))
    }
}

pub async fn steer(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    steering_response(engine::job_control::steer(&state,"job",&id,body["instruction"].as_str().unwrap_or(""),body["mode"].as_str().unwrap_or("append")))
}
pub(crate) fn steering_response(result:Result<Value,engine::job_control::SteeringError>)->Response{
    use engine::job_control::SteeringError;
    match result{Ok(value)=>Json(value).into_response(),Err(e)=>{
        let code=match &e{SteeringError::BadInput(_)=>StatusCode::BAD_REQUEST,SteeringError::NotFound=>StatusCode::NOT_FOUND,SteeringError::Conflict(_)=>StatusCode::CONFLICT};
        error(code,&e.to_string())
    }}
}
