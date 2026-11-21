use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn list_sessions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("session")))
}

pub async fn create_session(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    // New private chats cannot overwrite a bound room/external transcript.
    let record = json!({"id":uuid::Uuid::new_v4().to_string(),"title":body["title"].as_str().filter(|title|!title.trim().is_empty()).unwrap_or("새 대화").chars().take(200).collect::<String>(),
        "agentId":body["agentId"].as_str(),"providerId":body["providerId"].as_str(),"model":body["model"].as_str(),
        "mode":if body["mode"]=="chat" || body["agentId"].is_null() {"chat"} else {"agent"},"archived":false});
    let Some(saved) = state.store.put_if_absent("session", &record) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "세션을 저장할 수 없습니다.");
    };
    state.bus.record("session", &saved);
    Json(saved).into_response()
}

pub async fn get_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("session", &id) {
        Some(record) => Json(record).into_response(),
        None => error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다."),
    }
}

// Private-chat mutations must never erase or rebind Room/channel transcripts.
// Reuse the live turn's lock so owner actions cannot race an active conversation.
fn lock_private_session(
    state: &AppState,
    id: &str,
) -> Result<(crate::state::SessionGuard, Value), Response> {
    if !state.begin_session(id) {
        return Err(error(StatusCode::CONFLICT, "진행 중인 대화가 있습니다. 완료 후 다시 시도하세요."));
    }
    let guard = state.session_guard(id);
    let Some(session) = state.store.get("session", id) else {
        return Err(error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다."));
    };
    if !session["roomId"].is_null()
        || !session["channelContext"].is_null()
        || matches!(session["kind"].as_str(), Some("room" | "channel"))
    {
        return Err(error(
            StatusCode::CONFLICT,
            "회의실·외부 채널 대화 기록은 해당 대화 화면에서 관리하세요.",
        ));
    }
    let running = state.store.active_executions_indexed().map_err(|_| {
        error(StatusCode::INTERNAL_SERVER_ERROR, "실행 중인 작업을 확인할 수 없습니다.")
    })?;
    if running.iter().any(|execution| execution["sessionId"] == id) {
        return Err(error(
            StatusCode::CONFLICT,
            "실행 중인 작업이 있습니다. 완료 후 다시 시도하세요.",
        ));
    }
    Ok((guard, session))
}

pub async fn delete_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let (_guard, _session) = match lock_private_session(&state, &id) {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    if !state.store.remove("session", &id) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "대화 삭제에 실패했습니다.");
    }
    state.store.clear_messages(&id);
    state
        .bus
        .emit(json!({ "type": "record", "kind": "session", "removed": id }));
    Json(json!({ "ok": true })).into_response()
}

pub async fn session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    Json(json!(state.store.messages(&id)))
}

pub async fn session_usage(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    Json(state.store.session_usage(&id))
}

pub async fn rename_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let (_guard, mut session) = match lock_private_session(&state, &id) {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let title = body
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return error(StatusCode::BAD_REQUEST, "제목이 비어 있습니다.");
    }
    session["title"] = json!(title);
    let saved = state.store.put("session", &session).unwrap_or(session);
    state.bus.record("session", &saved);
    Json(saved).into_response()
}

pub async fn clear_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let (_guard, _session) = match lock_private_session(&state, &id) {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    state.store.clear_messages(&id);
    state
        .bus
        .emit(json!({ "type": "record", "kind": "session", "id": id }));
    Json(json!({ "ok": true })).into_response()
}

pub async fn fork_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let (_guard, source) = match lock_private_session(&state, &id) {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let mut fork = source.clone();
    if let Value::Object(ref mut map) = fork {
        map.remove("id");
        map.remove("createdAt");
        map.remove("updatedAt");
    }
    let title = source
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("새 대화");
    fork["title"] = json!(format!("{title} (fork)"));
    let Some(fork) = state.store.put("session", &fork) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "fork 실패");
    };
    let fork_id = fork
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    for mut message in state.store.messages(&id) {
        if let Value::Object(ref mut map) = message {
            map.remove("id");
            map.insert("session_id".into(), json!(fork_id));
        }
        state.store.write_message(&message);
    }
    state.bus.record("session", &fork);
    Json(fork).into_response()
}

// ---------------------------------------------------------------- approvals

pub async fn cancel_session(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let running = state.store.has_running_execution(&id);
    state.cancel_session(&id);
    // The acknowledgement must mean more than "a flag was written".
    // Wait for the chat turn to release its same-session lock AND durable
    // execution to reach a terminal state. Otherwise a new send races the
    // previous provider HTTP request and returns "Session already running".
    // Do not hold a mutex or SQLite transaction across this async wait.
    let settled = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let active = state.active.lock().unwrap().contains(&id);
            let unfinished = state.store.has_running_execution(&id);
            if !active && !unfinished { break; }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }).await.is_ok();
    state.store.audit(
        "session.cancel",
        json!({ "sessionId": id, "running": running, "settled": settled }),
    );
    Json(json!({ "cancelled": running, "settled": settled }))
}

