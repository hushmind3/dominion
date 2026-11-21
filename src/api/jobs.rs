use crate::{engine,state::AppState};
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use std::collections::HashMap;
use serde_json::{json,Value};
use super::response::error;
pub async fn list(State(state):State<AppState>,Query(query):Query<HashMap<String,String>>)->Response{
    if let Some(session_id)=query.get("sessionId"){
        if session_id.is_empty()||session_id.len()>256{
            return error(StatusCode::BAD_REQUEST,"작업 세션 ID가 올바르지 않습니다.");
        }
        let limit=query.get("limit").and_then(|v|v.parse::<usize>().ok())
            .unwrap_or(128).clamp(1,256);
        return match state.store.session_job_overview(session_id,limit){
            Ok(rows)=>Json(json!(rows)).into_response(),
            Err(message)=>error(StatusCode::INTERNAL_SERVER_ERROR,&message),
        };
    }
    Json(json!(state.store.all("job"))).into_response()
}
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

/// Owner-initiated continuation uses the original native delegation tool.
/// The old CLI receipt, same owner/agent/runtime and safe read-only worktree
/// are all verified inside delegate_task before a new child is admitted.
pub async fn resume_external(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    let Some(prompt)=body["prompt"].as_str().filter(|s|
        !s.trim().is_empty()&&s.len()<=3000) else{
        return error(StatusCode::BAD_REQUEST,"후속 조사 지시(1~3000바이트)가 필요합니다.");
    };
    let Some(previous)=state.store.get("job",&id) else{
        return error(StatusCode::NOT_FOUND,"기존 외부 조사 작업이 없습니다.");
    };
    if previous["status"]!="completed"
        || previous["input"]["delegationProfile"]!="inspection"
        || previous["result"]["externalRunId"].as_str().is_none(){
        return error(StatusCode::CONFLICT,"성공적으로 완료된 외부 조사 작업만 이어갈 수 있습니다.");
    }
    let Some(external)=previous["input"]["externalRuntime"].as_str().filter(|s|
        matches!(*s,"codex"|"claude"|"qwen"|"codex-app-server"|"qwen-acp")) else{
        return error(StatusCode::CONFLICT,"이어갈 수 있는 외부 실행기가 없습니다.");
    };
    let Some(parent_id)=previous["input"]["parentSessionId"].as_str() else{
        return error(StatusCode::CONFLICT,"원본 부모 세션이 없습니다.");
    };
    let Some(agent_id)=previous["input"]["agentId"].as_str() else{
        return error(StatusCode::CONFLICT,"원본 조사 에이전트가 없습니다.");
    };
    let Some(parent)=state.store.get("session",parent_id) else{
        return error(StatusCode::CONFLICT,"이전 부모 세션을 찾을 수 없습니다.");
    };
    if previous["input"]["toolsEnabled"]!=true
        || previous["input"]["toolsExecutionEnabled"]==false
        || parent["readOnly"]==true
        || parent["toolsEnabled"]==false
        || parent["toolsExecutionEnabled"]==false
        || state.is_cancelled(parent_id)
        || state.store.get("agent",agent_id).is_none(){
        return error(StatusCode::CONFLICT,"현재 소유권·도구 권한에서 이어갈 수 없습니다.");
    }
    let workspace=parent["workspaceDir"].as_str()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(||state.config.workspace.clone());
    let tool_context=crate::engine::tools::ToolContext{
        workspace:&workspace,store:&state.store,state:&state,
        session_id:parent_id,agent_id:parent["agentId"].as_str(),
        tools_enabled:true,tools_execution_enabled:true,
    };
    let args=json!({
        "agentId":agent_id,"prompt":prompt,"wait":false,
        "profile":"inspection","readOnly":true,
        "isolatedWorktree":true,"externalRuntime":external,
        "resumeFromJobId":id
    });
    match crate::engine::tools::execute(
        &tool_context,"delegate_task",&args,"ui-external-inspection-resume"
    ).await{
        Ok(value)=>(StatusCode::ACCEPTED,Json(value)).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message)
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

#[cfg(test)]
mod external_resume_ui_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->AppState {
        let root=std::env::temp_dir().join(format!("dominion-resume-ui-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root};
        AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()))
    }
    #[tokio::test]
    async fn endpoint_rejects_missing_evidence_before_creating_child_job() {
        let state=fixture();
        state.store.put("job",&json!({"id":"prior","status":"completed",
            "input":{"parentSessionId":"parent","agentId":"reviewer",
                "delegationProfile":"inspection","externalRuntime":"qwen-acp",
                "toolsEnabled":true},"result":{}})).unwrap();
        let response=resume_external(State(state.clone()),Path("prior".into()),
            Json(json!({"prompt":"continue inspection"}))).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.all("job").len(),1);
        let invalid=resume_external(State(state.clone()),Path("prior".into()),
            Json(json!({"prompt":"   "}))).await;
        assert_eq!(invalid.status(),StatusCode::BAD_REQUEST);
    }
    #[tokio::test]
    async fn endpoint_rejects_readonly_parent_without_reusing_cli_session() {
        let state=fixture();
        state.store.put("agent",&json!({"id":"reviewer"})).unwrap();
        state.store.put("session",&json!({"id":"parent","readOnly":true})).unwrap();
        state.store.put("job",&json!({"id":"prior","status":"completed",
            "input":{"parentSessionId":"parent","agentId":"reviewer",
                "delegationProfile":"inspection","externalRuntime":"qwen-acp",
                "toolsEnabled":true,"toolsExecutionEnabled":true},
            "result":{"externalRunId":"external-1"}})).unwrap();
        let response=resume_external(State(state.clone()),Path("prior".into()),
            Json(json!({"prompt":"inspect another file"}))).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.all("job").len(),1);
    }
}


#[cfg(test)]
mod scoped_listing_tests {
    use super::*;
    #[tokio::test]
    async fn scoped_listing_retains_team_roots_children_and_legacy_unfiltered_api(){
        let root=std::env::temp_dir().join(format!("dominion-jobs-scoped-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=crate::config::Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        for (id,session,parent) in [("team","owner",None),("child","worker",Some("owner")),
            ("foreign","other",None)] {
            state.store.put("job",&json!({"id":id,"kind":"team",
                "input":{"sessionId":session,"parentSessionId":parent}})).unwrap();
        }
        let scoped=list(State(state.clone()),Query(HashMap::from([
            ("sessionId".into(),"owner".into())
        ]))).await;
        assert_eq!(scoped.status(),StatusCode::OK);
        let body=axum::body::to_bytes(scoped.into_body(),1024*1024).await.unwrap();
        let rows:Value=serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.as_array().unwrap().len(),2);
        assert!(!rows.as_array().unwrap().iter().any(|j|j["id"]=="foreign"));
        let all=list(State(state.clone()),Query(HashMap::new())).await;
        let body=axum::body::to_bytes(all.into_body(),1024*1024).await.unwrap();
        let rows:Value=serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.as_array().unwrap().len(),3);
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
}
