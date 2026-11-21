//! API-only Task Teams. No new UI tab and no second runtime.
use crate::{state::AppState,engine};
use crate::features::collaboration::task_team::{self,Team};
use axum::{extract::{Path,State},http::StatusCode,
    response::{IntoResponse,Response},Json};
use serde_json::{json,Value};

fn error(status:StatusCode,message:impl Into<String>)->Response {
    (status,Json(json!({"error":message.into()}))).into_response()
}
pub async fn list(State(state):State<AppState>)->Response{
    match state.store.recent_team_jobs(100) {
        Ok(jobs)=>Json(json!(jobs.iter().map(|job|task_team::project(&state,job))
            .collect::<Vec<_>>())).into_response(),
        Err(_)=>error(StatusCode::INTERNAL_SERVER_ERROR,
            "Task Team history unavailable"),
    }
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
        // CLI bridges and the text-only remote inference gateway are
        // independent opt-ins. A remote worker needs no external Codex/Claude
        // binary and must not accidentally inherit its broader permissions.
        match task.runtime {
            task_team::WorkerRuntime::Native=>{},
            task_team::WorkerRuntime::Remote=>{
                if std::env::var("DOMINION_REMOTE_WORKERS").ok().as_deref()!=Some("1"){
                    return error(StatusCode::BAD_REQUEST,
                        "Remote Team workers require DOMINION_REMOTE_WORKERS=1");
                }
            },
            _=>{
                if std::env::var("DOMINION_EXTERNAL_AGENTS").ok().as_deref()!=Some("1"){
                    return error(StatusCode::BAD_REQUEST,
                        "External CLI Team workers require DOMINION_EXTERNAL_AGENTS=1");
                }
            }
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
    async fn owner_retry_requires_confirmation_and_never_reuses_prior_child_ids(){
        let dir=std::env::temp_dir().join(format!(
            "dominion-team-retry-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"old-team","kind":"team",
            "status":"needs_review","input":{"goal":"Inspect","maxParallel":1,
                "tasks":[{"id":"read","prompt":"Check files","mode":"read"}]}})).unwrap();
        let denied=retry_team(State(state.clone()),Path("old-team".into()),
            Json(json!({"confirm":false}))).await;
        assert_eq!(denied.status(),StatusCode::BAD_REQUEST);
        let accepted=retry_team(State(state.clone()),Path("old-team".into()),
            Json(json!({"confirm":true}))).await;
        assert_eq!(accepted.status(),StatusCode::ACCEPTED);
        let created=state.store.all("job").into_iter()
            .filter(|j|j["kind"]=="team"&&j["id"]!="old-team")
            .collect::<Vec<_>>();
        assert_eq!(created.len(),1);
        assert_eq!(created[0]["retryOf"],"old-team");
        assert_eq!(created[0]["status"],"queued");
        assert_ne!(created[0]["id"],"old-team");
        assert!(state.store.get("job","team-child:old-team:read").is_none());
        crate::test_support::cleanup_dir(dir);
    }
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

/// Version-only detection. Does not verify login, run a model or expose keys.
pub async fn bridges()->Json<Value>{
    Json(crate::integrations::agent_bridge::bridge_preflight().await)
}
/// Explicit owner replay of a failed Team from its immutable instructions.
/// Never forge old receipts or claim unfinished external actions succeeded.
pub async fn retry_team(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    if body["confirm"]!=true {
        return error(StatusCode::BAD_REQUEST,
            "Owner confirmation required: replay starts a new Team and may incur model costs");
    }
    let Some(old)=state.store.get("job",&id)
        .filter(|job|job["kind"]=="team") else{
        return error(StatusCode::NOT_FOUND,"Task Team not found");
    };
    if !matches!(old["status"].as_str(),
        Some("failed"|"interrupted"|"needs_review")){
        return error(StatusCode::CONFLICT,
            "Only failed/interrupted/review-needed Teams can be replayed");
    }
    let team=match Team::parse(&old["input"]){
        Ok(team)=>team,Err(reason)=>return error(StatusCode::CONFLICT,reason),
    };
    // Each worker receives a new parent-scoped ID; no old native receipts or
    // uncertain prior external session are copied into the new run.
    let input=serde_json::to_value(team).expect("validated Team roundtrip");
    let Some(new_job)=engine::jobs::enqueue(&state,"team",input) else{
        return error(StatusCode::INTERNAL_SERVER_ERROR,"Could not enqueue a new Team");
    };
    let Some(new_id)=new_job["id"].as_str() else{
        return error(StatusCode::INTERNAL_SERVER_ERROR,"New Team ID missing");
    };
    let _=state.store.patch_if("job",new_id,"id",&json!(new_id),
        &json!({"retryOf":id,"retryRequestedAt":chrono::Utc::now().to_rfc3339()}));
    state.store.audit("team.owner_retry",json!({
        "previousTeamId":id,"nextTeamId":new_id,"freshRun":true
    }));
    (StatusCode::ACCEPTED,Json(json!({
        "newTeamId":new_id,"retryOf":id,"freshRun":true,
        "automatic":false,"status":"queued",
        "team":task_team::project(&state,&new_job)
    }))).into_response()
}
