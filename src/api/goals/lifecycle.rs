use super::*;

pub async fn list_goals(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("goal")))
}

pub async fn get_goal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("goal", &id) {
        Some(goal) => Json(goal).into_response(),
        None => error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다."),
    }
}

pub async fn delete_goal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if matches!(goal["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"진행 중인 목표를 먼저 중단하세요.");
    }
    if goal["gitWorktree"].is_object() && goal["gitWorktree"]["rolledBack"]!=true{
        return error(StatusCode::CONFLICT,"독립 Git 작업공간을 먼저 폐기한 후 목표를 삭제하세요.");
    }
    state.store.remove("goal", &id);
    state.bus.emit(json!({"type":"record","kind":"goal","removed":id}));
    Json(json!({"ok":true})).into_response()
}

pub async fn create_goal(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let text = body
        .get("goal")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "목표 내용이 필요합니다.");
    }
    let criteria = body
        .get("successCriteria")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if let Err(message) = engine::goals::validate_criteria(&criteria) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    if body.get("requireTests").is_some() && !body["requireTests"].is_boolean(){
        return error(StatusCode::BAD_REQUEST,"requireTests must be a boolean.");
    }
    let require_tests=body["requireTests"].as_bool().unwrap_or(false);
    let token_budget=match body.get("tokenBudget"){
        None|Some(Value::Null)=>Value::Null,
        Some(v)=>match v.as_u64(){
            Some(limit) if (1_000..=2_000_000).contains(&limit)=>json!(limit),
            _=>return error(StatusCode::BAD_REQUEST,
                "tokenBudget must be an integer between 1000 and 2000000."),
        },
    };
    let isolate=body["isolateWorktree"].as_bool().unwrap_or(false);
    if body.get("isolateWorktree").is_some() && !body["isolateWorktree"].is_boolean(){
        return error(StatusCode::BAD_REQUEST,"isolateWorktree must be a boolean.");
    }
    let executor = body.get("executor").cloned().unwrap_or_else(|| json!({}));
    let max_iterations = body
        .get("maxIterations")
        .cloned()
        .unwrap_or_else(|| json!(16));

    for key in ["executor","reviewer","planner"] { if body.get(key).is_some_and(|v|!v.is_object()){return error(StatusCode::BAD_REQUEST,"실행자·검토자·계획자는 객체로 지정하세요.");} }
    for (key,min,max) in [("maxIterations",1,64),("maxReplans",0,16),("replanAfter",1,8)] {if body.get(key).is_some_and(|v|v.as_u64().is_none_or(|n|n<min||n>max)){return error(StatusCode::BAD_REQUEST,"반복·재계획 한도가 유효하지 않습니다.");}}
    // A runnable goal and its job are one atomic record set. A partially
    // successful enqueue must never leave a queued goal with no worker job.
    let goal_id=uuid::Uuid::new_v4().to_string();
    let job_id=format!("goal-job-{goal_id}");
    let now=chrono::Utc::now().to_rfc3339();
    let goal=json!({
        "id":goal_id,"jobId":job_id,
        "taskId":uuid::Uuid::new_v4().to_string(),
        "goal":text,"successCriteria":criteria,"isolateWorktree":isolate,
        "requireTests":require_tests,
        "tokenBudget":token_budget,
        "executor":executor,
        "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
        "planner":body.get("planner").cloned().unwrap_or(json!({})),
        "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
        "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
        "replans":0,"planRevision":0,"appliedPlanRevision":0,"ownerPlan":null,
        "planHistory":[],"reviewHistory":[],"artifacts":[],
        "status":"queued","iterations":0,"maxIterations":max_iterations,
        "createdAt":now
    });
    let job=json!({
        "id":job_id,"kind":"goal","input":{"goalId":goal_id},
        "status":"queued","queuedAt":chrono::Utc::now().to_rfc3339_opts(
            chrono::SecondsFormat::Micros,true),
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    if let Err(cause)=state.store.transaction(&[
        ("goal".to_string(),goal.clone()),
        ("job".to_string(),job.clone())
    ]){
        state.store.audit("goal.enqueue_failed",json!({
            "goalId":goal_id,"jobId":job_id,"error":cause
        }));
        return error(StatusCode::INTERNAL_SERVER_ERROR,
            "목표와 실행 작업을 동시에 저장하지 못했습니다. 실패한 실행을 만들지 않았습니다.");
    }
    let saved=state.store.get("goal",&goal_id).unwrap_or(goal);
    let queued=state.store.get("job",&job_id).unwrap_or(job);
    state.bus.record("goal",&saved);
    state.bus.record("job",&queued);
    state.store.audit("job.queued",json!({"kind":"goal","jobId":job_id}));
    Json(saved).into_response()
}

/// Stop the durable job, not only the browser view. The goal controller
/// checks the cancellation epoch while awaiting independent model completion.
pub async fn cancel_goal(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표가 없습니다.");
    };
    if matches!(goal["status"].as_str(),Some("completed"|"cancelled"|"failed"|"needs_review")){
        return Json(json!({"id":id,"status":goal["status"]})).into_response();
    }
    if let Some(session)=goal["sessionId"].as_str(){state.cancel_session(session);}
    for job in state.store.all("job").into_iter().filter(|job|
        job["kind"]=="goal"&&job["input"]["goalId"]==id&&
        matches!(job["status"].as_str(),Some("queued"|"running"))) {
        let Some(job_id)=job["id"].as_str() else{continue};
        if let Some(saved)=state.store.patch_if("job",job_id,"status",&job["status"],
            &json!({"cancelRequested":true,"status":"cancelled",
                "finishedAt":chrono::Utc::now().to_rfc3339()})){
            state.bus.record("job",&saved);
        }
    }
    if let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"cancelled","phase":"cancelled",
            "finishedAt":chrono::Utc::now().to_rfc3339()})){
        state.bus.record("goal",&saved);
    }
    Json(json!({"id":id,"status":"cancelled"})).into_response()
}
