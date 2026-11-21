//! Owner UI adapters over native Rust Team and remote artifact execution.
use crate::{engine,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::path::PathBuf;

fn fail(code:StatusCode,message:&str)->Response{
    (code,Json(json!({"error":message}))).into_response()
}
fn owned_team(state:&AppState,id:&str)->Result<Value,String>{
    let team=state.store.get("managed_team",id).ok_or("Managed team not found")?;
    let owner=team["leadSessionId"].as_str().ok_or("Missing Lead owner")?;
    if state.store.get("session",owner).is_none(){
        return Err("Lead session was deleted; cannot control team".into());
    }
    Ok(team)
}
fn workspace_for(state:&AppState,team:&Value)->Result<PathBuf,String>{
    let owner=team["leadSessionId"].as_str().ok_or("Missing Lead session")?;
    let session=state.store.get("session",owner).ok_or("Lead session not found")?;
    match session["workspaceDir"].as_str(){
        Some(path)=>engine::workspace_scope::resolve(state,path),
        None=>Ok(state.config.workspace.clone()),
    }
}
fn tool_context<'a>(
    state:&'a AppState,team:&'a Value,workspace:&'a std::path::Path,
)->Result<engine::tools::ToolContext<'a>,String>{
    let owner=team["leadSessionId"].as_str().ok_or("Missing Lead owner")?;
    Ok(engine::tools::ToolContext{
        workspace,store:&state.store,state,session_id:owner,
        agent_id:team["leadAgentId"].as_str(),
        tools_enabled:true,tools_execution_enabled:true,
    })
}
/// A Lead job is queued, not a fictitious team. Its model creates the DAG
/// through the existing team_workers/team_execute tools with normal approvals.
pub async fn start(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let Some(prompt)=body["prompt"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=4_000)
        else{return fail(StatusCode::BAD_REQUEST,"Describe the task in 1..4000 bytes");};
    let Some(lead_id)=body["leadAgentId"].as_str().filter(|s|!s.is_empty())
        else{return fail(StatusCode::BAD_REQUEST,"Lead agent required");};
    let Some(lead)=state.store.get("agent",lead_id)else{
        return fail(StatusCode::BAD_REQUEST,"Lead agent not registered");
    };
    if lead["enabled"]==false ||lead["disabled"]==true
        ||lead["providerId"].as_str().is_none_or(str::is_empty)
        ||lead["model"].as_str().is_none_or(str::is_empty){
        return fail(StatusCode::BAD_REQUEST,"Lead has no runnable model or is disabled");
    }
    let Some(selected)=body["workerAgentIds"].as_array().filter(|a|!a.is_empty()&&a.len()<=8)
        else{return fail(StatusCode::BAD_REQUEST,"Select 1..8 worker agents");};
    let remote_id=body["remoteWorkerId"].as_str().filter(|s|!s.trim().is_empty());
    let advertised=remote_id.and_then(|remote_id|state.store.get("remote_worker",remote_id))
        .and_then(|node|node["agents"].as_array().cloned()).unwrap_or_default();
    let mut worker_ids=Vec::<String>::new();
    for entry in selected{
        let Some(id)=entry.as_str().filter(|id|!id.is_empty()&&id.len()<=80
            && id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'))
            else{return fail(StatusCode::BAD_REQUEST,"Invalid Worker agent ID");};
        if worker_ids.iter().any(|saved|saved==id){
            return fail(StatusCode::BAD_REQUEST,"Worker selected twice");
        }
        if let Some(agent)=state.store.get("agent",id){
            if agent["enabled"]==false ||agent["disabled"]==true{
                return fail(StatusCode::BAD_REQUEST,"Disabled Worker agent");
            }
        }else if !advertised.iter().any(|advertised|advertised.as_str()==Some(id)){
            // A remote-only Agent belongs to the other PC's local catalog,
            // not to the Lead's database. Never accept an arbitrary ID.
            return fail(StatusCode::BAD_REQUEST,"Worker is neither local nor advertised by the selected remote device");
        }
        worker_ids.push(id.to_string());
    }
    if let Some(remote_id)=remote_id{
        let eligible=worker_ids.iter().filter(|id|
            advertised.iter().any(|agent|agent.as_str()==Some(id.as_str())))
            .collect::<Vec<_>>();
        if eligible.is_empty(){
            return fail(StatusCode::CONFLICT,"No selected Agent is advertised by this remote Worker");
        }
        if let Err(reason)=crate::remote::worker_available(&state,remote_id,eligible[0],true){
            return fail(StatusCode::CONFLICT,&reason);
        }
    }
    let roster=worker_ids.iter().map(|s|format!("- {s}")).collect::<Vec<_>>().join("\n");
    let remote_hint=remote_id.map_or(String::new(),|id|format!(
        "Selected remote Worker {id} can execute only Agents advertised by that node. Assign at least one eligible task with remoteWorkerId={id}; unadvertised Agents remain local. Prefer remote readOnly research and a local verified Writer unless remote writes were explicitly enabled."));
    let instruction=format!(
        "OWNER REQUEST:\n{prompt}\n\nRun the DOMINION native managed team. \
        First call team_workers and inspect real Agent IDs. Then call team_execute \
        with a bounded DAG, dependencies, independent read-only research and \
        a serialized verified Writer if required. Report real teamId and evidence; \
        never claim completion without native receipts.\n\nSELECTED WORKERS:\n{roster}\n{remote_hint}"
    );
    let input=json!({"prompt":instruction,"agentId":lead_id,
        "providerId":lead["providerId"],"model":lead["model"],
        "toolsEnabled":true,"toolsExecutionEnabled":true,
        "autonomous":true,"maxAutonomousAttempts":2,"uiSource":"team_activity",
        "teamUiWorkerIds":worker_ids,"teamUiRemoteWorkerId":remote_id});
    match engine::jobs::enqueue(&state,"prompt",input){
        Some(job)=>(StatusCode::ACCEPTED,Json(json!({"status":"lead_queued",
            "jobId":job["id"],"note":"Native Lead job queued; teamId is assigned only after team_execute"}))).into_response(),
        None=>fail(StatusCode::INTERNAL_SERVER_ERROR,"Unable to enqueue Lead"),
    }
}
/// Persist team cancellation before attempting child/remote job cancellation.
/// A launching child is uncertain, not falsely reported as safely stopped.
pub async fn stop(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let team=match owned_team(&state,&id){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::NOT_FOUND,&e),
    };
    if team["status"]!="running"{
        return fail(StatusCode::CONFLICT,"Team is not running");
    }
    let Some(saved)=state.store.patch_if("managed_team",&id,"status",&json!("running"),
        &json!({"status":"cancelled","cancelRequested":true,
            "cancelledAt":chrono::Utc::now().to_rfc3339()}))else{
        return fail(StatusCode::CONFLICT,"Team status changed; refresh and retry");
    };
    state.bus.record("managed_team",&saved);
    crate::remote::cancel_team(&state,&id);
    for task in state.store.all("managed_team_task").into_iter().filter(|t|t["teamId"]==id){
        let Some(task_id)=task["id"].as_str()else{continue;};
        let prev=task["status"].as_str().unwrap_or("");
        let update=match prev{
            "assigned"=>Some(json!({"status":"cancelled","error":"Lead stopped team"})),
            "launching"=>Some(json!({"status":"needs_review",
                "error":"Stopped during launch; inspect any native effects"})),
            _=>None,
        };
        if let Some(update)=update{
            if let Some(record)=state.store.patch_if("managed_team_task",task_id,
                "status",&json!(prev),&update){
                state.bus.record("managed_team_task",&record);
            }
        }
        if let Some(job_id)=task["jobId"].as_str(){
            let _=engine::jobs::cancel_job(&state,job_id);
        }
    }
    Json(json!({"teamId":id,"status":"cancelled",
        "note":"Review any in-flight effects before reusing files"})).into_response()
}
/// Resume the exact persisted DAG through the same tool gateway; never
/// create a second team or replay ambiguous Writer outcomes.
pub async fn resume(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let team=match owned_team(&state,&id){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::NOT_FOUND,&e),
    };
    if team["status"]!="running" ||team["cancelRequested"]==true{
        return fail(StatusCode::CONFLICT,"Stopped or finalized team cannot resume");
    }
    let session_id=team["leadSessionId"].as_str().unwrap_or("");
    // Claim the actual Lead turn slot. A pending old turn cannot race a
    // user-initiated resume or leave effects running in the same session.
    if !state.begin_session(session_id){
        return fail(StatusCode::CONFLICT,"Lead turn is still active; inspect it before resuming");
    }
    let _session_guard=state.session_guard(session_id);
    // A previously stopped turn leaves a durable cancellation tombstone.
    // Re-arm only the latest generation, after acquiring the session slot.
    // Old provider/tool work remains fenced by the generation mismatch.
    if state.is_cancelled(session_id){
        let generation=state.cancellation_generation(session_id);
        if !state.resume_session_if_generation(session_id,generation){
            return fail(StatusCode::CONFLICT,"Lead cancellation changed; reload and inspect");
        }
    }
    let workspace=match workspace_for(&state,&team){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    let ctx=match tool_context(&state,&team,&workspace){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    let run_id=format!("owner-team-resume-{}",uuid::Uuid::new_v4());
    match engine::tools::execute(&ctx,"team_execute",
        &json!({"teamId":id,"wait":false}),&run_id).await{
        Ok(value)=>Json(value).into_response(),
        Err(e)=>fail(StatusCode::CONFLICT,&e),
    }
}
pub async fn preview(
    State(state):State<AppState>,Path((team_id,task_id)):Path<(String,String)>,
)->Response{
    let team=match owned_team(&state,&team_id){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::NOT_FOUND,&e),
    };
    let workspace=match workspace_for(&state,&team){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    let ctx=match tool_context(&state,&team,&workspace){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    match engine::tools::review_remote_artifacts(&ctx,&team_id,&task_id){
        Ok(value)=>Json(value).into_response(),Err(e)=>fail(StatusCode::CONFLICT,&e),
    }
}
pub async fn approve_apply(
    State(state):State<AppState>,Path((team_id,task_id)):Path<(String,String)>,
    Json(body):Json<Value>,
)->Response{
    if body["approved"]!=true{
        return fail(StatusCode::BAD_REQUEST,"Explicit approval required");
    }
    let Some(digest)=body["reviewDigest"].as_str()else{
        return fail(StatusCode::BAD_REQUEST,"Missing reviewed diff SHA-256");
    };
    let team=match owned_team(&state,&team_id){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::NOT_FOUND,&e),
    };
    if team["status"]=="cancelled" ||team["cancelRequested"]==true{
        return fail(StatusCode::CONFLICT,"Cancelled teams cannot apply artifacts");
    }
    let workspace=match workspace_for(&state,&team){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    let ctx=match tool_context(&state,&team,&workspace){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    match engine::tools::apply_operator_reviewed_remote(&ctx,&team_id,&task_id,digest).await{
        Ok(value)=>{
            state.store.audit("remote.artifacts_approved",
                json!({"teamId":team_id,"remoteTaskId":task_id,"reviewDigest":digest}));
            Json(value).into_response()
        },
        Err(e)=>fail(StatusCode::CONFLICT,&e),
    }
}
pub async fn reject(
    State(state):State<AppState>,Path((team_id,task_id)):Path<(String,String)>,
    Json(body):Json<Value>,
)->Response{
    if body["rejected"]!=true{
        return fail(StatusCode::BAD_REQUEST,"Explicit rejection required");
    }
    let team=match owned_team(&state,&team_id){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::NOT_FOUND,&e),
    };
    let workspace=match workspace_for(&state,&team){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    let ctx=match tool_context(&state,&team,&workspace){
        Ok(v)=>v,Err(e)=>return fail(StatusCode::CONFLICT,&e),
    };
    match engine::tools::reject_remote_artifacts(&ctx,&team_id,&task_id){
        Ok(value)=>{
            state.store.audit("remote.artifacts_rejected",
                json!({"teamId":team_id,"remoteTaskId":task_id}));
            Json(value).into_response()
        },
        Err(e)=>fail(StatusCode::CONFLICT,&e),
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-team-ui-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,workspace:dir.clone(),
            web_dir:dir.join("web"),data_dir:dir.join("data")};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        (state,dir)
    }
    #[tokio::test]
    async fn real_axum_router_exposes_native_team_and_review_controls(){
        let (mut state,dir)=fixture();
        state.store.put("agent",&json!({"id":"lead","name":"Lead",
            "providerId":"configured","model":"running-model","enabled":true})).unwrap();
        state.store.put("agent",&json!({"id":"worker","name":"Worker",
            "providerId":"configured","model":"running-model","enabled":true})).unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        // The real listener picked a random port. The Host guard must compare
        // against that bound port, not the fixture's pre-bind placeholder 0.
        state.config.port=address.port();
        let app=crate::api::router(state.clone());
        let server=tokio::spawn(async move{
            axum::serve(listener,app).await.unwrap();
        });
        let client=reqwest::Client::new();
        let endpoint=format!("http://{address}/api/activity/teams");
        let overview=client.get(&endpoint).send().await.unwrap();
        assert_eq!(overview.status(),reqwest::StatusCode::OK);
        let payload=overview.json::<Value>().await.unwrap();
        assert_eq!(payload["teams"].as_array().unwrap().len(),0);
        let started=client.post(format!("{endpoint}/start")).json(&json!({
            "prompt":"Inspect all fixtures","leadAgentId":"lead",
            "workerAgentIds":["worker"]
        })).send().await.unwrap();
        assert_eq!(started.status(),reqwest::StatusCode::ACCEPTED);
        let job_id=started.json::<Value>().await.unwrap()["jobId"].as_str()
            .unwrap().to_string();
        assert_eq!(state.store.get("job",&job_id).unwrap()
            ["input"]["teamUiWorkerIds"],json!(["worker"]));
        assert_eq!(client.post(format!("{endpoint}/missing/stop"))
            .send().await.unwrap().status(),reqwest::StatusCode::NOT_FOUND);
        assert_eq!(client.get(format!("{endpoint}/missing/artifacts/t-1"))
            .send().await.unwrap().status(),reqwest::StatusCode::NOT_FOUND);
        assert_eq!(client.post(format!("{endpoint}/missing/artifacts/t-1/apply"))
            .json(&json!({"approved":false})).send().await.unwrap().status(),
            reqwest::StatusCode::BAD_REQUEST);
        server.abort();drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn lead_launch_preserves_selected_workers_in_real_native_job(){
        let (state,dir)=fixture();
        for id in ["lead","worker"]{
            state.store.put("agent",&json!({"id":id,"name":id,
                "providerId":"provider-1","model":"model-1","enabled":true})).unwrap();
        }
        let result=start(State(state.clone()),Json(json!({
            "prompt":"Inspect the repository and report test failures",
            "leadAgentId":"lead","workerAgentIds":["worker"]
        }))).await;
        assert_eq!(result.status(),StatusCode::ACCEPTED);
        let jobs=state.store.all("job");
        assert_eq!(jobs.len(),1);
        assert_eq!(jobs[0]["kind"],"prompt");
        assert_eq!(jobs[0]["input"]["uiSource"],"team_activity");
        assert_eq!(jobs[0]["input"]["teamUiWorkerIds"],json!(["worker"]));
        assert_eq!(state.store.all("managed_team").len(),0);
        let bad=start(State(state.clone()),Json(json!({
            "prompt":"Inspect repository","leadAgentId":"lead",
            "workerAgentIds":["unknown-agent"]
        }))).await;
        assert_eq!(bad.status(),StatusCode::BAD_REQUEST);
        assert_eq!(state.store.all("job").len(),1);
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn stopping_one_team_is_durable_and_does_not_stop_other_team(){
        let (state,dir)=fixture();
        state.store.put("session",&json!({"id":"lead-session"})).unwrap();
        for id in ["team-a","team-b"]{
            state.store.put("managed_team",&json!({"id":id,"status":"running",
                "leadSessionId":"lead-session","tasks":[]})).unwrap();
        }
        state.store.put("managed_team_task",&json!({"id":"member-a","teamId":"team-a",
            "status":"assigned"})).unwrap();
        assert_eq!(stop(State(state.clone()),Path("team-a".into())).await.status(),StatusCode::OK);
        assert_eq!(state.store.get("managed_team","team-a").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("managed_team_task","member-a").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("managed_team","team-b").unwrap()["status"],"running");
        assert_eq!(resume(State(state.clone()),Path("team-a".into())).await.status(),StatusCode::CONFLICT);
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn resume_requires_drained_lead_and_rearms_only_its_epoch(){
        let (state,dir)=fixture();
        state.store.put("session",&json!({"id":"lead-session"})).unwrap();
        state.store.put("managed_team",&json!({"id":"team-r","status":"running",
            "leadSessionId":"lead-session","tasks":[],"waves":[]})).unwrap();
        assert!(state.begin_session("lead-session"));
        state.cancel_session("lead-session");
        assert_eq!(resume(State(state.clone()),Path("team-r".into())).await.status(),
            StatusCode::CONFLICT);
        assert!(state.is_cancelled("lead-session"));
        state.end_session("lead-session");
        assert_eq!(resume(State(state.clone()),Path("team-r".into())).await.status(),
            StatusCode::OK);
        assert!(!state.is_cancelled("lead-session"));
        assert!(!state.active.lock().unwrap().contains("lead-session"));
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn owner_file_actions_require_explicit_decisions(){
        let (state,dir)=fixture();
        state.store.put("session",&json!({"id":"lead-session"})).unwrap();
        state.store.put("managed_team",&json!({"id":"team-1","status":"running",
            "leadSessionId":"lead-session"})).unwrap();
        assert_eq!(approve_apply(State(state.clone()),Path(("team-1".into(),"task-1".into())),
            Json(json!({"approved":false}))).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(reject(State(state.clone()),Path(("team-1".into(),"task-1".into())),
            Json(json!({"rejected":false}))).await.status(),StatusCode::BAD_REQUEST);
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
}
