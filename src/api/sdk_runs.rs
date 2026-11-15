//! Compatibility API for the frontend pinned to main commit 879dc23d.
//! Delegates to the existing Rust jobs and approval engines: no second harness.
use crate::state::AppState;
use axum::{body::to_bytes,extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::{approvals,jobs,response::error};

fn prompt_job(state:&AppState,id:&str)->Option<Value>{
    state.store.get("job",id).filter(|j|j["kind"]=="prompt")
}
fn pending(state:&AppState,job:&Value)->Option<Value>{
    let session=job["sessionId"].as_str()?;
    if job["status"]!="running" {return None;}
    state.store.all("approval").into_iter().find(|a|
        a["sessionId"].as_str()==Some(session)
        && a["status"]=="pending"
        && a["runId"]==job["id"])
}
fn project(state:&AppState,job:&Value)->Value{
    let snapshot=&job["nativeState"];
    let result=&job["result"];
    let awaiting=pending(state,job);
    let status=if awaiting.is_some(){"awaiting_approval"}else {
        match job["status"].as_str().unwrap_or("queued"){
            "needs_review"=>"in_doubt",
            "queued"=>"queued","running"=>"running",
            "completed"=>"completed","cancelled"=>"cancelled",
            "failed"=>"failed","interrupted"=>"interrupted",
            other=>other
        }
    };
    let m=&job["nativeModelUsage"];
    let usage=if m["reportedCalls"].as_u64().unwrap_or(0)>0 {
        json!({"prompt_tokens":m["inputTokens"].as_u64().unwrap_or(0),
            "completion_tokens":m["outputTokens"].as_u64().unwrap_or(0),
            "cached_prompt_tokens":m["cacheReadTokens"].as_u64().unwrap_or(0)})
    }else {Value::Null};
    let approval=awaiting.map(|a|json!({"id":a["id"],
        "name":a["tool"],"arguments":a["args"]})).unwrap_or(Value::Null);
    let events=job["events"].as_array().cloned().unwrap_or_default()
        .into_iter().map(|event|json!({
            "kind":event["kind"].as_str().or_else(||event["type"].as_str())
                .unwrap_or("event"),"detail":event
        })).collect::<Vec<_>>();
    json!({
        "id":job["id"],"status":status,
        "instruction":job["input"]["prompt"],
        "answer":result["answer"].as_str().or_else(||snapshot["answer"].as_str()),
        "error":job["error"].as_str()
            .or_else(||result["reason"].as_str())
            .or_else(||snapshot["error"].as_str()),
        "checkpoint":{
            "state":{"state":status},
            "rounds":snapshot["step"].as_u64().unwrap_or(0),
            "toolCalls":snapshot["receipts"].as_array().map(Vec::len)
                .or_else(||result["toolCalls"].as_u64().map(|n|n as usize)).unwrap_or(0),
            "answer":snapshot["answer"],"usage":usage,
            "pendingCall":approval,"events":events
        }
    })
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    let mut rows=state.store.all("job").into_iter()
        .filter(|j|j["kind"]=="prompt").collect::<Vec<_>>();
    rows.sort_by(|a,b|b["queuedAt"].as_str().unwrap_or("")
        .cmp(a["queuedAt"].as_str().unwrap_or("")));
    Json(json!(rows.iter().take(30).map(|j|project(&state,j)).collect::<Vec<_>>()))
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match prompt_job(&state,&id){
        Some(job)=>Json(project(&state,&job)).into_response(),
        None=>error(StatusCode::NOT_FOUND,"SDK 작업을 찾지 못했습니다.")
    }
}
pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let Some(instruction)=body["instruction"].as_str()
        .filter(|s|!s.trim().is_empty()&&s.len()<=64000)
    else{return error(StatusCode::BAD_REQUEST,"작업 지시를 입력하세요.");};
    let verify=body["verifyCommand"].as_str().map(str::trim).unwrap_or("");
    if verify.len()>2048 {
        return error(StatusCode::BAD_REQUEST,"검증 명령이 너무 깁니다.");
    }
    let mut input=json!({"prompt":instruction,"toolsEnabled":true,
        "toolsExecutionEnabled":true});
    if let Some(p)=body["providerId"].as_str().filter(|s|!s.is_empty()){
        input["providerId"]=json!(p);
    }
    if !verify.is_empty(){
        input["acceptanceChecks"]=json!([{"type":"command_succeeded",
            "command":verify}]);
        input["ownerInstructions"]=json!(format!(
            "사용자가 검증 명령을 지정했습니다: {verify}. 작업을 마친 뒤 실행하고 결과를 확인하세요."
        ));
    }
    // The same validated job creation endpoint used by the public TS SDK.
    let response=jobs::create(State(state.clone()),
        Json(json!({"kind":"prompt","input":input}))).await;
    let status=response.status();
    if !status.is_success(){return response;}
    let bytes=match to_bytes(response.into_body(),1024*1024).await {
        Ok(b)=>b,Err(_)=>return error(StatusCode::INTERNAL_SERVER_ERROR,
            "작업 응답을 읽지 못했습니다.")
    };
    match serde_json::from_slice::<Value>(&bytes){
        Ok(job)=>(status,Json(project(&state,&job))).into_response(),
        Err(_)=>error(StatusCode::INTERNAL_SERVER_ERROR,
            "작업 응답이 올바르지 않습니다.")
    }
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if prompt_job(&state,&id).is_none(){
        return error(StatusCode::NOT_FOUND,"SDK 작업을 찾지 못했습니다.");
    }
    let response=jobs::cancel(State(state.clone()),Path(id.clone())).await;
    if !response.status().is_success(){return response;}
    one(State(state),Path(id)).await
}
pub async fn approve(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    let Some(job)=prompt_job(&state,&id) else {
        return error(StatusCode::NOT_FOUND,"SDK 작업을 찾지 못했습니다.");
    };
    let Some(a)=pending(&state,&job) else {
        return error(StatusCode::CONFLICT,"승인 대기 중인 실제 도구 호출이 없습니다.");
    };
    if body["callId"]!=a["id"] {
        return error(StatusCode::CONFLICT,"승인 대상 도구 ID가 변경되었습니다.");
    }
    let response=approvals::resolve_approval(
        State(state.clone()),Json(json!({"id":a["id"],"allow":true}))).await;
    if !response.status().is_success(){return response;}
    one(State(state),Path(id)).await
}
pub async fn resume(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=prompt_job(&state,&id) else {
        return error(StatusCode::NOT_FOUND,"SDK 작업을 찾지 못했습니다.");
    };
    let snap=&job["nativeState"];
    let replay_safe=matches!(job["status"].as_str(),Some("failed"|"interrupted"))
        && snap["status"]=="running"
        && snap["modelInFlight"]!=true
        && snap["inFlight"].as_array().is_some_and(Vec::is_empty)
        && job["nativeInFlight"]!=true && job["cancelRequested"]!=true;
    if !replay_safe {
        return error(StatusCode::CONFLICT,
            "확정되지 않은 실행이 있거나 커널이 종료됐습니다. 상태를 확인하고 새 작업으로 시작하세요.");
    }
    let Some(updated)=state.store.patch_if("job",&id,"status",&job["status"],
        &json!({"status":"queued","availableAt":null,"error":null,
            "attempts":0,"recovered":true})) else {
        return error(StatusCode::CONFLICT,"재개 중 작업 상태가 바뀌었습니다.");
    };
    state.store.audit("sdk.main_ui_safe_resume",json!({"jobId":id}));
    state.bus.record("job",&updated);
    Json(project(&state,&updated)).into_response()
}
pub async fn reconcile(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    let Some(job)=prompt_job(&state,&id) else{
        return error(StatusCode::NOT_FOUND,"SDK 작업을 찾지 못했습니다.");
    };
    if job["status"]!="needs_review" || job["cancelRequested"]==true {
        return error(StatusCode::CONFLICT,"수동 확인 대상이 아니거나 이미 취소된 작업입니다.");
    }
    let Some(details)=body["details"].as_str().map(str::trim)
        .filter(|text|!text.is_empty()&&text.len()<=8192)
    else{return error(StatusCode::BAD_REQUEST,"확인한 결과를 입력하세요.");};
    let Some(applied)=body["applied"].as_bool() else {
        return error(StatusCode::BAD_REQUEST,"적용 여부가 필요합니다.");
    };
    let Ok(mut snapshot)=serde_json::from_value::<dominion_sdk::harness::Snapshot>(
        job["nativeState"].clone()) else{
        return error(StatusCode::CONFLICT,"재개 가능한 Rust SDK 체크포인트가 없습니다.");
    };
    let action=match snapshot.reconcile_tool_observation(applied,details) {
        Ok(action)=>action,
        Err(message)=>return error(StatusCode::CONFLICT,&message),
    };
    let operation_id=format!("native-op-{id}-{}",action.id);
    let operation=state.store.get("execution_operation",&operation_id);
    if operation.as_ref().is_some_and(|o|
        o["runId"]!=id || o["tool"]!=action.tool
            || o["args"]!=action.arguments) {
        return error(StatusCode::CONFLICT,
            "원래의 도구 실행 기록과 수동 확인 대상이 일치하지 않습니다.");
    }
    let old_directive=job["nativeDirective"].as_str().unwrap_or("");
    let recovery_rule=if applied {
        "The effect may already exist. Never repeat the observed operation. Perform read-only inspection before any new mutation."
    }else{
        "The owner found no applied effect. Reinspect the current state before planning a safe retry."
    };
    let directive=format!(
        "{old_directive}\n\nOWNER MANUAL RECOVERY: The uncertain {} operation {} was observed {}. Operator note: {}. This observation is NOT independently verified and is NOT a successful tool receipt. {}",
        action.tool,action.id,if applied{"APPLIED"}else{"NOT APPLIED"},details,recovery_rule);
    if directive.len()>32000 {
        return error(StatusCode::CONFLICT,"복구 지시가 너무 깁니다.");
    }
    let observation=json!({
        "applied":applied,"details":details,
        "actionId":action.id,"tool":action.tool,"arguments":action.arguments,
        "recordedAt":chrono::Utc::now().to_rfc3339(),
        "kind":"owner_observation_not_execution_proof",
        "requiresInspection":applied
    });
    // Publish the exact reconciled SDK snapshot *and* a paused job status in
    // one CAS. The owner must explicitly click Resume; no hidden paid replay.
    let Some(updated)=state.store.patch_if("job",&id,"status",&json!("needs_review"),
        &json!({
            "status":"interrupted","nativeState":snapshot,
            "nativeInFlight":false,"nativeOwnerObservation":observation,
            "nativeDirective":directive,"error":null
        })) else {
        return error(StatusCode::CONFLICT,"수동 확인 중 실행 상태가 변경됐습니다.");
    };
    if let Some(operation)=operation {
        if operation["outcomeUnknown"]==true {
            let before=operation["status"].clone();
            let _=state.store.patch_if("execution_operation",&operation_id,
                "status",&before,&json!({
                    "status":"manually_reconciled",
                    "outcomeUnknown":false,"ownerObservation":observation,
                    "reconciledAt":chrono::Utc::now().to_rfc3339()
                }));
        }
    }
    state.store.audit("sdk.main_ui_reconciled_observation",json!({
        "jobId":id,"actionId":action.id,"tool":action.tool,
        "applied":applied,"replayed":false,"independentlyVerified":false
    }));
    state.bus.record("job",&updated);
    Json(project(&state,&updated)).into_response()
}

