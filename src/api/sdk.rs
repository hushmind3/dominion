//! SDK, chat, rooms, jobs and workflows share the native Rust execution path.
//! SQLite owns records, approvals, effects and native sub4 checkpoints.
use crate::{engine::{self,chat::TurnRequest,gateway},state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use super::response::error;
use std::collections::{HashMap,HashSet};
use sha2::{Digest,Sha256};
use dominion_core::run_event::{RunEventRef,RunLifecycle};

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SourceRef { kind: String, id: String }

/// Provenance is a display/linking hint, never an authorization or proof.
/// The caller must still pass normal model/tool permissions and owner approval.
fn source_metadata(source: Option<&SourceRef>) -> Result<Value, String> {
    let Some(source) = source else { return Ok(Value::Null); };
    let valid_kind = |s: &str| !s.is_empty() && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'));
    // Source IDs are opaque record identifiers, never filesystem paths.
    // Permit existing slash/Unicode IDs while rejecting controls and oversized inputs.
    let valid_id = !source.id.trim().is_empty() && source.id.len() <= 200
        && !source.id.chars().any(char::is_control);
    if !valid_kind(&source.kind) || !valid_id {
        return Err("작업 출처(kind/id) 형식이 올바르지 않습니다.".into());
    }
    Ok(json!({"kind":source.kind,"id":source.id,"verified":false}))
}

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Start {
    instruction: String,
    provider_id: Option<String>,
    model: Option<String>,
    agent_id: Option<String>,
    source: Option<SourceRef>,
    idempotency_key: Option<String>,
    verify_command: Option<String>,
    max_rounds: Option<usize>,
    #[serde(flatten)] options: Value,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Approve{call_id:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ReconcileInput{applied:bool,details:String}

/// The model answering is not evidence that a requested artifact was produced.
    /// Preserve the user's answer for inspection, and expose the independently
    /// finalized task verdict separately from transport-level completion.
fn finish_with_evidence(record:&mut Value,message:&Value,run:Option<&Value>){
    record["status"]=json!("completed");
    record["answer"]=message["content"].clone();
    record["finalMessageId"]=message["id"].clone();
    let outcome=run.map(|r|r["taskOutcome"].clone()).unwrap_or(Value::Null);
    record["taskOutcome"]=outcome.clone();
    record["verified"]=json!(outcome=="succeeded");
    record["acceptancePassed"]=run.map(|r|r["acceptancePassed"].clone()).unwrap_or(Value::Null);
    record["failureClass"]=run.map(|r|r["failureClass"].clone()).unwrap_or(Value::Null);
    record["evidenceExecutionId"]=run.map(|r|r["id"].clone()).unwrap_or(Value::Null);
    if matches!(outcome.as_str(),Some("failed"|"cancelled"))
        || run.is_some_and(|r|r["status"]!="completed") {
        record["status"]=json!("failed");
        record["error"]=json!("네이티브 실행 결과가 성공 조건을 충족하지 못했습니다.");
    } else if record["verifyCommand"].as_str().is_some_and(|v|!v.trim().is_empty())
        && record["verified"]!=true {
        record["status"]=json!("failed");
        record["error"]=json!("요청한 검증 명령의 성공 증거를 확보하지 못했습니다.");
    } else {
        record["error"]=Value::Null;
    }
}

/// If the native completion evidence cannot be read, never permit a
/// blind resume that might duplicate already-applied model/tool effects.
fn require_manual_evidence_review(record:&mut Value){
    record["status"]=json!("needs_review");
    record["verified"]=json!(false);
    record["failureClass"]=json!("completion_evidence_unavailable");
    record["error"]=json!("완료 증거 조회에 실패했습니다. 실행 기록을 확인하세요.");
}
fn execution(state:&AppState,record:&Value)->Result<Option<Value>,String>{
    let Some(session)=record["sessionId"].as_str()else{return Ok(None)};
    state.store.sdk_latest_execution(session)
}
/// A consistent read snapshot for SDK listings: collect executions,
/// approvals and uncertain effects once rather than scanning them per run.
/// The single-run response reuses this same projection contract.
struct ViewIndex {
    runs:HashMap<String,Value>,
    pending:HashMap<String,Value>,
    uncertain:HashSet<String>,
    uncertain_counts:HashMap<String,usize>,
}
impl ViewIndex{
    fn from_rows(executions:Vec<Value>,approvals:Vec<Value>,operations:Vec<Value>)->Self{
        let mut runs=HashMap::<String,Value>::new();
        for run in executions {
            let Some(session)=run["sessionId"].as_str().map(str::to_string) else{continue};
            let newer=runs.get(&session).is_none_or(|prior|
                run["startedAt"].as_str().unwrap_or("") >= prior["startedAt"].as_str().unwrap_or(""));
            if newer {runs.insert(session,run);}
        }
        let mut pending=HashMap::<String,Value>::new();
        for approval in approvals {
            if approval["status"]=="pending" {
                if let Some(session)=approval["sessionId"].as_str(){
                    pending.entry(session.to_string()).or_insert(approval);
                }
            }
        }
        let mut uncertain_counts=HashMap::<String,usize>::new();
        for op in operations.iter().filter(|op|op["outcomeUnknown"]==true){
            if let Some(id)=op["runId"].as_str(){*uncertain_counts.entry(id.to_owned()).or_default()+=1;}
        }
        let uncertain=uncertain_counts.keys().cloned().collect();
        Self{runs,pending,uncertain,uncertain_counts}
    }
    fn load(state:&AppState)->Self{
        let (executions,operations)=match state.store.sdk_evidence_rows(){
            Ok(rows)=>rows,
            Err(error)=>{
                state.store.audit("sdk.evidence_query_failed",json!({"error":error}));
                (state.store.all("execution"),state.store.all("execution_operation"))
            }
        };
        let approvals=match state.store.pending_approvals(){
            Ok(rows)=>rows,
            Err(error)=>{
                state.store.audit("sdk.approval_query_failed",json!({"error":error}));
                state.store.all("approval")
            }
        };
        Self::from_rows(executions,approvals,operations)
    }
}
fn public_view_indexed(state:&AppState,mut record:Value,index:&ViewIndex)->Value{
    let Some(session)=record["sessionId"].as_str().map(str::to_owned)else{
        record["readOnly"]=json!(true);return record;
    };
    let pending=index.pending.get(&session);
    let run=index.runs.get(&session);
    let raw_status=record["status"].as_str().unwrap_or("").to_owned();
    let active=state.active.lock().unwrap().contains(&session);
    let uncertain_count=run.and_then(|r|r["id"].as_str())
        .and_then(|id|index.uncertain_counts.get(id)).copied().unwrap_or(0);
    // In-doubt effects must remain visible for *interrupted and failed* SDK
    // runs as well. Otherwise resume() could approve a replay from a failed
    // status while its last tool result remains unobserved.
    let revisitable=matches!(record["status"].as_str(),
        Some("queued"|"running"|"failed"|"interrupted"));
    if revisitable && run.is_some_and(|run|
        run["id"].as_str().is_some_and(|id|index.uncertain.contains(id))){
        record["status"]=json!("in_doubt");
    }else if matches!(record["status"].as_str(),Some("queued"|"running")) {
        if let Some(approval)=pending {
            record["status"]=json!("awaiting_approval");
            record["pendingApprovalId"]=approval["id"].clone();
        }else if record["status"]=="running"
            && run.is_some_and(|run|run["status"]=="interrupted")
            && !state.active.lock().unwrap().contains(&session){
            record["status"]=json!("interrupted");
        }
    }
    let live=matches!(raw_status.as_str(),"queued"|"running");
    let recoverable=matches!(raw_status.as_str(),"failed"|"interrupted");
    let approval=raw_status=="running" && pending.is_some() && !state.is_cancelled(&session)
        && record["status"]=="awaiting_approval";
    let recheck=raw_status=="needs_review" && record["failureClass"]=="completion_evidence_unavailable";
    record["actions"]=json!({
        "cancel":live,
        "approve":approval,
        "deny":approval,
        "resume":!active && ((recoverable && uncertain_count==0)||recheck),
        "reconcile":recoverable && !active && uncertain_count==1,
        "steer":live && record["status"]!="in_doubt",
        "refresh":true
    });
    record["uncertainEffectCount"]=json!(uncertain_count);
    if let Some(run)=run{
        // Legacy records lacked a verdict field. Read but never rewrite them.
        if record.get("taskOutcome").is_none() {
            record["taskOutcome"]=run["taskOutcome"].clone();
            record["verified"]=json!(run["taskOutcome"]=="succeeded");
            record["acceptancePassed"]=run["acceptancePassed"].clone();
        }
        let message=run["finalMessageId"].as_str().and_then(|id|state.store.message(id));
        let usage=message.as_ref().map(|m|json!({
            "prompt_tokens":m["metadata"]["usage"]["promptTokens"],
            "completion_tokens":m["metadata"]["usage"]["completionTokens"],
            "cached_prompt_tokens":m["metadata"]["usage"]["cachedPromptTokens"],
            "estimated":m["metadata"]["usage"]["estimated"]
        }));
        let mut events=run["kernelState"]["native"]["events"].as_array().cloned().unwrap_or_default();
        if let Some(items)=record["observations"].as_array(){events.extend(items.clone());}
        record["checkpoint"]=json!({
            "state":{"state":record["status"]},"rounds":run["modelCalls"],
            "toolCalls":run["toolCalls"],"answer":record["answer"],
            "usage":usage,"events":events,
            "pendingCall":pending.map(|a|json!({"id":a["id"],"name":a["tool"],"arguments":a["args"]})),
            "executionId":run["id"],
            "taskOutcome":record["taskOutcome"],"verified":record["verified"],
            "acceptancePassed":record["acceptancePassed"],
            "failureClass":record["failureClass"]
        });
        // Expose the same canonical execution reference as SQLite event replay.
        // The SDK's display status remains separate from the native lifecycle.
        if let Some(id)=run["id"].as_str(){
            if let Some(mut reference)=RunEventRef::new(id){
                reference.session_id=Some(session.to_owned());
                reference.room_id=run["roomId"].as_str().map(str::to_string);
                reference.job_id=run["jobId"].as_str().map(str::to_string);
                reference.parent_execution_id=run["parentExecutionId"].as_str().map(str::to_string);
                reference.task_id=run["taskId"].as_str().map(str::to_string);
                reference.agent_id=run["agentId"].as_str().map(str::to_string);
                reference.lifecycle=run["status"].as_str().and_then(RunLifecycle::parse);
                record["runRef"]=json!(reference);
            }
        }
        record["runtime"]=json!("dominion-core");
        // Chat and SDK share the same native execution accounting. The
        // checkpoint's old usage fields describe the primary response only.
        record["modelAccounting"]=run["modelAccounting"].clone();
        record["checkpoint"]["modelAccounting"]=run["modelAccounting"].clone();
    }
    record
}
pub(crate) fn public_view(state:&AppState,record:Value)->Value{
    let Some(session)=record["sessionId"].as_str() else{
        let mut record=record;
        record["readOnly"]=json!(true);
        return record;
    };
    match state.store.sdk_evidence_for_session(session){
        Ok((runs,approvals,operations))=>{
            let index=ViewIndex::from_rows(runs,approvals,operations);
            public_view_indexed(state,record,&index)
        }
        Err(error)=>{
            state.store.audit("sdk.single_evidence_query_failed",json!({"error":error}));
            public_view_indexed(state,record,&ViewIndex::load(state))
        }
    }
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    let index=ViewIndex::load(&state);
    Json(json!(state.store.all("sdk_run").into_iter()
        .map(|record|public_view_indexed(&state,record,&index)).collect::<Vec<_>>()))
}
/// Bounded indexed lookup used by optional tool UIs to recover prior SDK runs.
/// No tool-specific execution or approval behavior enters the SDK.
pub async fn by_source(State(state):State<AppState>,Path(kind):Path<String>)->Response{
    if kind.is_empty()||kind.len()>64||!kind.chars()
        .all(|c|c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'.'|':')){
        return error(StatusCode::BAD_REQUEST,"작업 출처 형식이 올바르지 않습니다.");
    }
    match state.store.sdk_runs_by_source(&kind,40){
        Ok(records)=>Json(json!(records.into_iter().map(|r|json!({
            "id":r["id"],"source":r["source"],"status":r["status"],
            "sessionId":r["sessionId"],"taskOutcome":r["taskOutcome"],
            "verified":r["verified"],"createdAt":r["createdAt"]
        })).collect::<Vec<_>>())).into_response(),
        Err(message)=>error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    }
}

pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    state.store.get("sdk_run",&id).map(|r|Json(public_view(&state,r)).into_response()).unwrap_or_else(||error(StatusCode::NOT_FOUND,"작업이 없습니다."))
}
/// Prepare a native SDK task without persistent side effects. This is also
/// used by optional tools that must atomically link their own approval receipt
/// with the ordinary SDK session and execution record.
fn prepare_start(state:&AppState,input:&Start)->Result<(Value,Value),String>{
    if input.instruction.trim().is_empty()||input.instruction.len()>64000{
        return Err("작업 지시는 1~64000자로 입력하세요.".into());
    }
    if input.verify_command.as_ref().is_some_and(|c|c.trim().is_empty()||c.len()>2048){
        return Err("검증 명령의 길이를 확인하세요.".into());
    }
    if input.agent_id.as_ref().is_some_and(|id|id.is_empty()||id.len()>200
        ||state.store.get("agent",id).is_none()){
        return Err("SDK 작업에 지정한 에이전트가 없습니다.".into());
    }
    let source=source_metadata(input.source.as_ref())?;
    let route=gateway::resolve(state,input.provider_id.as_deref().unwrap_or(""),
        input.model.as_deref())?;
    let options=engine::job_control::options(state,&input.options)?;
    let id=uuid::Uuid::new_v4().to_string();
    let session=uuid::Uuid::new_v4().to_string();
    let mut session_row=json!({
        "id":session,
        "title":input.instruction.chars().take(40).collect::<String>(),
        "providerId":route.id,"model":route.model,
        "nativeSdkRunId":id,
        "agentId":input.agent_id,
        "nativeSdkMaxRounds":input.max_rounds.unwrap_or(24).clamp(1,80),
        "nativeSdkVerifyCommand":input.verify_command,
    });
    for (key,value) in options.as_object().unwrap() {
        session_row[key]=value.clone();
    }
    session_row["budgetOwnerId"]=json!(format!("sdk:{id}"));
    let record=json!({
        "id":id,"sessionId":session,"status":"queued",
        "instruction":input.instruction,"providerId":route.id,"model":route.model,
        "runtime":"dominion-core","verifyCommand":input.verify_command,
        "agentId":input.agent_id,"source":source,
        "maxRounds":input.max_rounds.unwrap_or(24).clamp(1,80)
    });
    Ok((session_row,record))
}

