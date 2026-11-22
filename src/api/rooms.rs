//! Owner-centered room API. Room sessions and worker checkpoints are server
//! state; clients can change the roster and policy, never rebind a transcript.
use crate::engine::{memory, rooms};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub(crate) type ApiResult = Result<Value, (StatusCode, String)>;
fn response(result: ApiResult) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err((status, error)) => (status, Json(json!({"error":error}))).into_response(),
    }
}
fn bad(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message.into())
}
fn failed(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, message.into())
}
fn conflict(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::CONFLICT, message.into())
}
fn room(state: &AppState, id: &str) -> Result<Value, (StatusCode, String)> {
    state
        .store
        .get("room", id)
        .ok_or((StatusCode::NOT_FOUND, "회의실을 찾을 수 없습니다.".into()))
}
fn lock(state: &AppState, id: &str) -> Result<crate::state::SessionGuard, (StatusCode, String)> {
    let key = format!("room-submit:{id}");
    if !state.begin_session(&key) {
        return Err(conflict("회의실 상태가 변경 중입니다. 다시 시도하세요."));
    }
    Ok(state.session_guard(&key))
}
fn text_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: &str,
    limit: usize,
) -> Result<String, (StatusCode, String)> {
    let value = body.get(key).or_else(|| previous.get(key));
    let text = match value {
        Some(value) => value
            .as_str()
            .ok_or_else(|| bad(format!("{key}는 문자열이어야 합니다.")))?,
        None => default,
    }
    .trim();
    if text.chars().count() > limit {
        return Err(bad(format!("{key}는 {limit}자 이하여야 합니다.")));
    }
    Ok(text.into())
}
fn bool_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: bool,
) -> Result<bool, (StatusCode, String)> {
    body.get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| bad(format!("{key}는 true/false여야 합니다.")))
        })
        .unwrap_or(Ok(default))
}
fn number_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: u64,
    max: u64,
) -> Result<u64, (StatusCode, String)> {
    let number = body
        .get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| bad(format!("{key}는 양의 정수여야 합니다.")))
        })
        .unwrap_or(Ok(default))?;
    if number == 0 || number > max {
        return Err(bad(format!("{key} 범위는 1~{max}입니다.")));
    }
    Ok(number)
}
fn number_field_allow_zero(
    body: &Value,
    previous: &Value,
    key: &str,
    max: u64,
) -> Result<u64, (StatusCode, String)> {
    let number = body
        .get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| bad(format!("{key}는 0 이상의 정수여야 합니다.")))
        })
        .unwrap_or(Ok(0))?;
    if number > max {
        return Err(bad(format!("{key} 범위는 0~{max}입니다.")));
    }
    Ok(number)
}
fn validated_config(state: &AppState, id: &str, previous: &Value, body: &Value) -> ApiResult {
    if !body.is_object() {
        return Err(bad("회의실 객체가 필요합니다."));
    }
    if let Some(binding) = body.get("sessionId") {
        if binding != &previous["sessionId"] {
            return Err(bad("회의실의 대화 세션은 변경할 수 없습니다."));
        }
    }
    for reserved in [
        "roomState",
        "activeAgentId",
        "status",
        "generation",
        "cancelRequested",
        "ownerId",
        "partitionKey",
    ] {
        if body.get(reserved).is_some() {
            return Err(bad(format!("{reserved}는 서버가 관리하는 상태입니다.")));
        }
    }
    let name = text_field(body, previous, "name", "새 회의실", 160)?;
    if name.is_empty() {
        return Err(bad("회의실 이름이 필요합니다."));
    }
    // A room can be created without picking members. The owner should not have
    // to build a roster before saying hello, so an empty request fills itself
    // with up to three saved assistants.
    let requested = body.get("agentIds").or_else(|| previous.get("agentIds"));
    let filled: Vec<Value>;
    let source = match requested.and_then(Value::as_array).filter(|ids| !ids.is_empty()) {
        Some(ids) => ids,
        None => {
            filled = state
                .store
                .all("agent")
                .into_iter()
                .take(3)
                .map(|agent| agent["id"].clone())
                .collect();
            if filled.is_empty() {
                return Err(bad("먼저 도우미를 한 명 만들어 주세요."));
            }
            &filled
        }
    };
    if source.len() > 32 {
        return Err(bad("참가자는 32명까지 가능합니다."));
    }
    let mut known = HashSet::new();
    let mut agent_ids = Vec::new();
    for value in source {
        let id = value
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| bad("유효한 에이전트 ID가 필요합니다."))?;
        if state.store.get("agent", id).is_none() {
            return Err(bad(format!("참가 에이전트를 찾을 수 없습니다: {id}")));
        }
        if known.insert(id.to_string()) {
            agent_ids.push(id.to_string());
        }
    }
    let routing = text_field(body, previous, "routingMode", "discussion", 32)?;
    if !matches!(routing.as_str(), "discussion" | "round_robin" | "mentioned") {
        return Err(bad("지원하지 않는 발언 방식입니다."));
    }
    // Owner-picked model pool. FreeLLMAPI exposes dozens of models; the owner
    // checks the ones they trust once and every turn rotates inside that list.
    let models_source = body
        .get("models")
        .or_else(|| previous.get("models"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if models_source.len() > 64 {
        return Err(bad("방 모델은 최대 64개까지 선택할 수 있습니다."));
    }
    let mut models = Vec::new();
    let mut seen_models = HashSet::new();
    for entry in &models_source {
        let provider_id = entry
            .get("providerId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad("방 모델에 providerId가 필요합니다."))?;
        let model = entry
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 200)
            .ok_or_else(|| bad("방 모델에 model 이름이 필요합니다."))?;
        state
            .store
            .get("provider", provider_id)
            .ok_or_else(|| bad(format!("제공자를 찾을 수 없습니다: {provider_id}")))?;
        let effort = entry
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !valid_effort(effort) {
            return Err(bad("추론 강도 값이 올바르지 않습니다."));
        }
        if seen_models.insert(format!("{provider_id}|{model}")) {
            let mut picked = json!({"providerId":provider_id,"model":model});
            if !effort.is_empty() {
                picked["reasoningEffort"] = json!(effort);
            }
            models.push(picked);
        }
    }
    let max_turns = body
        .get("maxTurns")
        .and_then(Value::as_i64)
        .or_else(|| previous.get("maxTurns").and_then(Value::as_i64))
        .unwrap_or(0);
    if !(0..=100_000).contains(&max_turns) {
        return Err(bad("총 AI 발언 상한은 0~100000 사이여야 합니다."));
    }
    let reasoning_effort=text_field(body,previous,"reasoningEffort","",16)?;
    if !valid_effort(&reasoning_effort) {
        return Err(bad("추론 강도 값이 올바르지 않습니다."));
    }
    Ok(json!({
        "id":id,"name":name,"agentIds":agent_ids,"routingMode":routing,
        "description":text_field(body,previous,"description","",4000)?,
        "topic":text_field(body,previous,"topic","",16000)?,
        "ownerName":text_field(body,previous,"ownerName","나",100)?,
        "rounds":number_field(body,previous,"rounds",3,100_000)?,
        "maxTurns":max_turns,
        "tokenBudget":number_field_allow_zero(body,previous,"tokenBudget",10_000_000)?,
        "contextBudgetTokens":number_field(body,previous,"contextBudgetTokens",2_000,1_000_000)?,
        "toolsEnabled":bool_field(body,previous,"toolsEnabled",false)?,
        "autoLearning":bool_field(body,previous,"autoLearning",true)?,
        "autoModels":bool_field(body,previous,"autoModels",true)?,
        "reasoningEffort":reasoning_effort,
        "models":models,
    }))
}
fn valid_effort(value: &str) -> bool {
    crate::engine::reasoning::valid_effort(value)
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
}
fn save_room(state: &AppState, id_override: Option<&str>, body: &Value) -> ApiResult {
    let chosen = id_override.or_else(|| body["id"].as_str());
    let id = chosen
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if !valid_id(&id) {
        return Err(bad("유효하지 않은 회의실 ID입니다."));
    }
    if let Some(body_id) = body["id"].as_str() {
        if body_id != id {
            return Err(bad("경로와 본문의 회의실 ID가 다릅니다."));
        }
    }
    let _guard = lock(state, &id)?;
    let previous = state.store.get("room", &id).unwrap_or(Value::Null);
    if id_override.is_some() && previous.is_null() {
        return Err((StatusCode::NOT_FOUND, "회의실을 찾을 수 없습니다.".into()));
    }
    let config = validated_config(state, &id, &previous, body)?;
    let saved = if previous.is_null() {
        state.store.put("room", &config)
    } else {
        // `patch_if` deliberately rejects identity changes. Keep the
        // immutable room ID as the compare guard, not as a patched field.
        let mut patch=config;
        let _=patch.as_object_mut().and_then(|fields|fields.remove("id"));
        state.store.patch_if("room", &id, "id", &json!(id), &patch)
    }
    .ok_or_else(|| failed("회의실을 저장할 수 없습니다."))?;
    state.bus.record("room", &saved);
    rooms::ensure_room_session(state, &id).map_err(bad)?;
    room(state, &id)
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("room")))
}
pub async fn save(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    response(save_room(&state, None, &body))
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(save_room(&state, Some(&id), &body))
}
pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response(room(&state, &id))
}

