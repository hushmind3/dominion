use crate::{engine,state::AppState};
use std::collections::HashMap;
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;
/// An API request describes what must be true; only the Rust kernel may
/// produce acceptance receipts. Clients cannot submit pre-verified results.
fn native_acceptance_checks(items:&[Value])->Result<Vec<Value>,String> {
    items.iter().map(|item|match item["type"].as_str() {
        Some("file_exists"|"file_absent"|"file_contains"|"tool_succeeded") =>
            Ok(item.clone()),
        Some("command_succeeded") if item["command"].as_str()
            .is_some_and(|text|!text.trim().is_empty()&&text.len()<=4096) =>
            Ok(json!({"type":"tool_succeeded","tool":"shell_exec",
                "contains":item["command"]})),
        _=>Err("Rust SDK가 지원하는 검증 조건을 선택하세요.".into()),
    }).collect()
}

pub async fn list(State(state):State<AppState>,Query(query):Query<HashMap<String,String>>)->Json<Value>{
    let limit=query.get("limit").and_then(|v|v.parse::<usize>().ok()).unwrap_or(200).min(500);
    let session=query.get("sessionId").filter(|v|!v.is_empty());
    let mut jobs=state.store.all("job");
    if let Some(session)=session{
        jobs.retain(|job|{
            job["sessionId"].as_str()==Some(session)
                ||job.pointer("/input/sessionId").and_then(Value::as_str)==Some(session)
        });
    }
    if jobs.len()>limit{jobs=jobs.split_off(jobs.len()-limit);}
    Json(json!(jobs))
}