pub async fn compact_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    // Compaction rewrites a transcript; the Room/channel owner must control it.
    let (_guard, _session) = match lock_private_session(&state, &id) {
        Ok(locked) => locked,
        Err(response) => return response,
    };
    let provider_id = body
        .get("providerId")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let model = body
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match engine::compact::run(&state, &id, provider_id, model).await {
        Ok(result) => Json(result).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn archive_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(archived) = body["archived"].as_bool() else {
        return error(StatusCode::BAD_REQUEST, "archived:true/false가 필요합니다.");
    };
    let Some(session) = state.store.get("session", &id) else {
        return error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다.");
    };
    if session["roomId"].is_string() || session["channelContext"].is_object() {
        return error(
            StatusCode::BAD_REQUEST,
            "그룹과 외부 채널은 각 대화 화면에서 관리하세요.",
        );
    }
    let saved=state.store.patch_if("session",&id,"id",&json!(id),&json!({"archived":archived,"archivedAt":if archived{json!(chrono::Utc::now().to_rfc3339())}else{Value::Null}}));
    match saved {
        Some(saved) => {
            state.bus.record("session", &saved);
            Json(saved).into_response()
        }
        None => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "세션 보관 상태를 저장할 수 없습니다.",
        ),
    }
}

/// Queue a real, immutable evidence extraction job; no request thread claims
/// learning has completed before the worker persists its result.
pub async fn learn(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(session)=state.store.get("session",&id)else{return error(StatusCode::NOT_FOUND,"학습할 세션을 찾을 수 없습니다.")};
    if session["channelContext"]["authorIsBot"]==true{return error(StatusCode::FORBIDDEN,"봇 발언은 주인의 기억 근거로 학습하지 않습니다.")}
    let messages=state.store.messages(&id);
    if messages.is_empty(){return Json(json!({"status":"completed","result":{"memories":0,"skill":false,"reason":"no messages"},"message":"기억을 추출할 대화가 없습니다."})).into_response();}
    // A manual user action may use the model for semantic extraction. Chat
    // auto-learning stays deterministic unless a caller explicitly opts in.
    let semantic=body["semantic"].as_bool().unwrap_or(true);
    // A room shares one session but each participant has its own memory scope.
    // Never reuse the last speaker's session provider for the whole room.
    if let Some(room_id)=session["roomId"].as_str(){
        let Some(room)=state.store.get("room",room_id)else{return error(StatusCode::NOT_FOUND,"회의실을 찾을 수 없습니다.")};
        let agents=room["agentIds"].as_array().cloned().unwrap_or_default();
        if agents.is_empty(){return error(StatusCode::BAD_REQUEST,"학습할 참가자가 없습니다.")}
        let explicit_provider=body["providerId"].as_str().filter(|value|!value.is_empty());
        if let Some(provider)=explicit_provider{if state.store.get("provider",provider).is_none_or(|p|p["enabled"]==false){return error(StatusCode::BAD_REQUEST,"사용 가능한 학습 제공자를 선택하세요.")}}
        let explicit_model=body["model"].as_str().filter(|value|!value.is_empty());
        let mut jobs=Vec::new();
        for agent_id in agents.iter().filter_map(Value::as_str){
            if state.store.get("agent",agent_id).is_none(){continue}
            let through=messages.iter().rev().find(|message|message["agent_id"].as_str()==Some(agent_id)).and_then(|message|message["id"].as_str());
            let Some(through)=through else{continue};
            let queued=if semantic {
                engine::learning::enqueue_semantic_for_agent_through(&state,&id,Some(agent_id),explicit_provider.unwrap_or(""),explicit_model.unwrap_or(""),Some(through))
            } else {
                engine::learning::enqueue_for_agent_through(&state,&id,Some(agent_id),explicit_provider.unwrap_or(""),explicit_model.unwrap_or(""),Some(through))
            };
            if let Some(job)=queued{
                jobs.push(json!({"agentId":agent_id,"jobId":job["id"],"status":job["status"]}));
            }
        }
        if jobs.is_empty(){return Json(json!({"status":"completed","result":{"memories":0,"skill":false,"reason":"already learned or no agent messages"},"message":"참가자별로 새로 학습할 발언이 없습니다."})).into_response()}
        let first=jobs[0]["jobId"].clone();
        let first_job=first.as_str().and_then(|job_id|state.store.get("job",job_id));
        return (StatusCode::ACCEPTED,Json(json!({"status":"queued","jobs":jobs,"count":jobs.len(),"jobId":first,"job":first_job}))).into_response();
    }
    let provider=body["providerId"].as_str().or(session["providerId"].as_str()).unwrap_or("");
    if !provider.is_empty()&&state.store.get("provider",provider).is_none_or(|p|p["enabled"]==false){return error(StatusCode::BAD_REQUEST,"사용 가능한 학습 제공자를 선택하세요.")}
    let model=body["model"].as_str().or(session["model"].as_str()).unwrap_or("");
    let queued=if semantic {
        engine::learning::enqueue_semantic(&state,&id,provider,model)
    } else {
        engine::learning::enqueue(&state,&id,provider,model)
    };
    match queued{
        Some(job)=>(StatusCode::ACCEPTED,Json(json!({"jobId":job["id"],"status":job["status"],"job":job}))).into_response(),
        None=>{
            let through=messages.last().and_then(|m|m["id"].as_str());
            if state.store.all("learning_checkpoint").iter().any(|c|c["sessionId"]==id&&c["throughMessageId"].as_str()==through){
                Json(json!({"status":"completed","result":{"memories":0,"skill":false,"reason":"already learned"},"message":"저장된 대화까지 이미 기억을 추출했습니다."})).into_response()
            }else{error(StatusCode::INTERNAL_SERVER_ERROR,"기억 추출 작업을 저장할 수 없습니다.")}
        }
    }
}