pub async fn messages(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        Ok(json!(state.store.messages(session_id)))
    })())
}

/// Committed per-Room event stream plus each agent's durable readCursor.
pub async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String,String>>,
)->Response{
    response((||{
        let config=room(&state,&id)?;
        let session=rooms::ensure_room_session(&state,&id).map_err(bad)?;
        let session_id=session["id"].as_str()
            .ok_or_else(||failed("Room session ID missing"))?;
        let after=query.get("after").map(|s|s.parse::<u64>())
            .transpose().map_err(|_|bad("Invalid Room event cursor"))?
            .unwrap_or(0);
        let limit=query.get("limit").map(|s|s.parse::<usize>())
            .transpose().map_err(|_|bad("Invalid Room event limit"))?
            .unwrap_or(100).clamp(1,200);
        let mut rows=state.store.room_events(session_id,after,limit+1)
            .map_err(failed)?;
        let has_more=rows.len()>limit;
        rows.truncate(limit);
        let next_cursor=rows.last().and_then(|v|v["seq"].as_u64()).unwrap_or(after);
        let bindings=config["agentIds"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).map(|agent|{
                let key=crate::features::collaboration::room_cursor::binding_id(
                    session_id,agent);
                let saved=state.store.get("room_read_cursor",&key);
                json!({"roomId":id,"agentId":agent,"sessionId":session_id,
                    "readCursor":saved.as_ref()
                        .and_then(|c|c["throughSeq"].as_u64())
                        .or_else(||saved.as_ref()
                            .and_then(|c|c["throughMessageId"].as_str())
                            .and_then(|anchor|state.store.room_event_seq(session_id,anchor)))
                        .unwrap_or(0),
                    "state":"bound"})
            }).collect::<Vec<_>>();
        Ok(json!({"roomId":id,"sessionId":session_id,"events":rows,
            "nextCursor":next_cursor,"hasMore":has_more,"bindings":bindings}))
    })())
}

