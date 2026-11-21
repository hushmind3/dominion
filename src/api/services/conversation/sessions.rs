use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn list_sessions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("session").into_iter()
        .filter(|session|session["kind"]!="room_worker").collect::<Vec<_>>()))
}

pub async fn create_session(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    // New private chats cannot overwrite a bound room/external transcript.
    let record = json!({"id":uuid::Uuid::new_v4().to_string(),"title":body["title"].as_str().filter(|title|!title.trim().is_empty()).unwrap_or("새 대화").chars().take(200).collect::<String>(),
        "agentId":body["agentId"].as_str(),"providerId":body["providerId"].as_str(),"model":body["model"].as_str(),
        "conversationMode":body["conversationMode"].as_str().filter(|mode|matches!(*mode,"chat"|"agent")),"archived":false});
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

pub async fn delete_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(session) = state.store.get("session", &id) else {
        return error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다.");
    };
    // A Room or channel owns its transcript. Deleting it through the private
    // Chat endpoint would detach live work and revoke another owner's files.
    if !session["roomId"].is_null() || !session["channelContext"].is_null()
        || session["kind"] == "room_worker" {
        return error(StatusCode::CONFLICT, "그룹 또는 외부 채널 대화는 해당 화면에서 관리하세요.");
    }
    let running = state.active.lock().unwrap().contains(&id)
        || state.store.all("execution").iter().any(|run| {
            run["sessionId"].as_str() == Some(id.as_str()) && run["status"] == "running"
        });
    if running {
        return error(StatusCode::CONFLICT, "실행 중인 대화입니다. 중지 완료 후 삭제하세요.");
    }
    if let Err(message) = engine::attachments::purge_session(&state, &id) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, &message);
    }
    if !state.store.remove("session", &id) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "대화 기록을 삭제하지 못했습니다.");
    }
    state.store.clear_messages(&id);
    state.bus.emit(json!({ "type": "record", "kind": "session", "removed": id }));
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
    let mut summary=state.store.session_usage(&id);
    summary["breakdown"]=state.store.session_usage_breakdown(&id);
    Json(summary)
}

pub async fn rename_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(mut session) = state.store.get("session", &id) else {
        return error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다.");
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

pub async fn clear_session(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.clear_messages(&id);
    state.store.audit("chat.session_cleared",json!({"sessionId":id}));
    state
        .bus
        .emit(json!({ "type": "record", "kind": "session", "id": id }));
    Json(json!({ "ok": true }))
}

pub async fn fork_session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(source) = state.store.get("session", &id) else {
        return error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다.");
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

/// The browser may issue Stop before its /api/chat POST reaches the Rust
/// runtime. Reserve an immutable UUID stop tombstone first, then cancel the
/// active session epoch. Old /stop clients can continue sending an empty body.
pub async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<Value>>,
) -> Json<Value> {
    let request_id=body.as_ref()
        .and_then(|Json(body)|body["requestId"].as_str())
        .filter(|id|uuid::Uuid::parse_str(id).is_ok());
    let running = state.store.all("execution").iter().any(|execution| {
        execution.get("sessionId").and_then(|value| value.as_str()) == Some(id.as_str())
            && execution.get("status").and_then(|value| value.as_str()) == Some("running")
    });
    let mut request_recorded=false;
    if let Some(request_id)=request_id {
        // A cancellation marker is scoped to one session and one UI turn.
        // Never let an external UUID become a way to cancel a different chat.
        let existing=state.store.get("chat_cancel_request",request_id);
        if existing.as_ref().is_none_or(|record|record["sessionId"]==id) {
            request_recorded=state.store.put("chat_cancel_request",&json!({
                "id":request_id,"sessionId":id,"cancelled":true,
                "cancelledAt":chrono::Utc::now().to_rfc3339()
            })).is_some();
        }
    }
    state.cancel_session(&id);
    state.store.audit(
        "session.cancel",
        json!({ "sessionId": id, "running": running,
            "requestId":request_id,"requestRecorded":request_recorded }),
    );
    Json(json!({ "cancelled": running||request_recorded,
        "running":running,"requestRecorded":request_recorded }))
}

/// One process may release its active session before a detached provider
/// response is fully recorded. The UI must keep Stop active until both the
/// live session guard and the durable execution journal are terminal.
pub async fn cancel_status(
    State(state): State<AppState>,Path(id):Path<String>
) -> Json<Value> {
    let active=state.active.lock().unwrap().contains(&id);
    let running=state.store.all("execution").iter().any(|execution|{
        execution["sessionId"].as_str()==Some(id.as_str())
            && execution["status"]=="running"
    });
    Json(json!({"sessionId":id,"stopped":!active&&!running,
        "active":active,"running":running}))
}

pub async fn compact_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
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


#[cfg(test)]
mod deletion_tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    fn fixture() -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-delete-chat-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        (state, root)
    }

    #[tokio::test]
    async fn deleted_private_chat_removes_file_bytes_and_record_but_preserves_other_session() {
        let (state, root) = fixture();
        state.store.put("session", &json!({"id":"one"}));
        state.store.put("session", &json!({"id":"two"}));
        let one = engine::attachments::upload(&state, &json!({
            "sessionId":"one", "name":"private.txt", "data":STANDARD.encode("private bytes")
        })).unwrap();
        let two = engine::attachments::upload(&state, &json!({
            "sessionId":"two", "name":"other.txt", "data":STANDARD.encode("other bytes")
        })).unwrap();
        let one_id = one["id"].as_str().unwrap().to_string();
        let two_id = two["id"].as_str().unwrap().to_string();
        let disk = root.join("attachments").join(format!("{one_id}.bin"));
        assert!(disk.exists());
        let response = delete_session(State(state.clone()), Path("one".into())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.store.get("session", "one").is_none());
        assert!(state.store.get("attachment", &one_id).is_none());
        assert!(!disk.exists(), "private file bytes must be erased after deleting chat");
        assert!(engine::attachments::read(&state, &one_id).is_err());
        assert!(engine::attachments::read(&state, &two_id).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn deletion_refuses_active_chat_and_room_owned_transcripts() {
        let (state, root) = fixture();
        state.store.put("session", &json!({"id":"active"}));
        state.store.put("session", &json!({"id":"group","roomId":"room-1"}));
        assert!(state.begin_session("active"));
        let busy = delete_session(State(state.clone()), Path("active".into())).await;
        assert_eq!(busy.status(), StatusCode::CONFLICT);
        assert!(state.store.get("session", "active").is_some());
        state.end_session("active");
        let group = delete_session(State(state.clone()), Path("group".into())).await;
        assert_eq!(group.status(), StatusCode::CONFLICT);
        assert!(state.store.get("session", "group").is_some());
        let _ = std::fs::remove_dir_all(root);
    }
}