#[cfg(test)]
mod tests{
    use super::*;
    fn temporary_state()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!("sdk-main-route-{}",uuid::Uuid::new_v4()));
        let cfg=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.join("work")};
        let state=AppState::new(cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()));
        (state,root)
    }
    #[tokio::test]
    async fn pinned_main_ui_creates_a_real_validated_rust_job_and_can_cancel(){
        let (state,root)=temporary_state();
        let response=create(State(state.clone()),Json(json!({
            "instruction":"Inspect Rust code and verify the result",
            "verifyCommand":"cargo test --workspace"
        }))).await;
        assert_eq!(response.status(),StatusCode::ACCEPTED);
        let bytes=to_bytes(response.into_body(),1024*1024).await.unwrap();
        let shown:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(shown["status"],"queued");
        assert_eq!(shown["instruction"],"Inspect Rust code and verify the result");
        let id=shown["id"].as_str().unwrap();
        let job=state.store.get("job",id).unwrap();
        assert_eq!(job["kind"],"prompt");
        assert_eq!(job["input"]["checks"][0]["type"],"tool_succeeded");
        assert_eq!(job["input"]["checks"][0]["tool"],"shell_exec");
        assert_eq!(job["input"]["checks"][0]["contains"],"cargo test --workspace");
        let Json(listed)=list(State(state.clone())).await;
        assert_eq!(listed[0]["id"],id);
        assert_eq!(one(State(state.clone()),Path(id.into())).await.status(),StatusCode::OK);
        let stopped=cancel(State(state.clone()),Path(id.into())).await;
        assert_eq!(stopped.status(),StatusCode::OK);
        assert_eq!(state.store.get("job",id).unwrap()["status"],"cancelled");
        assert_eq!(cancel(State(state.clone()),Path("missing".into())).await.status(),
            StatusCode::NOT_FOUND);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn main_ui_approval_is_bound_to_the_actual_pending_native_tool_call(){
        let (state,root)=temporary_state();
        state.store.put("job",&json!({"id":"sdk-job","kind":"prompt",
            "status":"running","sessionId":"native-session",
            "input":{"prompt":"Inspect before editing"}})).unwrap();
        state.store.put("approval",&json!({"id":"approved-call",
            "status":"pending","sessionId":"native-session","runId":"sdk-job",
            "tool":"workspace_write","args":{"path":"src/lib.rs"}})).unwrap();
        let denied=approve(State(state.clone()),Path("sdk-job".into()),
            Json(json!({"callId":"another-call"}))).await;
        assert_eq!(denied.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("approval","approved-call").unwrap()["status"],"pending");
        let shown=one(State(state.clone()),Path("sdk-job".into())).await;
        let displayed:Value=serde_json::from_slice(
            &to_bytes(shown.into_body(),1024*1024).await.unwrap()).unwrap();
        assert_eq!(displayed["status"],"awaiting_approval");
        assert_eq!(displayed["checkpoint"]["pendingCall"]["id"],"approved-call");
        let allowed=approve(State(state.clone()),Path("sdk-job".into()),
            Json(json!({"callId":"approved-call"}))).await;
        assert_eq!(allowed.status(),StatusCode::OK);
        assert_eq!(state.store.get("approval","approved-call").unwrap()["status"],"approved");
        assert_eq!(approve(State(state.clone()),Path("sdk-job".into()),
            Json(json!({"callId":"approved-call"}))).await.status(),StatusCode::CONFLICT,
            "a decided tool must not be approved a second time");
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn main_ui_reconcile_is_a_real_paused_sdk_checkpoint_not_fake_success(){
        let (state,root)=temporary_state();
        let id="reconcile-job";
        let original=json!({"id":id,"kind":"prompt","status":"needs_review",
            "sessionId":"s-reconcile","nativeInFlight":true,
            "input":{"prompt":"Write result.txt"},
            "nativeState":{"id":id,"status":"needs_review",
                "instruction":"Write result.txt","plan":[],"step":2,
                "modelCalls":1,"chargedTokens":1200,"receipts":[],
                "checks":[],"answer":null,"error":"unconfirmed operation",
                "modelInFlight":false,
                "inFlight":[{"id":"tool-1","tool":"workspace_write",
                    "arguments":{"path":"result.txt","content":"hello"},
                    "dependsOn":[]}]
            }
        });
        state.store.put("job",&original).unwrap();
        state.store.put("execution_operation",&json!({
            "id":"native-op-reconcile-job-tool-1","runId":id,
            "tool":"workspace_write",
            "args":{"path":"result.txt","content":"hello"},
            "status":"interrupted","outcomeUnknown":true
        })).unwrap();
        let result=reconcile(State(state.clone()),Path(id.into()),
            Json(json!({"applied":true,"details":"result.txt contains hello"}))).await;
        assert_eq!(result.status(),StatusCode::OK);
        let response:Value=serde_json::from_slice(
            &to_bytes(result.into_body(),1024*1024).await.unwrap()).unwrap();
        assert_eq!(response["status"],"interrupted",
            "main UI must render its real Resume control after reconciliation");
        let saved=state.store.get("job",id).unwrap();
        assert_eq!(saved["status"],"interrupted");
        assert_eq!(saved["nativeInFlight"],false);
        assert_eq!(saved["nativeState"]["status"],"running");
        assert_eq!(saved["nativeState"]["inFlight"],json!([]));
        assert_eq!(saved["nativeState"]["receipts"][0]["success"],false);
        assert_eq!(saved["nativeState"]["receipts"][0]["result"]["operatorObservedApplied"],true);
        assert_eq!(saved["nativeOwnerObservation"]["requiresInspection"],true);
        assert_eq!(state.store.get("execution_operation",
            "native-op-reconcile-job-tool-1").unwrap()["outcomeUnknown"],false);
        let resumed=resume(State(state.clone()),Path(id.into())).await;
        assert_eq!(resumed.status(),StatusCode::OK);
        assert_eq!(state.store.get("job",id).unwrap()["status"],"queued");
        let again=reconcile(State(state.clone()),Path(id.into()),
            Json(json!({"applied":true,"details":"again"}))).await;
        assert_eq!(again.status(),StatusCode::CONFLICT);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn main_ui_reconciliation_cannot_mask_uncertain_paid_model_delivery(){
        let (state,root)=temporary_state();
        let id="uncertain-paid";
        state.store.put("job",&json!({"id":id,"kind":"prompt",
            "status":"needs_review","nativeInFlight":true,
            "input":{"prompt":"Create a new project"},
            "nativeState":{"id":id,"status":"needs_review",
                "instruction":"Create a new project","plan":[],"step":1,
                "modelCalls":0,"chargedTokens":0,"receipts":[],
                "checks":[],"answer":null,"error":"network disconnected",
                "modelInFlight":true,"inFlight":[]}
        })).unwrap();
        let attempted=reconcile(State(state.clone()),Path(id.into()),
            Json(json!({"applied":false,"details":"I saw no file"}))).await;
        assert_eq!(attempted.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("job",id).unwrap()["nativeInFlight"],true);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn uncertain_native_effects_cannot_be_replayed_as_a_main_ui_resume(){
        let (state,root)=temporary_state();
        state.store.put("job",&json!({"id":"uncertain","kind":"prompt",
            "status":"failed","input":{"prompt":"Alter a project"},
            "nativeInFlight":true,"nativeState":{"status":"running",
                "modelInFlight":true,"inFlight":[{"name":"workspace_write"}]}})).unwrap();
        let response=resume(State(state.clone()),Path("uncertain".into())).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("job","uncertain").unwrap()["status"],"failed");
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn main_frontend_projects_real_native_state_without_fake_tokens(){
        let root=std::env::temp_dir().join(format!("sdk-main-ui-{}",uuid::Uuid::new_v4()));
        let cfg=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.join("work")};
        let state=AppState::new(cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()));
        let job=json!({"id":"j","kind":"prompt","status":"running",
            "input":{"prompt":"Fix the code"},"nativeState":{
                "step":2,"receipts":[{"tool":"workspace_read_range"}]}});
        let shown=project(&state,&job);
        assert_eq!(shown["status"],"running");
        assert_eq!(shown["instruction"],"Fix the code");
        assert_eq!(shown["checkpoint"]["toolCalls"],1);
        assert!(shown["checkpoint"]["usage"].is_null());
        let mut needs=job;
        needs["status"]=json!("needs_review");
        assert_eq!(project(&state,&needs)["status"],"in_doubt");
        let _=std::fs::remove_dir_all(root);
    }
}