pub async fn system_prompt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    response((|| {
        room(&state, &id)?;
        crate::engine::chat::system_prompt_preview(&state, &id, query.get("agentId").map(String::as_str))
            .map_err(bad)
    })())
}


/// Owner edits for the room prompt. `common` replaces the shared contract,
/// `final` replaces the whole static prompt; null clears either override.
pub async fn save_system_prompt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response((|| {
        let _guard = lock(&state, &id)?;
        let mut room = room(&state, &id)?;
        for key in ["common", "final"] {
            let Some(value) = body.get(key) else { continue };
            match value {
                Value::Null => {
                    if let Some(fields) = room.as_object_mut() {
                        fields.remove(if key == "common" { "systemPromptCommon" } else { "systemPromptOverride" });
                    }
                }
                Value::String(text) if text.chars().count() <= 32_000 => {
                    room[if key == "common" { "systemPromptCommon" } else { "systemPromptOverride" }] =
                        json!(text);
                }
                _ => {
                    return Err(bad("프롬프트는 32000자 이내 문자열이거나 초기화용 null이어야 합니다."))
                }
            }
        }
        match state.store.put("room", &room) {
            Some(saved) => {
                state.bus.record("room", &saved);
                Ok(saved)
            }
            None => Err(failed("그룹 프롬프트를 저장하지 못했습니다.")),
        }
    })())
}

pub async fn knowledge(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        Ok(json!(crate::engine::room_knowledge::list(&state, &id)))
    })())
}

pub async fn learn(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        let through = state
            .store
            .messages(session_id)
            .last()
            .and_then(|message| message["id"].as_str().map(str::to_string))
            .ok_or_else(|| bad("학습할 대화가 없습니다."))?;
        crate::engine::room_knowledge::enqueue(&state, &id, session_id, &through)
            .ok_or_else(|| failed("방 학습 작업을 등록할 수 없습니다."))
    })())
}

