use super::*;

/// Owner-approved recovery of a failed/review-blocked goal. A fresh job
/// continues after the last completed iteration; it never replays the old
/// job's uncertain in-flight tool call or assumes external writes succeeded.
pub async fn resume_goal(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response {
    if body.as_object().is_none_or(|object|object.len()!=1
        || object.get("acknowledgeUnknownEffects")!=Some(&json!(true))) {
        return error(StatusCode::BAD_REQUEST,
            "이전 파일·외부 작업 결과를 확인했다면 acknowledgeUnknownEffects=true를 보내세요.");
    }
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(),Some("needs_review"|"failed")) {
        return error(StatusCode::CONFLICT,"검토 대기 또는 실패한 목표만 이어갈 수 있습니다.");
    }
    if state.store.all("job").iter().any(|job|job["kind"]=="goal"
        && job["input"]["goalId"]==id
        && matches!(job["status"].as_str(),Some("queued"|"running"))) {
        return error(StatusCode::CONFLICT,"이전 목표 작업이 아직 실행 중입니다.");
    }
    let iterations=goal["iterations"].as_u64().unwrap_or(0);
    let old_max=goal["maxIterations"].as_u64().unwrap_or(16).clamp(1,64);
    let next_max=if old_max<=iterations {old_max.saturating_add(8).min(64)}else{old_max};
    if next_max<=iterations {
        return error(StatusCode::CONFLICT,"목표 반복 한도 64회에 도달했습니다.");
    }
    let next=goal["resumeCount"].as_u64().unwrap_or(0).saturating_add(1);
    if next>12 {
        return error(StatusCode::CONFLICT,"검토 후 이어가기 횟수 한도에 도달했습니다.");
    }
    let old_job_id=goal["jobId"].as_str().map(str::to_string)
        .or_else(||state.store.all("job").into_iter()
            .find(|j|j["kind"]=="goal"&&j["input"]["goalId"]==id)
            .and_then(|j|j["id"].as_str().map(str::to_string)));
    let old_job=old_job_id.as_deref()
        .and_then(|job_id|state.store.get("job",job_id))
        .unwrap_or(Value::Null);
    let old_checkpoint=&old_job["goalState"];
    // A tool call whose result was unknown at the interruption boundary
    // must not be repeated as if it had never run.
    let interrupted=old_checkpoint["inFlight"]==true;
    let unfinished=old_checkpoint["nextIteration"].as_u64().unwrap_or(iterations);
    let next_iteration=iterations.max(if interrupted {
        unfinished.saturating_add(1)
    }else{unfinished});
    if next_iteration>=next_max {
        return error(StatusCode::CONFLICT,"이전 실행 단계가 반복 한도를 모두 사용했습니다.");
    }
    let reviewed_unknown_ids=if let Some(session)=goal["sessionId"].as_str(){
        match state.store.uncertain_session_operation_ids(session){
            Ok(ids) if ids.len()<=256=>ids,
            Ok(_)=>return error(StatusCode::CONFLICT,
                "검토 대상 미확정 작업이 너무 많아 이어갈 수 없습니다."),
            Err(reason)=>return error(StatusCode::INTERNAL_SERVER_ERROR,
                &format!("미확정 작업 영수증 조회 실패: {reason}"))
        }
    }else{Vec::new()};
    let new_job_id=format!("goal-job-{id}-resume-{next}");
    let mut history=goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"jobId":new_job_id,"previousJobId":old_job_id,
        "previousStatus":goal["status"],"startIteration":next_iteration,
        "acknowledgedBy":"owner","at":chrono::Utc::now().to_rfc3339()}));
    // The new durable worker starts after all completed iterations. Previous
    // inFlight/reviewPending flags remain on the old job as forensic evidence.
    let checkpoint=json!({
        "sessionId":goal["sessionId"],
        "nextIteration":next_iteration,"inFlight":false,"reviewPending":false,
        "appliedPlanRevision":goal["appliedPlanRevision"].as_u64().unwrap_or(0),
        "evidence":goal.get("evidence").cloned()
            .filter(|v|v.is_array()).unwrap_or_else(||json!([])),
        "transcript":old_checkpoint["transcript"].as_str().unwrap_or(""),
        "gaps":goal.get("gaps").cloned()
            .filter(|v|v.is_array()).unwrap_or_else(||json!([])),
        "stalled":0,"resumeAfterReview":true,
        "reviewedUnknownOpIds":reviewed_unknown_ids
    });
    let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"queued","phase":"resume","jobId":new_job_id,
            "resumeCount":next,"maxIterations":next_max,"resumeHistory":history,
            "finishedAt":null,"error":null,"finalReport":null})) else {
        return error(StatusCode::CONFLICT,"목표가 다른 요청에서 변경됐습니다.");
    };
    // Clear a prior session cancellation before the queued worker can start.
    if let Some(session)=saved["sessionId"].as_str(){state.clear_cancel(session);}
    let created=crate::engine::jobs::enqueue_goal_resume(
        &state,&new_job_id,&id,checkpoint,next_iteration,next_max);
    if created.is_none() {
        let restored=state.store.patch_if("goal",&id,"jobId",&json!(new_job_id),
            &json!({"status":"needs_review","phase":"resume_enqueue_failed",
                "error":"이어가기 실행 작업을 등록하지 못했습니다."}));
        if let Some(restored)=restored {state.bus.record("goal",&restored);}
        return error(StatusCode::INTERNAL_SERVER_ERROR,"이어가기 작업을 등록하지 못했습니다.");
    }
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_reviewed_resume",json!({
        "goalId":id,"jobId":new_job_id,"previousJobId":old_job_id,
        "startIteration":next_iteration,
        "reviewedUnknownOperationIdsCount":reviewed_unknown_ids.len()
    }));
    Json(saved).into_response()
}

