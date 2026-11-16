//! SDK, chat, rooms, jobs and workflows share the native Rust execution path.
//! SQLite owns records, approvals, effects and native sub4 checkpoints.
use crate::{engine::{self,chat::TurnRequest,gateway},state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use super::response::error;
use std::collections::{HashMap,HashSet};

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Start{instruction:String,provider_id:Option<String>,model:Option<String>,verify_command:Option<String>,max_rounds:Option<usize>,#[serde(flatten)] options:Value}
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

fn execution(state:&AppState,record:&Value)->Option<Value>{
    let session=record["sessionId"].as_str()?;
    state.store.all("execution").into_iter().filter(|e|e["sessionId"].as_str()==Some(session))
        .max_by_key(|e|e["startedAt"].as_str().unwrap_or("").to_owned())
}
/// A consistent read snapshot for SDK listings: collect executions,
/// approvals and uncertain effects once rather than scanning them per run.
/// The single-run response reuses this same projection contract.
struct ViewIndex {
    runs:HashMap<String,Value>,
    pending:HashMap<String,Value>,
    uncertain:HashSet<String>,
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
        let uncertain=operations.into_iter().filter(|op|op["outcomeUnknown"]==true)
            .filter_map(|op|op["runId"].as_str().map(str::to_string)).collect();
        Self{runs,pending,uncertain}
    }
    fn load(state:&AppState)->Self{
        Self::from_rows(state.store.all("execution"),
            state.store.all("approval"),state.store.all("execution_operation"))
    }
}
fn public_view_indexed(state:&AppState,mut record:Value,index:&ViewIndex)->Value{
    let Some(session)=record["sessionId"].as_str().map(str::to_owned)else{
        record["readOnly"]=json!(true);return record;
    };
    let pending=index.pending.get(&session);
    let run=index.runs.get(&session);
    if matches!(record["status"].as_str(),Some("running"|"queued")) {
      if let Some(approval)=pending{
        record["status"]=json!("awaiting_approval");
        record["pendingApprovalId"]=approval["id"].clone();
    }else if run.is_some_and(|run|run["id"].as_str().is_some_and(|id|index.uncertain.contains(id))){
        record["status"]=json!("in_doubt");
    }else if record["status"]=="running"
        && run.is_some_and(|run|run["status"]=="interrupted")
        && !state.active.lock().unwrap().contains(&session){
        record["status"]=json!("interrupted");
      }
    }
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
            "cached_prompt_tokens":m["metadata"]["usage"]["cachedPromptTokens"]
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
        record["runtime"]=json!("dominion-core");
    }
    record
}
fn public_view(state:&AppState,record:Value)->Value{
    public_view_indexed(state,record,&ViewIndex::load(state))
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    let index=ViewIndex::load(&state);
    Json(json!(state.store.all("sdk_run").into_iter()
        .map(|record|public_view_indexed(&state,record,&index)).collect::<Vec<_>>()))
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    state.store.get("sdk_run",&id).map(|r|Json(public_view(&state,r)).into_response()).unwrap_or_else(||error(StatusCode::NOT_FOUND,"작업이 없습니다."))
}
pub async fn start(State(state):State<AppState>,Json(input):Json<Start>)->Response{
    if input.instruction.trim().is_empty()||input.instruction.len()>64000{return error(StatusCode::BAD_REQUEST,"작업 지시는 1~64000자로 입력하세요.");}
    if input.verify_command.as_ref().is_some_and(|c|c.trim().is_empty()||c.len()>2048){return error(StatusCode::BAD_REQUEST,"검증 명령의 길이를 확인하세요.");}
    let route=match gateway::resolve(&state,input.provider_id.as_deref().unwrap_or(""),input.model.as_deref()){Ok(r)=>r,Err(e)=>return error(StatusCode::BAD_REQUEST,&e)};
    let options=match engine::job_control::options(&state,&input.options){Ok(value)=>value,Err(e)=>return error(StatusCode::BAD_REQUEST,&e)};
    let id=uuid::Uuid::new_v4().to_string();let session=uuid::Uuid::new_v4().to_string();
    if state.store.put("session",&json!({"id":session,"title":input.instruction.chars().take(40).collect::<String>(),"providerId":route.id,"model":route.model,"nativeSdkRunId":id,"nativeSdkMaxRounds":input.max_rounds.unwrap_or(24).clamp(1,80),"nativeSdkVerifyCommand":input.verify_command})).is_none(){return error(StatusCode::INTERNAL_SERVER_ERROR,"작업 세션 저장에 실패했습니다.");}
    if let Some(mut saved)=state.store.get("session",&session){
        for (key,value) in options.as_object().unwrap(){saved[key]=value.clone();}
        saved["budgetOwnerId"]=json!(format!("sdk:{id}"));
        state.store.put("session",&saved);
    }
    let record=json!({"id":id,"sessionId":session,"status":"queued","instruction":input.instruction,"providerId":route.id,"model":route.model,"runtime":"dominion-core","verifyCommand":input.verify_command,"maxRounds":input.max_rounds.unwrap_or(24).clamp(1,80)});
    let Some(saved)=state.store.put("sdk_run",&record)else{return error(StatusCode::INTERNAL_SERVER_ERROR,"작업 저장에 실패했습니다.");};
    launch(state.clone(),saved.clone(),false);
    (StatusCode::ACCEPTED,Json(public_view(&state,saved))).into_response()
}
fn launch(state:AppState,record:Value,resume:bool){
    tokio::spawn(async move{
        let id=record["id"].as_str().unwrap_or("");
        if let Some(saved)=state.store.patch_if("sdk_run",id,"status",&record["status"],&json!({"status":"running"})){state.bus.record("sdk_run",&saved);}else{return;}
        let control=state.store.get("sdk_run",id).unwrap_or_else(||record.clone());
        let goal=engine::job_control::intent(record["instruction"].as_str().unwrap_or(""),&control);
        if let Some(session)=record["sessionId"].as_str(){state.store.patch_if("session",session,"id",&json!(session),&json!({"ownerAppliedRevision":control["steeringRevision"]}));}
        let request=TurnRequest::from_value(&json!({"sessionId":record["sessionId"],"input":goal,"providerId":record["providerId"],"model":record["model"],"autoLearning":false}));
        let outcome=match request{Ok(request)=>if resume{engine::run_quiet_resume(&state,&request).await}else{engine::run_quiet(&state,&request).await},Err(e)=>Err(e)};
        let Some(current)=state.store.get("sdk_run",id)else{return;};
        if current["status"]!="running"{return;}
        let mut finalized=current.clone();
        match outcome {
            Ok(Some(message))=>{
                let run=execution(&state,&record);
                finish_with_evidence(&mut finalized,&message,run.as_ref());
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
    let Some(approval)=state.store.get("approval",&input.call_id)else{return error(StatusCode::CONFLICT,"승인 요청을 찾을 수 없습니다.");};
    if approval["sessionId"]!=record["sessionId"]||approval["status"]!="pending"{return error(StatusCode::CONFLICT,"이 작업의 대기 중인 승인이 아닙니다.");}
    let _=super::approvals::resolve_approval(State(state.clone()),Json(json!({"id":input.call_id,"allow":true}))).await;
    Json(public_view(&state,record)).into_response()
}
pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(mut record)=state.store.get("sdk_run",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.");};
    if matches!(record["status"].as_str(),Some("completed"|"cancelled")){return error(StatusCode::CONFLICT,"종료된 작업은 중단할 수 없습니다.");}
    if let Some(session)=record["sessionId"].as_str(){state.cancel_session(session);}
    record["status"]=json!("cancelled");
    state.store.put("sdk_run",&record).map(|r|Json(public_view(&state,r)).into_response()).unwrap_or_else(||error(StatusCode::INTERNAL_SERVER_ERROR,"중단 상태 저장에 실패했습니다."))
}
pub async fn resume(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(record)=state.store.get("sdk_run",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.");};
    if record["sessionId"].as_str().is_none(){return error(StatusCode::CONFLICT,"구형 하네스 기록은 읽기 전용입니다. 새 공통 코어 작업으로 시작하세요.");}
    if !matches!(record["status"].as_str(),Some("failed"|"interrupted")){return error(StatusCode::CONFLICT,"재개 가능한 중단 상태가 아닙니다.");}
    if public_view(&state,record.clone())["status"]=="in_doubt"{return error(StatusCode::CONFLICT,"불명확한 도구 효과를 먼저 확인하세요.");}
    launch(state.clone(),record.clone(),true);(StatusCode::ACCEPTED,Json(public_view(&state,record))).into_response()
}
pub async fn reconcile(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<ReconcileInput>)->Response{
    if input.details.trim().is_empty()||input.details.len()>8192{return error(StatusCode::BAD_REQUEST,"실제 확인한 결과를 1~8192자로 입력하세요.");}
    let Some(mut record)=state.store.get("sdk_run",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.");};
    let Some(session)=record["sessionId"].as_str()else{return error(StatusCode::CONFLICT,"구형 하네스 기록은 읽기 전용입니다.");};
    if state.active.lock().unwrap().contains(session){return error(StatusCode::CONFLICT,"실행 중에는 복구 결과를 변경할 수 없습니다.");}
    let Some(run)=execution(&state,&record)else{return error(StatusCode::CONFLICT,"복구할 실행 기록이 없습니다.");};
    let pending=state.store.all("execution_operation").into_iter().filter(|op|op["runId"]==run["id"]&&op["outcomeUnknown"]==true).collect::<Vec<_>>();
    if pending.len()!=1{return error(StatusCode::CONFLICT,"효과가 불명확한 도구 하나만 수동 확인할 수 있습니다.");}
    let mut op=pending[0].clone();op["outcomeUnknown"]=json!(false);op["effectState"]=json!(if input.applied{"applied"}else{"not_applied"});
    op["operatorObservation"]=json!({"applied":input.applied,"details":input.details,"observedAt":chrono::Utc::now().to_rfc3339()});
    record["status"]=json!("interrupted");let mut observations=record["observations"].as_array().cloned().unwrap_or_default();
    observations.push(json!({"kind":"operator_observation","detail":op["operatorObservation"]}));record["observations"]=json!(observations);
    let mut updates=vec![("execution_operation",op.clone()),("sdk_run",record.clone())];
    let mut checkpoints=vec![("execution",run.clone())];
    checkpoints.extend(state.store.all("native_frame").into_iter().filter(|frame|frame["parentExecutionId"]==run["id"]).map(|frame|("native_frame",frame)));
    for (kind,mut saved) in checkpoints{
        let Ok(mut snapshot)=serde_json::from_value::<dominion_core::hosted::Snapshot>(saved["kernelState"].clone())else{continue;};
        if let Some(call)=snapshot.native.in_flight.clone().filter(|call|call.name==op["tool"]&&call.arguments==op["arguments"]){
            let observation=if input.applied{Ok(input.details.clone())}else{Err(input.details.clone())};
            if let Err(message)=dominion_core::journal::finish(&mut snapshot.native,&call,&observation,true){return error(StatusCode::CONFLICT,&message);}
            saved["kernelState"]=serde_json::to_value(snapshot).unwrap_or(Value::Null);updates.push((kind,saved));
        }
    }
    if let Err(message)=state.store.put_native_observation(&updates){return error(StatusCode::INTERNAL_SERVER_ERROR,&message);}
    if let Err(message)=engine::execution_budget::refresh_after_observation(&state,run["id"].as_str().unwrap_or("")) { return error(StatusCode::INTERNAL_SERVER_ERROR,&message); }
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