fn submit(state: &AppState, id: &str, body: &Value) -> ApiResult {
    submit_internal(state, id, body, None, true)
}
pub(crate) fn submit_internal(
    state: &AppState,
    id: &str,
    body: &Value,
    origin_metadata: Option<Value>,
    tools_allowed: bool,
) -> ApiResult {
    if !body.is_object() {
        return Err(bad("메시지 객체가 필요합니다."));
    }
    let _guard = lock(state, id)?;
    let mut config = room(state, id)?;
    if !tools_allowed {
        config["toolsEnabled"] = json!(false);
    }
    if origin_metadata
        .as_ref()
        .is_some_and(|origin| origin["authorIsBot"] == true)
    {
        return Err(bad("봇 발언은 방 주인의 메시지로 제출할 수 없습니다."));
    }
    let text = body
        .get("input")
        .or_else(|| body.get("goal"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let attachment_ids = crate::engine::attachments::ids(&body["attachmentIds"]).map_err(bad)?;
    if text.is_empty() && attachment_ids.is_empty() || text.chars().count() > 32000 {
        return Err(bad("메시지는 1~32000자여야 합니다."));
    }
    if body.get("sessionId").is_some()
        || body.get("metadata").is_some()
        || body.get("role").is_some()
        || body.get("agentId").is_some()
    {
        return Err(bad("화자와 세션 정보는 서버가 정합니다."));
    }
    let session = rooms::ensure_room_session(state, id).map_err(bad)?;
    let session_id = session["id"]
        .as_str()
        .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
    let attachments = crate::engine::attachments::validate_ids(state, session_id, &attachment_ids)
        .map_err(bad)?;
    let settings=crate::engine::chat::TurnRequest::from_value(&json!({"input":if text.is_empty(){"첨부 파일"}else{text},"thinking":body["thinking"],"reasoningEffort":body["reasoningEffort"],"providerId":body["providerId"],"model":body["model"]})).map_err(bad)?;
    if let Some(provider) = settings.provider_id.as_deref() {
        if !state
            .store
            .get("provider", provider)
            .is_some_and(|provider| provider["enabled"].as_bool() != Some(false))
        {
            return Err(bad("활성 제공자를 찾을 수 없습니다."));
        }
    }
    if settings
        .model
        .as_ref()
        .is_some_and(|model| model.len() > 256)
    {
        return Err(bad("모델 이름이 너무 깁니다."));
    }
    if !body["reasoningLanguage"].is_null()
        && !matches!(body["reasoningLanguage"].as_str(), Some("ko" | "default"))
    {
        return Err(bad("추론 언어는 ko 또는 default여야 합니다."));
    }
    let nonce = if let Some(value) = body.get("clientMessageId") {
        let nonce = value
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 160)
            .ok_or_else(|| bad("clientMessageId는 1~160자의 문자열이어야 합니다."))?;
        format!("room-message-{}", memory::stable_id(&[id, nonce]))
    } else {
        uuid::Uuid::new_v4().to_string()
    };
    let job_id = format!("room-job-{}", memory::stable_id(&[id, &nonce]));
    if let Some(existing) = state.store.get("job", &job_id) {
        if existing["input"]["input"] != text
            || existing["input"]["requestedAgentIds"] != body["agentIds"]
            || existing["input"]["attachmentIds"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                != json!(attachment_ids)
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            || ["thinking", "reasoningEffort", "reasoningLanguage", "providerId", "model"]
                .iter()
                .any(|key| existing["input"][*key] != body[*key])
        {
            return Err(conflict(
                "같은 clientMessageId로 다른 메시지를 보낼 수 없습니다.",
            ));
        }
        let message = state
            .store
            .message(&nonce)
            .ok_or_else(|| failed("저장된 메시지를 찾을 수 없습니다."))?;
        return Ok(
            json!({"id":job_id,"jobId":job_id,"status":existing["status"],"room":config,"sessionId":session_id,"message":message,"job":existing}),
        );
    }
    let speakers = rooms::plan_speakers(state, &config, text, body.get("agentIds")).map_err(bad)?;
    let generation = state.cancellation_generation(session_id);
    let acceptance=crate::engine::chat::prepare_owner_action(state,text).map(|(request,checks)|json!({"request":request,"checks":checks}));
    let message = json!({"id":nonce,"session_id":session_id,"role":"user","agent_id":null,"content":text,
        "metadata":{"acceptance":acceptance,"authorKind":"owner","roomId":id,"ownerName":config["ownerName"],"clientMessageId":body["clientMessageId"],"attachments":attachments,"origin":origin_metadata}});
    let job = json!({"id":job_id,"kind":"room","status":"queued","attempts":0,"stepIndex":0,"stepTotal":0,"events":[],
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "sessionId":session_id,"input":{"roomId":id,"sessionId":session_id,"messageId":nonce,"sourceMessageId":nonce,"input":text,
            "agentIds":speakers,"requestedAgentIds":body["agentIds"],"explicitAgentIds":body.get("agentIds").is_some(),"routingMode":config["routingMode"],"generation":generation,"roomSnapshot":config,
            "attachmentIds":attachment_ids,"reasoningLanguage":body["reasoningLanguage"],"thinking":body["thinking"],"reasoningEffort":body["reasoningEffort"],"providerId":body["providerId"],"model":body["model"]}});
    let (saved_message, saved_job) = state
        .store
        .write_message_with_record(&message, "job", &job)
        .ok_or_else(|| failed("메시지와 응답 작업을 저장할 수 없습니다."))?;
    state.bus.message(&saved_message);
    state.bus.record("job", &saved_job);
    crate::engine::room_state::observe_owner_message(
        &state,
        id,
        session_id,
        &saved_message,
    );
    state.store.audit("room.owner_message",json!({"roomId":id,"sessionId":session_id,"messageId":saved_message["id"],"jobId":saved_job["id"]}));
    if let Some(saved)=state.store.patch_if("room",id,"id",&json!(id),&json!({"lastOwnerMessageId":saved_message["id"],"lastSubmittedAt":chrono::Utc::now().to_rfc3339(),"stoppedAt":null})){
        state.bus.record("room",&saved);
    }
    Ok(
        json!({"id":job_id,"jobId":job_id,"status":"queued","room":room(state,id)?,"sessionId":session_id,"message":saved_message,"job":saved_job}),
    )
}
pub async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(submit(&state, &id, &body))
}
/// The historical /run endpoint uses exactly the same persisted owner flow.
pub async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(submit(&state, &id, &body))
}

fn stop_room(state: &AppState, id: &str) -> ApiResult {
    let _guard = lock(state, id)?;
    stop_room_locked(state, id)
}
fn stop_room_locked(state: &AppState, id: &str) -> ApiResult {
    let config = room(state, id)?;
    let session_id = config["sessionId"].as_str();
    let pending = state
        .store
        .all("job")
        .into_iter()
        .filter(|job| {
            job["kind"] == "room"
                && job["input"]["roomId"] == id
                && matches!(job["status"].as_str(), Some("queued" | "running"))
        })
        .collect::<Vec<_>>();
    if let Some(session_id) = session_id {
        state.cancel_session(session_id);
    }
    // Old imported jobs may lack a session binding until their first turn.
    for job in &pending {
        if let Some(job_id) = job["id"].as_str() {
            if let Some(saved)=state.store.patch_if("job",job_id,"cancelRequested",job.get("cancelRequested").unwrap_or(&Value::Null),
                &json!({"cancelRequested":true,"status":"cancelled","finishedAt":chrono::Utc::now().to_rfc3339()})){state.bus.record("job",&saved);}
        }
    }
    if let Some(saved) = state.store.patch_if(
        "room",
        id,
        "id",
        &json!(id),
        &json!({"stoppedAt":chrono::Utc::now().to_rfc3339()}),
    ) {
        state.bus.record("room", &saved);
    }
    Ok(
        json!({"ok":true,"roomId":id,"sessionId":session_id,"status":"stopped","cancelledJobs":pending.len()}),
    )
}
pub async fn stop(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response(stop_room(&state, &id))
}
pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let _guard = lock(&state, &id)?;
        stop_room_locked(&state, &id)?;
        if !state.store.remove("room", &id) {
            return Err(failed("회의실을 삭제할 수 없습니다."));
        }
        state
            .bus
            .emit(json!({"type":"record","kind":"room","removed":id}));
        // Keep the transcript as a durable archive; never erase owner messages.
        Ok(json!({"ok":true}))
    })())
}


