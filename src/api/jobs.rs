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
            if let Some(owner)=input["ownerInstructions"].as_str() {
                if owner.len()>32_000 {return error(StatusCode::BAD_REQUEST,"추가 지시가 너무 깁니다.");}
                value["ownerInstructions"]=json!(owner);
            }
            if let Some(items)=input.get("allowedTools") {
                let Some(items)=items.as_array().filter(|items| items.len()<=64&&items.iter().all(|item|
                    item.as_str().is_some_and(|s|!s.is_empty()&&s.len()<=128&&s.chars().all(|c|
                        c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'.')))))
                else{return error(StatusCode::BAD_REQUEST,"허용 도구 목록을 확인하세요.");};
                value["allowedTools"]=json!(items);
            }
            if let Some(checks)=input.get("acceptanceChecks") {
                if let Err(message)=engine::verification::validate_criteria(checks) {
                    return error(StatusCode::BAD_REQUEST,message);
                }
                let valid=checks.as_array().is_some_and(|list|list.iter().all(|item|
                    matches!(item["type"].as_str(),
                        Some("file_exists"|"file_absent"|"file_contains"|"tool_succeeded"))));
                if !valid {return error(StatusCode::BAD_REQUEST,"Rust SDK가 지원하는 검증 조건을 선택하세요.");}
                value["checks"]=checks.clone();
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

/// Accept the owner's mid-task correction at a safe Rust SDK step boundary.
/// The user directive never grants additional tool privileges.
pub async fn directive(
    State(state):State<AppState>, Path(id):Path<String>, Json(body):Json<Value>
)->Response {
    let Some(instruction)=body["instruction"].as_str().map(str::trim)
        .filter(|text|!text.is_empty()&&text.len()<=16_000)
    else {return error(StatusCode::BAD_REQUEST,"작업 변경 지시가 필요합니다.");};
    let Some(job)=state.store.get("job",&id)
    else {return error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.");};
    if job["kind"]!="prompt"||!matches!(job["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"진행 중인 일반 작업에만 새 지시를 전달할 수 있습니다.");
    }
    let Some(updated)=state.store.patch_if("job",&id,"status",&job["status"],&json!({
        "nativeDirective":instruction,"directiveUpdatedAt":chrono::Utc::now().to_rfc3339()
    })) else {return error(StatusCode::CONFLICT,"작업 상태가 변경되었습니다.");};
    state.bus.record("job",&updated);
    Json(json!({"ok":true,"jobId":id,"directive":instruction})).into_response()
}