/// Opt-in retry protection. Receipt, session and SDK run share one transaction.
fn retry_identity(input:&Start)->Result<Option<(String,String)>,String>{
    let Some(key)=input.idempotency_key.as_deref()else{return Ok(None)};
    if key.is_empty()||key.len()>128
        ||!key.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'.'|':')){
        return Err("idempotencyKey는 1~128자의 영문·숫자·_-.:만 허용합니다.".into());
    }
    let body=json!({"instruction":input.instruction,"providerId":input.provider_id,
        "agentId":input.agent_id,"model":input.model,"maxRounds":input.max_rounds,
        "verifyCommand":input.verify_command,"source":input.source.as_ref().map(|s|
            json!({"kind":s.kind,"id":s.id})),"options":input.options});
    Ok(Some((key.to_owned(),format!("{:x}",Sha256::digest(body.to_string().as_bytes())))))
}
fn earlier_run(state:&AppState,key:&str,fingerprint:&str)->Option<Response>{
    let claim=state.store.get("sdk_start_claim",key)?;
    if claim["fingerprint"]!=fingerprint{
        return Some(error(StatusCode::CONFLICT,"다른 요청에 사용한 idempotencyKey입니다."));
    }
    let Some(run_id)=claim["runId"].as_str()else{
        return Some(error(StatusCode::CONFLICT,"기존 요청 기록이 불완전합니다."));
    };
    Some(match state.store.get("sdk_run",run_id){
        Some(run)=>Json(public_view(state,run)).into_response(),
        None=>error(StatusCode::CONFLICT,"기존 SDK 작업이 누락됐습니다. 중복 실행하지 않습니다.")
    })
}
fn claim_run(state:&AppState,key:&str,fingerprint:&str,
    session:&Value,run:&Value)->Result<(),String>{
    state.store.mutate_record("sdk_start_claim",key,json!({"id":key}),|claim|{
        if claim["runId"].is_string(){return Err("already_claimed".into());}
        claim["runId"]=run["id"].clone();
        claim["fingerprint"]=json!(fingerprint);
        claim["createdAt"]=json!(chrono::Utc::now().to_rfc3339());
        Ok(vec![("session".into(),session.clone()),("sdk_run".into(),run.clone())])
    }).map(|_|())
}

