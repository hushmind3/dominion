use crate::{engine,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
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

pub async fn list(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("job")))}
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
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.")};
    if !matches!(job["status"].as_str(),Some("queued"|"running")){return error(StatusCode::CONFLICT,"진행 중인 작업만 중단할 수 있습니다.");}
    // Claim cancellation durably first. Do not cancel a shared session if
    // another worker already finished this job between lookup and CAS.
    match state.store.patch_if("job",&id,"status",&job["status"],&json!({
        "status":"cancelled","cancelRequested":true,
        "finishedAt":chrono::Utc::now().to_rfc3339()
    })){
        Some(saved)=>{
            if let Some(session)=saved["sessionId"].as_str(){state.cancel_session(session);}
            state.bus.record("job",&saved);
            Json(saved).into_response()
        },
        None=>error(StatusCode::CONFLICT,
            "작업 상태가 변경되어 취소하지 않았습니다. 최신 상태를 다시 확인하세요.")
    }
}

/// Native job steering is revisioned through a single CAS mutation. It
/// never changes tool permissions or replays completed effects. Both existing
/// /directive and the current main /steer route use this authority.
fn apply_steering(state:&AppState,id:&str,instruction:&str,mode:&str)->Response {
    let instruction=instruction.trim();
    if instruction.is_empty()||instruction.len()>16_000 {
        return error(StatusCode::BAD_REQUEST,"작업 변경 지시가 필요합니다.");
    }
    if !matches!(mode,"append"|"replace") {
        return error(StatusCode::BAD_REQUEST,"변경 방식은 append 또는 replace여야 합니다.");
    }
    let Some(job)=state.store.get("job",id)
    else{return error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.");};
    if job["kind"]!="prompt"||!matches!(job["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"진행 중인 일반 작업에만 새 지시를 전달할 수 있습니다.");
    }
    let old=job["nativeDirective"].as_str().unwrap_or("");
    let directive=if mode=="append"&&!old.trim().is_empty() {
        format!("{old}\n\n{instruction}")
    }else{instruction.to_string()};
    if directive.len()>32_000 {
        return error(StatusCode::BAD_REQUEST,"작업 변경 지시가 너무 깁니다.");
    }
    let Some(updated)=state.store.patch_if("job",id,"status",&job["status"],&json!({
        "nativeDirective":directive,"directiveUpdatedAt":chrono::Utc::now().to_rfc3339()
    })) else{return error(StatusCode::CONFLICT,"작업 상태가 변경되었습니다.");};
    state.bus.record("job",&updated);
    Json(json!({"ok":true,"jobId":id,"directive":directive,"mode":mode})).into_response()
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