/// Owner-launched Team task inside a group room. The engine turns the goal
/// into one worker task per eligible room agent and runs them through the
/// existing native team job journal.
pub async fn team_launch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response((|| {
        let goal = body["goal"]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.len() <= 4000)
            .ok_or_else(|| bad("맡길 작업은 1~4000자여야 합니다."))?;
        let nonce = body["clientRequestId"]
            .as_str()
            .filter(|text| uuid::Uuid::parse_str(text).is_ok())
            .ok_or_else(|| bad("요청 ID가 올바르지 않습니다."))?;
        let parallel = body["maxParallel"].as_u64().unwrap_or(3);
        if !(1..=4).contains(&parallel) {
            return Err(bad("동시 작업은 1~4개까지 가능합니다."));
        }
        let _guard = lock(&state, &id)?;
        let config = room(&state, &id)?;
        if config["toolsEnabled"] != true {
            return Err((StatusCode::FORBIDDEN, "그룹의 도구 사용을 먼저 켜세요.".into()));
        }
        if config["status"].as_str() == Some("running") {
            return Err(conflict("그룹 대화가 진행 중입니다. 끝난 뒤 다시 시도하세요."));
        }
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"].as_str().ok_or_else(|| failed("그룹 세션이 없습니다."))?;
        let launch_id = format!("room-team-launch:{}", memory::stable_id(&[&id, nonce]));
        if let Some(previous) = state.store.get("room_team_launch", &launch_id) {
            if previous["roomId"] != id || previous["goal"] != goal {
                return Err(conflict("같은 요청 ID로 다른 작업을 시작할 수 없습니다."));
            }
            return Ok(previous);
        }
        let agents = config["agentIds"]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .filter_map(|value| value.as_str())
                    .filter_map(|agent_id| state.store.get("agent", agent_id))
                    .filter(|agent| agent["enabled"] != false)
                    .filter(|agent| agent["providerId"].as_str().is_some_and(|p| !p.is_empty()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if agents.is_empty() {
            return Err(bad("이 그룹에는 작업을 맡길 수 있는 AI가 없습니다. AI 연결을 확인하세요."));
        }
        let mut tasks = Vec::new();
        let mut plan_steps = Vec::new();
        for (index, agent) in agents.iter().take(4).enumerate() {
            let task_id = format!("team-{}", index + 1);
            plan_steps.push((task_id.clone(), agent["id"].clone()));
            tasks.push(json!({
                "id": task_id,
                "prompt": format!("{goal}\n\n맡은 역할에 따라 조사하고, 실제로 확인한 내용만 한국어로 간결하게 보고하세요."),
                "mode": "read",
                "runtime": "native",
                "agentId": agent["id"],
            }));
        }
        let team = json!({
            "goal": goal,
            "tasks": tasks,
            "maxParallel": parallel.min(3),
        });
        let Some(job) = crate::engine::jobs::enqueue(&state, "team", team) else {
            return Err(failed("Team 작업을 시작하지 못했습니다."));
        };
        let job_id = job["id"].as_str().unwrap_or_default().to_string();
        let mut patched = job.clone();
        patched["sessionId"] = json!(session_id);
        patched["roomId"] = json!(id);
        patched["teamLaunchId"] = json!(launch_id);
        // The UI reads input.plan to show step progress; worker ids are
        // deterministic (team-child:<job>:<task>), so they can be written now.
        let plan: Vec<Value> = plan_steps
            .iter()
            .map(|(task_id, agent_id)| {
                json!({
                    "id": task_id,
                    "agentId": agent_id,
                    "jobId": crate::features::collaboration::task_team::child_id(&job_id, task_id),
                })
            })
            .collect();
        patched["input"]["plan"] = json!(plan);
        patched["input"]["sessionId"] = json!(session_id);
        if let Some(saved) = state.store.put("job", &patched) {
            state.bus.record("job", &saved);
        }
        let launch = json!({
            "id": launch_id,
            "roomId": id,
            "sessionId": session_id,
            "status": "planning",
            "goal": goal,
            "maxParallel": parallel,
            "createdAt": chrono::Utc::now().to_rfc3339(),
            "clientRequestId": nonce,
            "jobId": job_id,
            "error": Value::Null,
        });
        let saved = state
            .store
            .put_if_absent("room_team_launch", &launch)
            .unwrap_or(launch);
        state.bus.record("room_team_launch", &saved);
        Ok(saved)
    })())
}