/// Worker jobs spawned by a team lead, keyed by the parent team job.
pub async fn team_steps(State(state):State<AppState>,Path(id):Path<String>)->Response{
    use crate::features::collaboration::task_team;
    let Some(parent)=state.store.get("job",&id).filter(|job|job["kind"]=="team") else{
        return error(StatusCode::NOT_FOUND,"팀 작업을 찾을 수 없습니다.");
    };
    let team=match task_team::Team::parse(&parent["input"]){
        Ok(team)=>team,Err(message)=>return error(StatusCode::CONFLICT,&message),
    };
    let mut jobs=Vec::new();
    let mut missing=Vec::new();
    for task in &team.tasks {
        let child_id=task_team::child_id(&id,&task.id);
        if let Some(job)=state.store.get("job",&child_id){
            if task_team::belongs_to_team(&id,task,&job){jobs.push(job);}
            else{missing.push(child_id);}
        }
    }
    let projection=task_team::project(&state,&parent);
    Json(json!({"teamId":id,"jobs":jobs,"missing":missing,"tasks":projection["tasks"],
        "status":projection["status"]})).into_response()
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{match state.store.get("job",&id){Some(j)=>Json(j).into_response(),None=>error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.")}}
/// Client requests describe work, never mutable runtime status or evidence.
pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let kind=body["kind"].as_str().unwrap_or("prompt");let input=&body["input"];
    let clean=match kind{
        "prompt"=>{
            let Some(prompt)=input["prompt"].as_str().filter(|p|!p.trim().is_empty()&&p.len()<=64000)else{return error(StatusCode::BAD_REQUEST,"실행할 작업 지시가 필요합니다.")};
            let mut value=json!({"prompt":prompt,"toolsEnabled":input["toolsEnabled"].as_bool().unwrap_or(true)});
            // Used only to scope the main UI's autonomous-work history.
            // It grants no tool, model or approval permission.
            if input["clientSurface"]=="native_agent_panel" {
                value["clientSurface"]=json!("native_agent_panel");
                // Compatibility surface is presentation-only. It cannot
                // grant tool authority or create a second execution kernel.
                if input["harnessCompat"]==true {
                    value["harnessCompat"]=json!(true);
                }
            }
            for key in ["agentId","providerId","model","reasoningEffort"]{if let Some(v)=input[key].as_str().filter(|s|!s.is_empty()){value[key]=json!(v);}}
            if let Some(v)=input["thinking"].as_bool(){value["thinking"]=json!(v);}
            if let Some(v)=input["toolsExecutionEnabled"].as_bool(){value["toolsExecutionEnabled"]=json!(v);}
            // Rust SDK owns the agent loop, tools and evidence. Client may set
            // bounded policies but never mutable execution states or receipts.
            let max_steps = input["maxSteps"].as_u64().unwrap_or(24).clamp(1,200);
            let attempts = input["maxAttempts"].as_u64().unwrap_or(3).clamp(1,6);
            let max_calls = input["maxModelCalls"].as_u64()
                .unwrap_or(max_steps.saturating_mul(attempts)).clamp(1,250);
            value["limits"] = json!({
                "maxSteps":max_steps,
                "maxModelCalls":max_calls,
                "maxToolCalls":input["maxToolCalls"].as_u64().unwrap_or(max_steps.saturating_mul(4)).clamp(1,4096),
                "maxEstimatedTokens":input["tokenBudget"].as_u64().unwrap_or(100_000).clamp(1,20_000_000),
                "maxParallelReads":input["parallelReads"].as_u64().unwrap_or(4).clamp(1,16),
                "maxResultChars":input["maxResultChars"].as_u64().unwrap_or(3000).clamp(128,16384)
            });
            if let Some(strategy)=input["strategy"].as_str() {
                if !matches!(strategy,"balanced"|"verify_first"|"low_cost") {
                    return error(StatusCode::BAD_REQUEST,"지원하지 않는 실행 전략입니다.");
                }
                value["strategy"]=json!(strategy);
            }
            // Job-local intelligence profiles: no mutable global engine mode.
            if let Some(raw)=input.get("algorithmId") {
                let Some(id)=raw.as_str() else {
                    return error(StatusCode::BAD_REQUEST,"알고리즘 ID는 문자열이어야 합니다.");
                };
                if !crate::features::cognition::profiles::supported(id) {
                    return error(StatusCode::BAD_REQUEST,"지원하지 않는 에이전트 알고리즘입니다.");
                }
                value["algorithmId"]=json!(id);
            }
            if let Some(owner)=input["ownerInstructions"].as_str() {
                if owner.len()>32_000 {return error(StatusCode::BAD_REQUEST,"추가 지시가 너무 깁니다.");}
                value["ownerInstructions"]=json!(owner);
            }
            if let Some(items)=input.get("allowedTools") {
                let Some(items)=items.as_array().filter(|items| items.len()<=1024&&items.iter().all(|item|
                    item.as_str().is_some_and(|s|!s.is_empty()&&s.len()<=128&&s.chars().all(|c|
                        c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'.')))))
                else{return error(StatusCode::BAD_REQUEST,"허용 도구 목록을 확인하세요.");};
                value["allowedTools"]=json!(items);
            }
            if let Some(checks)=input.get("acceptanceChecks") {
                if let Err(message)=engine::verification::validate_criteria(checks) {
                    return error(StatusCode::BAD_REQUEST,&message);
                }
                let Some(items)=checks.as_array() else {
                    return error(StatusCode::BAD_REQUEST,"검증 조건은 배열이어야 합니다.");
                };
                match native_acceptance_checks(items) {
                    Ok(native)=>value["checks"]=json!(native),
                    Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
                }
            }
            if let Some(mode)=input["routingMode"].as_str() {
                if !matches!(mode,"default"|"quality_cost") {
                    return error(StatusCode::BAD_REQUEST,"지원하지 않는 모델 라우팅 정책입니다.");
                }
                let min_quality=input["minQuality"].as_f64().unwrap_or(0.80);
                let max_drop=input["maxQualityDrop"].as_f64().unwrap_or(0.10);
                if !min_quality.is_finite()||!(0.0..=1.0).contains(&min_quality)
                    ||!max_drop.is_finite()||!(0.0..=0.50).contains(&max_drop){
                    return error(StatusCode::BAD_REQUEST,"모델 품질 하한·허용 품질 차이를 확인하세요.");
                }
                value["nativeRoutingPolicy"]=json!({"mode":mode,
                    "minQuality":min_quality,"maxQualityDrop":max_drop});
            }
            if let Some(v)=input["allowFallback"].as_bool() {
                value["allowFallback"]=json!(v);
            }
            if let Some(v)=input["requireEvidence"].as_bool() {
                value["requireEvidence"]=json!(v);
            }
            // A private transcript can be reused, but a room or external
            // channel cannot be silently rebound to a different worker.
            if let Some(session_id)=input["sessionId"].as_str().filter(|s|!s.is_empty()) {
                let Some(session)=state.store.get("session",session_id)
                else {return error(StatusCode::BAD_REQUEST,"연결할 세션을 찾을 수 없습니다.");};
                if session["roomId"].is_string()||session["channelContext"].is_object(){
                    return error(StatusCode::BAD_REQUEST,"그룹·외부 채널 세션은 해당 화면에서 실행하세요.");
                }
                value["sessionId"]=json!(session_id);
            }
            if let Some(id)=value["agentId"].as_str(){if state.store.get("agent",id).is_none(){return error(StatusCode::BAD_REQUEST,"실행 에이전트를 찾을 수 없습니다.");}}
            value
        },
        _=>return error(StatusCode::BAD_REQUEST,"해당 작업은 대화·회의·영상의 실행 API에서 시작하세요."),
    };
    match engine::jobs::enqueue(&state,kind,clean){Some(job)=>(StatusCode::ACCEPTED,Json(job)).into_response(),None=>error(StatusCode::INTERNAL_SERVER_ERROR,"작업을 대기열에 저장할 수 없습니다.")}
}
fn control_response(result:Result<Value,crate::host::execution::job_control::ControlError>)->Response{
    use crate::host::execution::job_control::ControlError;
    match result{
        Ok(saved)=>Json(saved).into_response(),
        Err(ControlError::Invalid(message))=>error(StatusCode::BAD_REQUEST,&message),
        Err(ControlError::Missing)=>error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다."),
        Err(ControlError::Changed(message))=>error(StatusCode::CONFLICT,&message),
    }
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    control_response(crate::host::execution::job_control::cancel(&state,&id))
}
fn apply_steering(state:&AppState,id:&str,instruction:&str,mode:&str)->Response{
    control_response(crate::host::execution::job_control::steer(state,id,instruction,mode))
}
pub async fn steer(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    apply_steering(&state,&id,body["instruction"].as_str().unwrap_or(""),
        body["mode"].as_str().unwrap_or("append"))
}
/// Legacy native job directive endpoint: replacement remains its default.
pub async fn directive(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response {
    apply_steering(&state,&id,body["instruction"].as_str().unwrap_or(""),
        body["mode"].as_str().unwrap_or("replace"))
}

#[cfg(test)]
mod native_acceptance_tests {
    use super::*;
    #[test]
    fn a_required_command_maps_to_exact_rust_kernel_postcondition() {
        let criteria=native_acceptance_checks(&[json!({
            "type":"command_succeeded","command":"cargo test --workspace"
        })]).unwrap();
        assert_eq!(criteria,vec![json!({
            "type":"tool_succeeded","tool":"shell_exec",
            "contains":"cargo test --workspace"
        })]);
        assert!(native_acceptance_checks(&[json!({
            "type":"command_succeeded","command":"   "
        })]).is_err());
    }
    #[test]
    fn caller_cannot_submit_a_fake_passed_acceptance_result() {
        let criteria=native_acceptance_checks(&[json!({
            "type":"command_succeeded","command":"cargo test",
            "satisfied":true,"evidence":"forged","receipt":"fake"
        })]).unwrap();
        assert_eq!(criteria[0],json!({"type":"tool_succeeded",
            "tool":"shell_exec","contains":"cargo test"}));
    }
}

#[cfg(test)]
mod algorithm_profile_api_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!("dominion-profile-api-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        (state,root)
    }
    #[tokio::test]
    async fn job_api_rejects_unknown_and_nonstring_profiles_before_queueing(){
        let (state,root)=state();
        for id in [json!("uninstalled"),json!(42),json!(null)]{
            let response=create(State(state.clone()),Json(json!({"kind":"prompt",
                "input":{"prompt":"summarize","algorithmId":id}}))).await;
            assert_eq!(response.status(),StatusCode::BAD_REQUEST);
        }
        assert!(state.store.all("job").is_empty());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn focused_and_legacy_jobs_keep_exact_saved_input_contract(){
        let (state,root)=state();
        let selected=create(State(state.clone()),Json(json!({"kind":"prompt",
            "input":{"prompt":"summarize","algorithmId":"focused-v1"}}))).await;
        assert_eq!(selected.status(),StatusCode::ACCEPTED);
        let default=create(State(state.clone()),Json(json!({"kind":"prompt",
            "input":{"prompt":"summarize"}}))).await;
        assert_eq!(default.status(),StatusCode::ACCEPTED);
        let jobs=state.store.all("job");
        assert_eq!(jobs.len(),2);
        assert!(jobs.iter().any(|job|job["input"]["algorithmId"]=="focused-v1"));
        assert!(jobs.iter().any(|job|job["input"]["algorithmId"].is_null()));
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
}
