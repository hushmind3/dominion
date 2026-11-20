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
        if task.runtime!=task_team::WorkerRuntime::Native
            && std::env::var("DOMINION_EXTERNAL_AGENTS").ok().as_deref()!=Some("1"){
            return error(StatusCode::BAD_REQUEST,
                "External Team workers require explicit DOMINION_EXTERNAL_AGENTS=1 opt-in");
        }
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
            // The endpoint must fence child jobs immediately. Previously only
            // the team's 350ms supervisor poll performed this cascade, which
            // left live workers able to start another model/tool operation.
            if let Ok(team)=Team::parse(&updated["input"]) {
                task_team::cancel_children(&state,&id,&team.tasks);
            }else{
                state.store.audit("team.cancel_invalid_contract",
                    json!({"teamId":id}));
            }
            state.bus.record("job",&updated);
            state.job_wakeup.notify_waiters();
            Json(task_team::project(&state,&updated)).into_response()
        }
        None=>error(StatusCode::CONFLICT,"Task Team status changed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[tokio::test]
    async fn api_cancel_fences_child_before_supervisor_wakes() {
        let root=std::env::temp_dir().join(format!(
            "dominion-team-cancel-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),workspace:root.clone(),
            web_dir:root.join("web")};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let team=json!({"goal":"Inspect","maxParallel":1,
            "tasks":[{"id":"inspect","prompt":"Read files","mode":"read",
                "runtime":"native"}]});
        state.store.put("job",&json!({"id":"parent","kind":"team",
            "status":"running","input":team})).unwrap();
        state.store.put("job",&json!({"id":"team-child:parent:inspect",
            "kind":"prompt","status":"running","attempts":1,
            "input":{"parentRunId":"parent","teamTaskId":"inspect",
                "teamMode":"read","bridgeRuntime":"native"}})).unwrap();
        let response=cancel(State(state.clone()),Path("parent".into())).await;
        assert_eq!(response.status(),StatusCode::OK);
        let parent=state.store.get("job","parent").unwrap();
        assert_eq!(parent["status"],"cancelled");
        let child=state.store.get("job","team-child:parent:inspect").unwrap();
        assert_eq!(child["status"],"cancelled");
        assert_eq!(child["cancelRequested"],true);
        crate::test_support::cleanup_dir(&root);
    }
}

pub async fn worktree_preview(
    State(state):State<AppState>,Path((id,task)):Path<(String,String)>
)->Response{
    match crate::features::collaboration::worktree::preview(&state,&id,&task).await{
        Ok(value)=>Json(value).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,reason),
    }
}
pub async fn worktree_promote(
    State(state):State<AppState>,
    Path((id,task)):Path<(String,String)>,Json(body):Json<Value>
)->Response{
    if body["confirm"]!=true {
        return error(StatusCode::BAD_REQUEST,
            "Explicit owner confirmation (confirm=true) is required");
    }
    let Some(digest)=body["expectedDiffSha256"].as_str() else {
        return error(StatusCode::BAD_REQUEST,"Preview digest is required");
    };
    match crate::features::collaboration::worktree::promote(&state,&id,&task,digest).await{
        Ok(value)=>Json(value).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,reason),
    }
}