pub async fn team_launch_status(
    State(state): State<AppState>,
    Path((id, launch_id)): Path<(String, String)>,
) -> Response {
    response((|| {
        let Some(mut launch) = state.store.get("room_team_launch", &launch_id) else {
            return Err((StatusCode::NOT_FOUND, "Team 요청을 찾을 수 없습니다.".into()));
        };
        if launch["roomId"].as_str() != Some(id.as_str()) {
            return Err((StatusCode::NOT_FOUND, "Team 요청을 찾을 수 없습니다.".into()));
        }
        if let Some(job_id) = launch["jobId"].as_str() {
            if let Some(job) = state.store.get("job", job_id) {
                let status = job["status"].as_str().unwrap_or("queued");
                launch["status"] = json!(match status {
                    "queued" | "running" | "planning" | "preparing" => "planning",
                    "failed" | "cancelled" | "interrupted" => "failed",
                    _ => "queued",
                });
                if matches!(status, "failed" | "cancelled" | "interrupted") {
                    launch["error"] = job["error"].clone();
                }
            }
        }
        Ok(launch)
    })())
}

/// Archives the current room transcript and binds the room to a fresh empty
/// session. The room, members, settings and knowledge stay intact; the old
/// session and all of its messages remain available as a durable archive.
pub async fn reset(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let _guard = lock(&state, &id)?;
        stop_room_locked(&state, &id)?;
        let current = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let previous_session_id = current["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?
            .to_string();
        crate::engine::chat::close_session_approvals(&state, &previous_session_id);

        let current_room = room(&state, &id)?;
        let title = current_room
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("대화방");
        let session_id = format!("room:{id}:{}", uuid::Uuid::new_v4().simple());
        let record = json!({
            "id": session_id,
            "title": title,
            "roomId": id,
            "kind": "room",
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });
        if state.store.put_if_absent("session", &record).is_none() {
            return Err(failed("새 대화방 세션을 만들 수 없습니다."));
        }
        let binding = current_room.get("sessionId").unwrap_or(&Value::Null);
        let Some(saved) = state.store.patch_if(
            "room",
            &id,
            "sessionId",
            binding,
            &json!({
                "sessionId": session_id,
                "status": "idle",
                "pauseReason": Value::Null,
                "activeAgentId": Value::Null,
                "resetAt": chrono::Utc::now().to_rfc3339(),
            }),
        ) else {
            return Err(conflict("대화방 세션이 동시에 변경되었습니다. 다시 시도하세요."));
        };
        state.bus.record("room", &saved);
        crate::engine::room_state::clear(&state, &id);
        Ok(json!({
            "ok": true,
            "roomId": id,
            "previousSessionId": previous_session_id,
            "sessionId": session_id,
        }))
    })())
}

pub async fn state(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let config = room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        let snapshot = state
            .store
            .room_status_records(&id, session_id)
            .map_err(failed)?;
        let jobs = snapshot.room_jobs;
        let queue_depth = snapshot.room_queue_depth;
        let running = snapshot.room_running_job.as_ref();
        let active_agent = running
            .and_then(|job| job["roomState"]["activeAgentId"].as_str())
            .or_else(|| {
                if running.is_some() {
                    config["activeAgentId"].as_str()
                } else {
                    None
                }
            });
        let status = if running.is_some() {
            "running"
        } else if queue_depth > 0 {
            "queued"
        } else if !config["stoppedAt"].is_null() {
            "stopped"
        } else if matches!(config["status"].as_str(), Some("paused" | "needs_review")) {
            config["status"].as_str().unwrap_or("idle")
        } else {
            jobs.first()
                .and_then(|job| job["status"].as_str())
                .filter(|status| matches!(*status, "failed" | "needs_review"))
                .unwrap_or("idle")
        };
        let executions = snapshot.executions;
        let learning = snapshot.learning_jobs;
        let operations = snapshot.operations.into_iter()
            .map(|operation|json!({"id":operation["id"],"runId":operation["runId"],"tool":operation["tool"],"status":operation["status"],
                "startedAt":operation["startedAt"],"finishedAt":operation["finishedAt"],
                "arguments":bounded_json(&operation["arguments"],2000),"result":bounded_json(&operation["result"],6000)})).collect::<Vec<_>>();
        let approvals = snapshot.approvals;
        Ok(
            json!({"room":room(&state,&id)?,"sessionId":session_id,"status":status,"activeAgentId":active_agent,"queueDepth":queue_depth,
            "jobs":jobs,"executions":executions,"learningJobs":learning,"operations":operations,"approvals":approvals,"usage":state.store.session_usage(session_id)}),
        )
    })())
}

