//! API-only Task Teams. No new UI tab and no second runtime.
use crate::{state::AppState,engine};
use crate::features::collaboration::task_team::{self,Team};
use axum::{extract::{Path,State},http::StatusCode,
    response::{IntoResponse,Response},Json};
use serde_json::{json,Value};

fn error(status:StatusCode,message:impl Into<String>)->Response {
    (status,Json(json!({"error":message.into()}))).into_response()
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    let mut jobs=state.store.all("job").into_iter()
        .filter(|job|job["kind"]=="team").collect::<Vec<_>>();
    jobs.sort_by(|a,b|b["queuedAt"].as_str().unwrap_or("")
        .cmp(a["queuedAt"].as_str().unwrap_or("")));
    Json(json!(jobs.iter().take(100).map(|job|task_team::project(&state,job))
        .collect::<Vec<_>>()))
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match state.store.get("job",&id).filter(|job|job["kind"]=="team"){
        Some(job)=>Json(task_team::project(&state,&job)).into_response(),
        None=>error(StatusCode::NOT_FOUND,"Task Team not found"),
    }
}
pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let team=match Team::parse(&body){
        Ok(team)=>team,Err(e)=>return error(StatusCode::BAD_REQUEST,e),
    };
    for task in &team.tasks {
        if let Some(id)=task.agent_id.as_deref() {
            if state.store.get("agent",id).is_none(){
                return error(StatusCode::BAD_REQUEST,
                    format!("Unknown Team worker agent: {id}"));
            }
        }
        if let Some(id)=task.provider_id.as_deref(){
            let route=match engine::gateway::resolve(&state,id,task.model.as_deref()){
                Ok(route)=>route,Err(e)=>return error(StatusCode::BAD_REQUEST,e),
            };
            if let Err(reason)=route.capabilities.require_native_tools(){
                return error(StatusCode::BAD_REQUEST,reason);
            }
        }
    }
    let input=serde_json::to_value(&team).expect("validated Team serializes");
    match engine::jobs::enqueue(&state,"team",input){
        Some(job)=>(StatusCode::ACCEPTED,Json(task_team::project(&state,&job))).into_response(),
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,"Could not queue Task Team"),
    }
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id).filter(|job|job["kind"]=="team")
    else{return error(StatusCode::NOT_FOUND,"Task Team not found");};
    if !matches!(job["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"Task Team already stopped");
    }
    match state.store.patch_if("job",&id,"status",&job["status"],&json!({
        "status":"cancelled","cancelRequested":true,
        "finishedAt":chrono::Utc::now().to_rfc3339()})){
        Some(updated)=>{
            state.bus.record("job",&updated);
            state.job_wakeup.notify_waiters();
            Json(task_team::project(&state,&updated)).into_response()
        }
        None=>error(StatusCode::CONFLICT,"Task Team status changed"),
    }
}