pub async fn start(State(state):State<AppState>,Json(input):Json<Start>)->Response{
    let retry=match retry_identity(&input){
        Ok(value)=>value,Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
    };
    if let Some((key,fingerprint))=&retry{
        if let Some(response)=earlier_run(&state,key,fingerprint){return response;}
    }
    let (session_row,record)=match prepare_start(&state,&input){
        Ok(prepared)=>prepared,
        Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
    };
    // The owner limits and SDK run commit atomically with an optional
    // retry receipt. No second engine or standalone storage is introduced.
    let persisted=if let Some((key,fingerprint))=&retry{
        match claim_run(&state,key,fingerprint,&session_row,&record){
            Ok(())=>record.clone(),
            Err(message) if message=="already_claimed"=>{
                return earlier_run(&state,key,fingerprint).unwrap_or_else(||
                    error(StatusCode::CONFLICT,"기존 SDK 요청 기록을 찾을 수 없습니다."));
            },
            Err(message)=>{
                state.store.audit("sdk.start_failed",json!({"error":message}));
                return error(StatusCode::INTERNAL_SERVER_ERROR,"SDK 작업 저장에 실패했습니다.");
            }
        }
    }else{
        if let Err(message)=state.store.transaction(&[
            ("session".to_string(),session_row),
            ("sdk_run".to_string(),record.clone()),
        ]){
            state.store.audit("sdk.start_failed",json!({"error":message}));
            return error(StatusCode::INTERNAL_SERVER_ERROR,"SDK 작업 저장에 실패했습니다.");
        }
        record.clone()
    };
    // Preserve existing API metadata (createdAt/updatedAt) returned by Store
    // while the session and SDK run remain transactionally durable.
    let run_id=persisted["id"].as_str().unwrap_or_default();
    let saved=state.store.get("sdk_run",run_id).unwrap_or(persisted);
    launch(state.clone(),saved.clone(),false);
    (StatusCode::ACCEPTED,Json(public_view(&state,saved))).into_response()
}
pub(crate) fn launch(state:AppState,record:Value,resume:bool){
    tokio::spawn(async move{
        let id=record["id"].as_str().unwrap_or("");
        if let Some(saved)=state.store.patch_if("sdk_run",id,"status",&record["status"],&json!({"status":"running"})){state.bus.record("sdk_run",&saved);}else{return;}
        let control=state.store.get("sdk_run",id).unwrap_or_else(||record.clone());
        let goal=engine::job_control::intent(record["instruction"].as_str().unwrap_or(""),&control);
        if let Some(session)=record["sessionId"].as_str(){state.store.patch_if("session",session,"id",&json!(session),&json!({"ownerAppliedRevision":control["steeringRevision"]}));}
        let request=TurnRequest::from_value(&json!({"sessionId":record["sessionId"],"input":goal,"agentId":record["agentId"],"providerId":record["providerId"],"model":record["model"],"autoLearning":false}));
        let outcome=match request{Ok(request)=>if resume{engine::run_quiet_resume(&state,&request).await}else{engine::run_quiet(&state,&request).await},Err(e)=>Err(e)};
        let Some(current)=state.store.get("sdk_run",id)else{return;};
        if current["status"]!="running"{return;}
        let mut finalized=current.clone();
        match outcome {
            Ok(Some(message))=>{
                // An evidence read failure cannot be reported as verified
                // completion. Preserve the run for explicit investigation.
                match execution(&state,&record){
                    Ok(run)=>finish_with_evidence(&mut finalized,&message,run.as_ref()),
                    Err(error)=>{
                        state.store.audit("sdk.completion_evidence_read_failed",
                            json!({"sdkRunId":id,"error":error}));
                        require_manual_evidence_review(&mut finalized);
                    }
                }
            },
            Ok(None)=>{
                finalized["status"]=json!("failed");
                finalized["verified"]=json!(false);
                finalized["error"]=json!("모델의 완료 응답이 없습니다.");
            },
            Err(error)=>{
                finalized["status"]=json!("failed");
                finalized["verified"]=json!(false);
                finalized["error"]=json!(error);
            }
        }
        finalized["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
        let mut patch=json!({});
        for key in ["status","answer","finalMessageId","taskOutcome","verified","acceptancePassed",
            "failureClass","evidenceExecutionId","error","finishedAt"] {
            if let Some(value)=finalized.get(key){patch[key]=value.clone();}
        }
        // CAS merge prevents a late completion from overwriting cancellation or
        // owner steering; no unconditional entire-record put is permitted.
        if let Some(saved)=state.store.patch_if("sdk_run",id,"status",&json!("running"),&patch){
            state.bus.record("sdk_run",&saved);
        }
    });
}
pub async fn approve(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Approve>)->Response{
    let Some(record)=state.store.get("sdk_run",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.");};
    if record["status"]!="running" {
        return error(StatusCode::CONFLICT,"실행 중인 SDK 작업만 도구를 승인할 수 있습니다.");
    }
    let Some(session)=record["sessionId"].as_str()else{
        return error(StatusCode::CONFLICT,"작업 세션을 찾을 수 없습니다.");
    };
    if state.is_cancelled(session){
        return error(StatusCode::CONFLICT,"중단된 세션의 도구는 승인할 수 없습니다.");
    }
    let Some(approval)=state.store.get("approval",&input.call_id)else{return error(StatusCode::CONFLICT,"승인 요청을 찾을 수 없습니다.");};
    if approval["sessionId"]!=record["sessionId"]||approval["status"]!="pending"{
        return error(StatusCode::CONFLICT,"이 작업의 대기 중인 승인이 아닙니다.");
    }
    let resolved=super::approvals::resolve_approval(
        State(state.clone()),Json(json!({"id":input.call_id,"allow":true}))
    ).await;
    if !resolved.status().is_success(){return resolved;}
    if state.store.get("approval",&input.call_id)
        .is_none_or(|saved|saved["status"]!="approved"){
        return error(StatusCode::CONFLICT,"승인 처리 중 다른 요청이 먼저 상태를 변경했습니다.");
    }
    let current=state.store.get("sdk_run",&id).unwrap_or(record);
    Json(public_view(&state,current)).into_response()
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(record)=state.store.get("sdk_run",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.");};
    if !matches!(record["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"진행 중인 작업만 중단할 수 있습니다.");
    }
    let Some(session)=record["sessionId"].as_str()else{
        return error(StatusCode::CONFLICT,"구형 하네스 기록은 읽기 전용입니다.");
    };
    // The terminal state wins the race against concurrent model completion.
    // A failed claim must not cancel a session that has already finished.
    let Some(saved)=state.store.patch_if(
        "sdk_run",&id,"status",&record["status"],
        &json!({"status":"cancelled","cancelRequested":true,
            "finishedAt":chrono::Utc::now().to_rfc3339()})
    )else{
        return error(StatusCode::CONFLICT,"작업 상태가 변경되어 중단할 수 없습니다.");
    };
    state.cancel_session(session);
    state.bus.record("sdk_run",&saved);
    Json(public_view(&state,saved)).into_response()
}
/// A model reply already exists, but the post-run evidence query failed.
/// Revalidate persisted native evidence without invoking the model or tools.
fn recheck_completion(state:&AppState,record:Value)->Response{
    let Some(id)=record["id"].as_str()else{
        return error(StatusCode::CONFLICT,"SDK 실행 ID가 없습니다.");
    };
    let run=match execution(state,&record){
        Ok(Some(run)) if run["status"]=="completed"=>run,
        Ok(_)=>return error(StatusCode::CONFLICT,
            "완료된 네이티브 실행 증거가 없어 재생을 금지했습니다."),
        Err(message)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    };
    let Some(run_id)=run["id"].as_str()else{
        return error(StatusCode::CONFLICT,"검증할 실행 ID가 없습니다.");
    };
    match state.store.has_unknown_execution_effect(run_id){
        Ok(true)=>return error(StatusCode::CONFLICT,
            "결과 불명확 도구가 있어 먼저 확인해야 합니다."),
        Ok(false)=>{},
        Err(message)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    }
    let Some(message)=run["finalMessageId"].as_str()
        .and_then(|mid|state.store.message(mid))else{
        return error(StatusCode::CONFLICT,
            "저장된 모델 최종 응답이 없어 재실행하지 않습니다.");
    };
    let mut finished=record.clone();
    finish_with_evidence(&mut finished,&message,Some(&run));
    finished["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
    let mut patch=json!({});
    for key in ["status","answer","finalMessageId","taskOutcome","verified",
        "acceptancePassed","failureClass","evidenceExecutionId","error","finishedAt"]{
        if let Some(value)=finished.get(key){patch[key]=value.clone();}
    }
    let Some(saved)=state.store.patch_if("sdk_run",id,"status",
        &json!("needs_review"),&patch)else{
        return error(StatusCode::CONFLICT,"SDK 상태가 변경되어 재확인하지 않았습니다.");
    };
    state.bus.record("sdk_run",&saved);
    state.store.audit("sdk.completion_evidence_rechecked",
        json!({"sdkRunId":id,"nativeExecutionId":run_id,"verified":saved["verified"]}));
    Json(public_view(state,saved)).into_response()
}

pub async fn resume(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(record)=state.store.get("sdk_run",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.");};
    if record["sessionId"].as_str().is_none(){return error(StatusCode::CONFLICT,"구형 하네스 기록은 읽기 전용입니다. 새 공통 코어 작업으로 시작하세요.");}
    if record["status"]=="needs_review"
        && record["failureClass"]=="completion_evidence_unavailable"{
        return recheck_completion(&state,record);
    }
    if !matches!(record["status"].as_str(),Some("failed"|"interrupted")){return error(StatusCode::CONFLICT,"재개 가능한 중단 상태가 아닙니다.");}
    if public_view(&state,record.clone())["status"]=="in_doubt"{return error(StatusCode::CONFLICT,"불명확한 도구 효과를 먼저 확인하세요.");}
    launch(state.clone(),record.clone(),true);(StatusCode::ACCEPTED,Json(public_view(&state,record))).into_response()
}
pub async fn reconcile(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<ReconcileInput>)->Response{
    if input.details.trim().is_empty()||input.details.len()>8192{return error(StatusCode::BAD_REQUEST,"실제 확인한 결과를 1~8192자로 입력하세요.");}
    let Some(mut record)=state.store.get("sdk_run",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.");};
    let expected_status=record["status"].as_str().unwrap_or("").to_string();
    if !matches!(expected_status.as_str(),"failed"|"interrupted"){
        return error(StatusCode::CONFLICT,"실행·완료·취소된 작업의 결과를 변경할 수 없습니다.");
    }
    let Some(session)=record["sessionId"].as_str()else{return error(StatusCode::CONFLICT,"구형 하네스 기록은 읽기 전용입니다.");};
    if state.active.lock().unwrap().contains(session){return error(StatusCode::CONFLICT,"실행 중에는 복구 결과를 변경할 수 없습니다.");}
    let run=match execution(&state,&record){
        Ok(Some(run))=>run,
        Ok(None)=>return error(StatusCode::CONFLICT,"복구할 실행 기록이 없습니다."),
        Err(message)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    };
    let run_id=run["id"].as_str().unwrap_or("");
    let pending=match state.store.execution_operations(run_id) {
        Ok(rows)=>rows.into_iter().filter(|op|op["outcomeUnknown"]==true).collect::<Vec<_>>(),
        Err(message)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    };
    if pending.len()!=1{return error(StatusCode::CONFLICT,"효과가 불명확한 도구 하나만 수동 확인할 수 있습니다.");}
    let mut op=pending[0].clone();op["outcomeUnknown"]=json!(false);op["effectState"]=json!(if input.applied{"applied"}else{"not_applied"});
    op["operatorObservation"]=json!({"applied":input.applied,"details":input.details,"observedAt":chrono::Utc::now().to_rfc3339()});
    record["status"]=json!("interrupted");let mut observations=record["observations"].as_array().cloned().unwrap_or_default();
    observations.push(json!({"kind":"operator_observation","detail":op["operatorObservation"]}));record["observations"]=json!(observations);
    let mut updates=vec![("execution_operation".to_string(),op.clone()),("sdk_run".to_string(),record.clone())];
    let mut checkpoints=vec![("execution".to_string(),run.clone())];
    let frames=match state.store.sdk_frames_for_execution(run_id){
        Ok(frames)=>frames,
        Err(message)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    };
    checkpoints.extend(frames.into_iter().map(|frame|("native_frame".to_string(),frame)));
    for (kind,mut saved) in checkpoints{
        let Ok(mut snapshot)=serde_json::from_value::<dominion_core::hosted::Snapshot>(saved["kernelState"].clone())else{continue;};
        if let Some(call)=snapshot.native.in_flight.clone().filter(|call|call.name==op["tool"]&&call.arguments==op["arguments"]){
            let observation=if input.applied{Ok(input.details.clone())}else{Err(input.details.clone())};
            if let Err(message)=dominion_core::journal::finish(&mut snapshot.native,&call,&observation,true){return error(StatusCode::CONFLICT,&message);}
            saved["kernelState"]=serde_json::to_value(snapshot).unwrap_or(Value::Null);updates.push((kind,saved));
        }
    }
    let operation_id=op["id"].as_str().unwrap_or("");
    match state.store.reconcile_sdk_effect_once(
        &id,&expected_status,operation_id,run_id,&updates
    ){
        Ok(true)=>{},
        Ok(false)=>return error(StatusCode::CONFLICT,
            "이미 확인한 도구 효과이거나 SDK 작업 상태가 변경되었습니다."),
        Err(message)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    }
    state.bus.record("sdk_run",&record);
    if let Err(message)=engine::execution_budget::refresh_after_observation(&state,run_id){
        state.store.audit("native.reconciliation_budget_refresh_failed",
            json!({"runId":run_id,"operationId":operation_id,"error":message}));
        return error(StatusCode::INTERNAL_SERVER_ERROR,
            "도구 결과는 저장됐지만 예산 재계산에 실패했습니다. 실행 기록을 확인하세요.");
    }
    state.store.audit("native.operator_reconciled",json!({"runId":run["id"],"operationId":op["id"],"observation":op["operatorObservation"]}));
    Json(public_view(&state,record)).into_response()
}


/// A process restart cannot replay a possibly billed model request. Recover
/// completed replies from SQLite; leave interrupted SDK work for explicit resume.
pub(crate) fn recover(state:&AppState){
    // One snapshot avoids scanning every execution for each SDK run at startup.
    let index=ViewIndex::load(state);
    for mut record in state.store.all("sdk_run"){
        if record["sessionId"].as_str().is_none()||!matches!(record["status"].as_str(),Some("running"|"queued")){continue;}
        let completed=record["sessionId"].as_str().and_then(|session|index.runs.get(session))
            .filter(|run|run["status"]=="completed")
            .and_then(|run|run["finalMessageId"].as_str().and_then(|id|state.store.message(id)));
        if let Some(message)=completed{
            let run=record["sessionId"].as_str().and_then(|session|index.runs.get(session));
            finish_with_evidence(&mut record,&message,run);
        } else {
            record["status"]=json!("interrupted");
            record["verified"]=json!(false);
            record["error"]=json!("서버가 재시작되었습니다. 실행 기록을 확인한 뒤 재개하세요.");
        }
        state.store.put("sdk_run",&record);
    }
}

pub async fn steer(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    super::jobs::steering_response(engine::job_control::steer(&state,"sdk_run",&id,body["instruction"].as_str().unwrap_or(""),body["mode"].as_str().unwrap_or("append")))
}


#[cfg(test)]
mod view_tests{
    use super::*;
    fn fixture() -> AppState {
        let root=std::env::temp_dir().join(format!("sdk-lifecycle-{}",uuid::Uuid::new_v4()));
        let cfg=crate::config::Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()))
    }
    #[test]
    fn retry_receipt_atomically_prevents_duplicate_sdk_jobs(){
        let state=fixture();
        let s1=json!({"id":"session-one"});let s2=json!({"id":"session-two"});
        let r1=json!({"id":"run-one","sessionId":"session-one"});
        let r2=json!({"id":"run-two","sessionId":"session-two"});
        let results=std::thread::scope(|scope|{
            let a=scope.spawn(||claim_run(&state,"key1","digest",&s1,&r1));
            let b=scope.spawn(||claim_run(&state,"key1","digest",&s2,&r2));
            (a.join().unwrap(),b.join().unwrap())
        });
        assert_eq!(usize::from(results.0.is_ok())+usize::from(results.1.is_ok()),1);
        assert_eq!(state.store.count_kind("sdk_run"),1);
        assert_eq!(state.store.count_kind("session"),1);
        assert_eq!(earlier_run(&state,"key1","different").unwrap().status(),StatusCode::CONFLICT);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn retry_key_has_stable_digest_and_rejects_invalid_characters(){
        let req:Start=serde_json::from_value(json!({
            "instruction":"test","idempotencyKey":"stable-1","tokenBudget":5000
        })).unwrap();
        assert_eq!(retry_identity(&req).unwrap().unwrap().1.len(),64);
        let bad:Start=serde_json::from_value(json!({
            "instruction":"test","idempotencyKey":"../path"
        })).unwrap();
        assert!(retry_identity(&bad).is_err());
    }
    #[test]
    fn sdk_source_is_generic_metadata_and_cannot_encode_permissions() {
        assert!(source_metadata(None).unwrap().is_null());
        let source=SourceRef{kind:"task_request".into(),id:"task:draft:123".into()};
        let meta=source_metadata(Some(&source)).unwrap();
        assert_eq!(meta["kind"],"task_request");
        assert_eq!(meta["id"],"task:draft:123");
        assert_eq!(meta["verified"],false);
        assert_eq!(source_metadata(Some(&SourceRef{kind:"task_request".into(),
            id:"lead/한글".into()})).unwrap()["id"],"lead/한글");
        assert!(source_metadata(Some(&SourceRef{kind:"../../admin".into(),
            id:"1".into()})).is_err());
        assert!(source_metadata(Some(&SourceRef{kind:"document".into(),
            id:"a".repeat(201)})).is_err());
    }
    #[tokio::test]
    async fn evidence_recheck_recovers_persisted_success_without_replaying_model_or_tools(){
        let state=fixture();
        state.store.put("sdk_run",&json!({"id":"sdk-recheck","sessionId":"s1",
            "status":"needs_review","failureClass":"completion_evidence_unavailable"})).unwrap();
        let message=state.store.write_message(&json!({"id":"final-m","session_id":"s1",
            "role":"assistant","content":"Actual verified result"})).unwrap();
        state.store.put("execution",&json!({"id":"execution-1","sessionId":"s1",
            "startedAt":"2026-11-18T00:00:00Z","status":"completed",
            "taskOutcome":"succeeded","acceptancePassed":true,
            "finalMessageId":message["id"]})).unwrap();
        let response=resume(State(state.clone()),Path("sdk-recheck".into())).await;
        assert_eq!(response.status(),StatusCode::OK);
        let saved=state.store.get("sdk_run","sdk-recheck").unwrap();
        assert_eq!(saved["status"],"completed");
        assert_eq!(saved["verified"],true);
        assert_eq!(saved["answer"],"Actual verified result");
        assert_eq!(saved["evidenceExecutionId"],"execution-1");
        let repeated=resume(State(state.clone()),Path("sdk-recheck".into())).await;
        assert_eq!(repeated.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.count_kind("execution"),1);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[tokio::test]
    async fn evidence_recheck_cannot_replay_without_confirmed_native_result(){
        let state=fixture();
        state.store.put("sdk_run",&json!({"id":"sdk-unknown","sessionId":"missing",
            "status":"needs_review","failureClass":"completion_evidence_unavailable"})).unwrap();
        let response=resume(State(state.clone()),Path("sdk-unknown".into())).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("sdk_run","sdk-unknown").unwrap()["status"],"needs_review");
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn unavailable_completion_evidence_never_grants_success_or_blind_resume(){
        let mut run=json!({"id":"sdk1","status":"running","verified":true});
        require_manual_evidence_review(&mut run);
        assert_eq!(run["status"],"needs_review");
        assert_eq!(run["verified"],false);
        assert_eq!(run["failureClass"],"completion_evidence_unavailable");
        assert!(!matches!(run["status"].as_str(),Some("failed"|"interrupted")),
            "owner must inspect unknown effects before any replay");
    }
    #[test]
    fn sdk_execution_and_frame_lookups_are_scoped_to_the_requested_run(){
        let state=fixture();
        for row in [
            json!({"id":"old","sessionId":"s1","startedAt":"2026-11-01T00:00:00Z"}),
            json!({"id":"new","sessionId":"s1","startedAt":"2026-11-18T00:00:00Z"}),
            json!({"id":"other","sessionId":"s2","startedAt":"2026-11-18T00:00:00Z"})
        ]{
            state.store.put("execution",&row).unwrap();
        }
        for frame in [
            json!({"id":"f1","parentExecutionId":"new"}),
            json!({"id":"f2","parentExecutionId":"other"}),
            json!({"id":"f3","parentExecutionId":"new"})
        ]{
            state.store.put("native_frame",&frame).unwrap();
        }
        assert_eq!(execution(&state,&json!({"sessionId":"s1"})).unwrap().unwrap()["id"],"new");
        assert_eq!(execution(&state,&json!({"sessionId":"s2"})).unwrap().unwrap()["id"],"other");
        assert!(execution(&state,&json!({"sessionId":"missing"})).unwrap().is_none());
        let frames=state.store.sdk_frames_for_execution("new").unwrap();
        assert_eq!(frames.len(),2);
        assert!(frames.iter().all(|frame|frame["parentExecutionId"]=="new"));
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn sdk_source_query_is_indexed_and_does_not_mix_tools() {
        let state=fixture();
        state.store.put("sdk_run",&json!({"id":"a","source":{"kind":"task_request",
            "id":"m1"},"status":"completed"})).unwrap();
        state.store.put("sdk_run",&json!({"id":"b","source":{"kind":"task_request",
            "id":"m2"},"status":"running"})).unwrap();
        state.store.put("sdk_run",&json!({"id":"c","source":{"kind":"other_tool",
            "id":"m1"},"status":"running"})).unwrap();
        let rows=state.store.sdk_runs_by_source("task_request",20).unwrap();
        assert_eq!(rows.len(),2);
        assert!(rows.iter().all(|r|r["source"]["kind"]=="task_request"));
        assert_eq!(state.store.sdk_runs_by_source("other_tool",20).unwrap().len(),1);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[tokio::test]
    async fn cancel_claims_live_status_without_overwriting_terminal_history(){
        let state=fixture();
        state.store.put("session",&json!({"id":"sdk-session"}));
        state.store.put("sdk_run",&json!({"id":"live","sessionId":"sdk-session",
            "status":"running","instruction":"create artifact"}));
        let response=cancel(State(state.clone()),Path("live".into())).await;
        assert_eq!(response.status(),StatusCode::OK);
        let saved=state.store.get("sdk_run","live").unwrap();
        assert_eq!(saved["status"],"cancelled");
        assert_eq!(saved["cancelRequested"],true);
        assert!(state.is_cancelled("sdk-session"));

        state.store.put("session",&json!({"id":"completed-session"}));
        state.store.put("sdk_run",&json!({"id":"done","sessionId":"completed-session",
            "status":"completed","answer":"written"}));
        let response=cancel(State(state.clone()),Path("done".into())).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("sdk_run","done").unwrap()["status"],"completed");
        assert!(!state.is_cancelled("completed-session"));
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[tokio::test]
    async fn approval_rejects_completed_runs_without_releasing_tool_waiters(){
        let state=fixture();
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("sdk_run",&json!({"id":"done","sessionId":"s",
            "status":"completed"}));
        state.store.put("approval",&json!({"id":"a","sessionId":"s",
            "status":"pending","tool":"workspace_write"}));
        let response=approve(State(state.clone()),Path("done".into()),
            Json(Approve{call_id:"a".into()})).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("approval","a").unwrap()["status"],"pending");
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn model_answer_does_not_invent_verified_task_success(){
        let reply=json!({"id":"message","content":"done"});
        let mut run=json!({"id":"sdk","verifyCommand":"cargo test"});
        finish_with_evidence(&mut run,&reply,Some(&json!({
            "id":"execution","status":"completed","taskOutcome":"answered_unverified",
            "acceptancePassed":false
        })));
        assert_eq!(run["status"],"failed");
        assert_eq!(run["verified"],false);
        assert_eq!(run["taskOutcome"],"answered_unverified");
        assert_eq!(run["answer"],"done");
        let mut accepted=json!({"id":"sdk"});
        finish_with_evidence(&mut accepted,&reply,Some(&json!({
            "id":"execution","status":"completed","taskOutcome":"succeeded",
            "acceptancePassed":true
        })));
        assert_eq!(accepted["status"],"completed");
        assert_eq!(accepted["verified"],true);
        let mut unverified=json!({"id":"sdk"});
        finish_with_evidence(&mut unverified,&reply,None);
        assert_eq!(unverified["status"],"completed");
        assert_eq!(unverified["verified"],false);
        assert!(unverified["taskOutcome"].is_null());
    }
    #[tokio::test]
    async fn reconcile_rejects_completed_and_cancelled_runs_without_mutation(){
        let state=fixture();
        for status in ["completed","cancelled"]{
            state.store.put("session",&json!({"id":format!("session-{status}")}));
            state.store.put("sdk_run",&json!({"id":status,
                "sessionId":format!("session-{status}"),"status":status}));
            state.store.put("execution",&json!({"id":format!("run-{status}"),
                "sessionId":format!("session-{status}"),"status":"interrupted"}));
            state.store.put("execution_operation",&json!({"id":format!("op-{status}"),
                "runId":format!("run-{status}"),"outcomeUnknown":true}));
            let response=reconcile(State(state.clone()),Path(status.into()),
                Json(ReconcileInput{applied:true,details:"observed".into()})).await;
            assert_eq!(response.status(),StatusCode::CONFLICT,"{status}");
            assert_eq!(state.store.get("sdk_run",status).unwrap()["status"],status);
            assert_eq!(state.store.get("execution_operation",
                &format!("op-{status}")).unwrap()["outcomeUnknown"],true);
        }
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[tokio::test]
    async fn resume_refuses_real_unreconciled_tool_effect_in_sqlite(){
        let state=fixture();
        state.store.put("session",&json!({"id":"sdk-session"}));
        state.store.put("sdk_run",&json!({
            "id":"sdk-1","sessionId":"sdk-session","status":"interrupted"
        }));
        state.store.put("execution",&json!({
            "id":"exec-1","sessionId":"sdk-session","status":"interrupted",
            "startedAt":"2026-11-17T00:00:00Z"
        }));
        state.store.put("execution_operation",&json!({
            "id":"op-1","runId":"exec-1","outcomeUnknown":true
        }));
        let snapshot=public_view(&state,state.store.get("sdk_run","sdk-1").unwrap());
        assert_eq!(snapshot["status"],"in_doubt");
        let response=resume(State(state.clone()),Path("sdk-1".into())).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("sdk_run","sdk-1").unwrap()["status"],"interrupted");
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn failed_or_interrupted_runs_show_in_doubt_until_tool_effect_is_reconciled(){
        let state=fixture();
        let running=json!({"id":"execution-1","sessionId":"sdk-session",
            "status":"interrupted","startedAt":"2026-11-17T00:00:00Z"});
        let pending_effect=json!({"id":"op-1","runId":"execution-1","outcomeUnknown":true});
        let index=ViewIndex::from_rows(vec![running.clone()],vec![],
            vec![pending_effect]);
        for status in ["failed","interrupted","running"] {
            let viewed=public_view_indexed(&state,json!({
                "id":"sdk-1","sessionId":"sdk-session","status":status
            }),&index);
            assert_eq!(viewed["status"],"in_doubt", "{status}");
            assert_eq!(viewed["runRef"]["schemaVersion"],1);
            assert_eq!(viewed["runRef"]["executionId"],"execution-1");
            assert_eq!(viewed["runRef"]["lifecycle"],"interrupted");
        }
        let completed=public_view_indexed(&state,json!({
            "id":"sdk-1","sessionId":"sdk-session","status":"completed"
        }),&index);
        assert_eq!(completed["status"],"completed",
            "a persisted terminal outcome is never rewritten by projection");
        let clear=ViewIndex::from_rows(vec![running],vec![],vec![]);
        let resumable=public_view_indexed(&state,json!({
            "id":"sdk-1","sessionId":"sdk-session","status":"interrupted"
        }),&clear);
        assert_eq!(resumable["status"],"interrupted");
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn terminal_sdk_response_is_not_masked_by_a_stale_approval(){
        let state_root=std::env::temp_dir().join(format!("sdk-evidence-{}",uuid::Uuid::new_v4()));
        let cfg=crate::config::Config{port:0,data_dir:state_root.clone(),
            web_dir:state_root.join("web"),workspace:state_root.clone()};
        let state=AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()));
        let index=ViewIndex::from_rows(vec![],vec![json!({
            "id":"approval","status":"pending","sessionId":"s"
        })],vec![]);
        let row=public_view_indexed(&state,json!({
            "id":"sdk","sessionId":"s","status":"completed","verified":true
        }),&index);
        assert_eq!(row["status"],"completed");
        let _=std::fs::remove_dir_all(state_root);
    }
    #[test]
    fn batch_view_projects_latest_execution_and_exact_uncertain_effect(){
        let index=ViewIndex::from_rows(
            vec![json!({"id":"old","sessionId":"a","startedAt":"2026-01-01","status":"completed"}),
                 json!({"id":"new","sessionId":"a","startedAt":"2026-11-16","status":"interrupted","modelCalls":4,"toolCalls":2}),
                 json!({"id":"other","sessionId":"b","startedAt":"2026-11-16","status":"running"})],
            vec![json!({"id":"ap1","sessionId":"b","status":"resolved"}),
                 json!({"id":"ap2","sessionId":"b","status":"pending","tool":"write_file"})],
            vec![json!({"runId":"old","outcomeUnknown":true}),
                 json!({"runId":"new","outcomeUnknown":false})]
        );
        assert_eq!(index.runs["a"]["id"],"new");
        assert_eq!(index.pending["b"]["id"],"ap2");
        assert!(!index.uncertain.contains("new"));
        assert!(index.uncertain.contains("old"));
        let same_session=ViewIndex::from_rows(
            vec![json!({"id":"a","sessionId":"s","startedAt":"1"}),
                 json!({"id":"b","sessionId":"s","startedAt":"2"})],
            vec![],vec![json!({"runId":"a","outcomeUnknown":true})]
        );
        assert_eq!(same_session.runs["s"]["id"],"b");
        assert!(!same_session.uncertain.contains("b"));
    }
    #[test]
    fn batch_view_handles_empty_and_legacy_records(){
        let index=ViewIndex::from_rows(vec![],vec![],vec![]);
        assert!(index.runs.is_empty());
        assert!(index.pending.is_empty());
        assert!(index.uncertain.is_empty());
    }
}