fn bounded_json(value: &Value, limit: usize) -> Value {
    let text = value.to_string();
    if text.chars().count() <= limit {
        value.clone()
    } else {
        json!({"preview":text.chars().take(limit).collect::<String>(),"truncated":true})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_room_can_be_created_without_picking_members() {
        let (state, dir) = test_state();
        let body = json!({"name":"빠른 방"});
        let saved = validated_config(&state, "room-quick", &json!({}), &body).unwrap();
        let members = saved["agentIds"].as_array().unwrap();
        assert!(!members.is_empty() && members.len() <= 3, "defaults must stay small");
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn test_state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-room-api-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        for index in 0..9 {
            state.store.put(
                "agent",
                &json!({"id":format!("a{index}"),"name":format!("참가자{index}")}),
            );
        }
        (state, dir)
    }
    fn config() -> Value {
        json!({"id":"room-one","name":"우리 방","agentIds":(0..9).map(|i|format!("a{i}")).collect::<Vec<_>>(),"routingMode":"discussion"})
    }
    #[tokio::test]
    async fn room_status_is_scoped_to_current_room_and_preserves_legacy_operation_receipts() {
        let (app, dir) = test_state();
        save_room(&app, None, &config()).unwrap();
        let room_session = rooms::ensure_room_session(&app, "room-one").unwrap();
        let sid = room_session["id"].as_str().unwrap().to_string();
        for index in 0..80 {
            app.store.put("job", &json!({
                "id":format!("other-job-{index}"),"kind":"room","status":"queued",
                "input":{"roomId":"unrelated"},"queuedAt":"2026-11-21T10:00:00Z"
            })).unwrap();
            app.store.put("execution", &json!({
                "id":format!("other-exec-{index}"),"sessionId":"unrelated",
                "status":"completed"
            })).unwrap();
            app.store.put("execution_operation", &json!({
                "id":format!("other-op-{index}"),"sessionId":"unrelated",
                "runId":format!("other-exec-{index}"),"status":"succeeded"
            })).unwrap();
            app.store.put("approval", &json!({
                "id":format!("other-approval-{index}"),"sessionId":"unrelated",
                "status":"approved"
            })).unwrap();
        }
        app.store.put("job", &json!({
            "id":"room-running","kind":"room","status":"running",
            "queuedAt":"2026-11-21T11:00:00Z",
            "input":{"roomId":"room-one"},"roomState":{"activeAgentId":"a0"}
        })).unwrap();
        app.store.put("job", &json!({
            "id":"room-learning","kind":"learn","status":"completed",
            "input":{"sessionId":sid}
        })).unwrap();
        app.store.put("execution", &json!({
            "id":"room-exec","sessionId":sid,"status":"completed"
        })).unwrap();
        app.store.put("execution_operation", &json!({
            "id":"legacy-room-op","runId":"room-exec","status":"succeeded",
            "tool":"workspace_read","arguments":{"path":"note.txt"}
        })).unwrap();
        app.store.put("approval", &json!({
            "id":"room-approval","sessionId":sid,"status":"approved"
        })).unwrap();
        let response = state(State(app.clone()), Path("room-one".into())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 64*1024).await.unwrap();
        let output: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(output["status"], "running");
        assert_eq!(output["queueDepth"], 0);
        assert_eq!(output["activeAgentId"], "a0");
        assert_eq!(output["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(output["jobs"][0]["id"], "room-running");
        assert_eq!(output["learningJobs"][0]["id"], "room-learning");
        assert_eq!(output["executions"][0]["id"], "room-exec");
        assert_eq!(output["operations"][0]["id"], "legacy-room-op");
        assert_eq!(output["approvals"][0]["id"], "room-approval");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn durable_room_events_and_agent_cursor_survive_an_api_page(){
        let (state,dir)=test_state();
        let _saved=save_room(&state,None,&config()).unwrap();
        let session=rooms::ensure_room_session(&state,"room-one").unwrap();
        let session_id=session["id"].as_str().unwrap().to_string();
        let owner=state.store.write_message(&json!({
            "id":"owner-room-event","session_id":session_id,
            "role":"user","content":"Test", "metadata":{"authorKind":"owner"}
        })).unwrap();
        let source_id=owner["id"].as_str().unwrap();
        let agent=state.store.write_room_answer_with_cursor(
            &json!({"id":"agent-room-event","session_id":session_id,
                "role":"assistant","agent_id":"a0","content":"Observed",
                "metadata":{"authorKind":"agent","replyToMessageId":source_id}}),
            &crate::features::collaboration::room_cursor::checkpoint(
                &session_id,"a0",source_id,source_id)
        ).unwrap();
        assert_eq!(agent["id"],"agent-room-event");
        let response=events(State(state.clone()),Path("room-one".into()),
            Query(HashMap::from([("after".to_string(),"0".to_string()),
                ("limit".to_string(),"1".to_string())]))).await;
        assert_eq!(response.status(),StatusCode::OK);
        let bytes=axum::body::to_bytes(response.into_body(),64*1024).await.unwrap();
        let page:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(page["events"][0]["eventId"],"owner-room-event");
        assert_eq!(page["events"][0]["seq"],1);
        assert_eq!(page["hasMore"],true);
        assert_eq!(page["nextCursor"],1);
        let cursor=page["bindings"].as_array().unwrap().iter()
            .find(|v|v["agentId"]=="a0").unwrap();
        assert_eq!(cursor["readCursor"],1);
        let page_two=events(State(state.clone()),Path("room-one".into()),
            Query(HashMap::from([("after".to_string(),"1".to_string())]))).await;
        let bytes=axum::body::to_bytes(page_two.into_body(),64*1024).await.unwrap();
        let page:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(page["events"][0]["eventId"],"agent-room-event");
        assert_eq!(page["events"][0]["seq"],2);
        crate::test_support::cleanup_dir(dir);
    }
    #[test]
    fn nine_agent_roster_has_stable_private_session_and_safe_defaults() {
        let (state, dir) = test_state();
        let saved = save_room(&state, None, &config()).unwrap();
        assert_eq!(saved["agentIds"].as_array().unwrap().len(), 9);
        assert_eq!(saved["toolsEnabled"], false);
        assert_eq!(saved["autoLearning"], true);
        let session_id = saved["sessionId"].as_str().unwrap();
        let session = state.store.get("session", session_id).unwrap();
        assert_eq!(session["roomId"], "room-one");
        let update = save_room(
            &state,
            Some("room-one"),
            &json!({"name":"오래 가는 방","rounds":2}),
        )
        .unwrap();
        assert_eq!(update["sessionId"], session_id);
        assert!(save_room(
            &state,
            None,
            &json!({"id":"rogue","name":"탈취","agentIds":["a0"],"sessionId":session_id})
        )
        .is_err());
        assert!(save_room(
            &state,
            Some("room-one"),
            &json!({"sessionId":"private-session"})
        )
        .is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"agentIds":["missing"]})).is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"activeAgentId":"a8"})).is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"maxTurns":100001})).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn local_runtime_models_can_be_selected_for_a_room() {
        let (state, dir) = test_state();
        state.store.put(
            "provider",
            &json!({
                "id":"local-mlx","name":"local MLX","kind":"mlx","local":true,
                "baseUrl":"http://127.0.0.1:1234/v1","model":"mlx-7b","enabled":true
            }),
        );
        let saved = save_room(
            &state,
            None,
            &json!({
                "id":"local-room","name":"로컬 방","agentIds":["a0","a1"],
                "models":[{"providerId":"local-mlx","model":"mlx-7b"}]
            }),
        )
        .unwrap();
        assert_eq!(saved["models"][0]["providerId"], "local-mlx", "{saved}");
        assert_eq!(saved["models"][0]["model"], "mlx-7b", "{saved}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn room_attachments_and_response_overrides_are_bound_and_immutable() {
        use base64::Engine;
        let (state, dir) = test_state();
        save_room(&state, None, &config()).unwrap();
        state
            .store
            .put("provider", &json!({"id":"chosen","enabled":true}));
        let file=crate::engine::attachments::upload(&state,&json!({"roomId":"room-one","name":"notes.txt","data":base64::engine::general_purpose::STANDARD.encode("실제 사용자 원문") })).unwrap();
        let body = json!({"input":"첨부 확인","attachmentIds":[file["id"]],"thinking":false,"reasoningEffort":"minimal","providerId":"chosen","model":"chosen-model","clientMessageId":"file-turn"});
        let submitted = submit(&state, "room-one", &body).unwrap();
        assert_eq!(
            submitted["message"]["metadata"]["attachments"][0]["id"],
            file["id"]
        );
        assert_eq!(submitted["job"]["input"]["reasoningEffort"], "minimal");
        assert_eq!(submitted["job"]["input"]["providerId"], "chosen");
        let mut changed = body.clone();
        changed["thinking"] = json!(true);
        assert!(submit(&state, "room-one", &changed).is_err());
        let internal = submit_internal(
            &state,
            "room-one",
            &json!({"input":"외부 명령","clientMessageId":"external"}),
            Some(json!({"transport":"telegram","messageId":"inbound-one"})),
            false,
        )
        .unwrap();
        assert_eq!(
            internal["job"]["input"]["roomSnapshot"]["toolsEnabled"],
            false
        );
        assert_eq!(
            internal["message"]["metadata"]["origin"]["transport"],
            "telegram"
        );
        assert!(submit_internal(
            &state,
            "room-one",
            &json!({"input":"봇을 주인으로 속임"}),
            Some(json!({"authorIsBot":true})),
            false
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn owner_message_and_job_are_atomic_idempotent_and_stop_preserves_transcript() {
        let (state, dir) = test_state();
        save_room(&state, None, &config()).unwrap();
        let body =
            json!({"input":"아홉 명 다 같이 계획을 논의해","clientMessageId":"owner-message-one"});
        let sent = submit(&state, "room-one", &body).unwrap();
        let session_id = sent["sessionId"].as_str().unwrap();
        let messages = state.store.messages(session_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert!(messages[0]["agent_id"].is_null());
        assert_eq!(messages[0]["metadata"]["authorKind"], "owner");
        assert_eq!(
            sent["job"]["input"]["agentIds"].as_array().unwrap().len(),
            9
        );
        assert_eq!(sent["job"]["input"]["sourceMessageId"], messages[0]["id"]);
        assert_eq!(sent["job"]["input"]["roomSnapshot"]["toolsEnabled"], false);
        let again = submit(&state, "room-one", &body).unwrap();
        assert_eq!(sent["jobId"], again["jobId"]);
        assert_eq!(state.store.messages(session_id).len(), 1);
        assert_eq!(state.store.count("job"), 1);
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"다른 문장","clientMessageId":"owner-message-one"})
        )
        .is_err());
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"화자 탈취","role":"assistant"})
        )
        .is_err());
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"다른 방 참가자","agentIds":["outsider"]})
        )
        .is_err());
        let original_generation = sent["job"]["input"]["generation"].as_u64().unwrap();
        let stopped = stop_room(&state, "room-one").unwrap();
        assert_eq!(stopped["cancelledJobs"], 1);
        let cancelled = state
            .store
            .get("job", sent["jobId"].as_str().unwrap())
            .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["cancelRequested"], true);
        assert!(state.is_cancelled(session_id));
        assert_eq!(state.store.messages(session_id).len(), 1);
        let next = submit(
            &state,
            "room-one",
            &json!({"input":"이제 새 주제로 다시 시작해","clientMessageId":"second"}),
        )
        .unwrap();
        assert!(next["job"]["input"]["generation"].as_u64().unwrap() > original_generation);
        // Submitting fresh work never clears the stop of an older running turn.
        assert!(state.is_cancelled(session_id));
        assert_eq!(state.store.messages(session_id).len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