/// Versioned steering; edits become effective at a safe executor turn boundary.
pub async fn steer_goal(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response {
    use crate::engine::goal_steering::{submit,submit_instruction,SteeringError};
    // Both first-party SDKs share the same native endpoint as the UI.
    // Versioned plan replacement: {plan,expectedRevision}
    // Idempotent owner directive: {text,mode,requestId}
    let result=match body.as_object(){
        Some(fields) if fields.len()==2
            && fields.contains_key("expectedRevision")&&fields.contains_key("plan")=>{
            let Some(revision)=body["expectedRevision"].as_u64() else{
                return error(StatusCode::BAD_REQUEST,"expectedRevision은 정수여야 합니다.");
            };
            let Some(plan)=body["plan"].as_str() else{
                return error(StatusCode::BAD_REQUEST,"plan은 문자열이어야 합니다.");
            };
            submit(&state,&id,revision,plan)
        },
        Some(fields) if fields.len()==3
            && fields.contains_key("text")&&fields.contains_key("mode")
            && fields.contains_key("requestId")=>{
            let (Some(text),Some(mode),Some(request_id))=(
                body["text"].as_str(),body["mode"].as_str(),body["requestId"].as_str()
            )else{
                return error(StatusCode::BAD_REQUEST,"text, mode, requestId는 문자열이어야 합니다.");
            };
            submit_instruction(&state,&id,text,mode,request_id)
        },
        _=>return error(StatusCode::BAD_REQUEST,
            "plan+expectedRevision 또는 text+mode+requestId가 필요합니다."),
    };
    match result{
        Ok(value)=>Json(value).into_response(),
        Err(SteeringError::Missing)=>error(StatusCode::NOT_FOUND,"목표가 없습니다."),
        Err(SteeringError::Immutable)=>error(StatusCode::CONFLICT,"종료된 목표는 수정할 수 없습니다."),
        Err(SteeringError::Conflict)=>error(StatusCode::CONFLICT,"계획 버전이 달라졌습니다. 최신 내용을 불러오세요."),
        Err(SteeringError::Invalid)=>error(StatusCode::BAD_REQUEST,"계획은 1~6000자여야 합니다."),
    }
}
