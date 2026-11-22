use crate::state::AppState;
use axum::{extract::{Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use std::collections::HashMap;
use serde_json::{json,Value};
use super::response::error;


/// Read-only preview from Sub2's existing native chat prompt compiler.
pub async fn system_prompt_preview(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String,String>>,
)->Response{
    let input=query.get("input").map(String::as_str).unwrap_or("일반적인 질문");
    if input.trim().is_empty() || input.chars().count()>512 {
        return error(StatusCode::BAD_REQUEST,"미리보기 질문은 1~512자여야 합니다.");
    }
    let flag=|key:&str|->Result<bool,&'static str>{
        match query.get(key).map(String::as_str){
            None|Some("true")=>Ok(true),Some("false")=>Ok(false),
            _=>Err("미리보기 옵션은 true/false여야 합니다."),
        }
    };
    let tools=match flag("toolsEnabled"){Ok(v)=>v,Err(e)=>return error(StatusCode::BAD_REQUEST,e)};
    let ko=match flag("koreanReasoning"){Ok(v)=>v,Err(e)=>return error(StatusCode::BAD_REQUEST,e)};
    let agent=query.get("agentId").map(String::as_str).filter(|s|!s.is_empty());
    let session=query.get("sessionId").map(String::as_str).filter(|s|!s.is_empty());
    match crate::engine::chat::private_system_prompt_preview(&state,session,agent,input,tools,ko){
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,&message),
    }
}

/// Direct-chat-only override; a null resets to Sub2's native generated prompt.
/// No user prompt is ever allowed to alter Rust tool permissions.
pub async fn save_system_prompt_override(
    State(state): State<AppState>,Json(body):Json<Value>,
)->Response{
    let prompt=match body.get("prompt"){
        Some(Value::String(text)) if text.chars().count()<=32_000=>Some(text.as_str()),
        Some(Value::Null)=>None,
        _=>return error(StatusCode::BAD_REQUEST,
            "prompt는 32000자 이내 문자열이거나 초기화용 null이어야 합니다."),
    };
    if body.get("agentId").is_some_and(|v|!v.is_string()){
        return error(StatusCode::BAD_REQUEST,"agentId는 문자열이어야 합니다.");
    }
    let agent=body["agentId"].as_str().filter(|s|!s.is_empty());
    let (kind,mut record)=if let Some(id)=agent{
        let Some(value)=state.store.get("agent",id)else{
            return error(StatusCode::NOT_FOUND,"에이전트를 찾을 수 없습니다.");
        };
        ("agent",value)
    }else{
        ("assistant_settings",state.store.get("assistant_settings","global")
            .unwrap_or_else(||json!({"id":"global","instructions":"","enabled":true})))
    };
    if let Some(text)=prompt{record["systemPromptOverride"]=json!(text)}
    else if let Some(fields)=record.as_object_mut(){fields.remove("systemPromptOverride");}
    match state.store.put(kind,&record){
        Some(saved)=>{
            state.bus.record(kind,&saved);
            Json(json!({"agentId":agent,"overrideActive":saved["systemPromptOverride"].is_string(),
                "overrideText":saved["systemPromptOverride"]})).into_response()
        }
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,"시스템 프롬프트를 저장하지 못했습니다."),
    }
}

pub async fn assistant_settings(State(state): State<AppState>) -> Json<Value> {
    Json(
        state
            .store
            .get("assistant_settings", "global")
            .unwrap_or_else(|| json!({"id":"global","instructions":"","enabled":true})),
    )
}

pub async fn save_assistant_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(instructions) = body["instructions"]
        .as_str()
        .filter(|text| text.chars().count() <= 16000)
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "공통 지침은 16000자 이내 문자열이어야 합니다.",
        );
    };
    if body
        .get("enabled")
        .is_some_and(|enabled| !enabled.is_boolean())
    {
        return error(StatusCode::BAD_REQUEST, "enabled는 true/false여야 합니다.");
    }
    match state.store.put("assistant_settings",&json!({"id":"global","instructions":instructions,"enabled":body["enabled"].as_bool().unwrap_or(true),"source":"owner","updatedAt":chrono::Utc::now().to_rfc3339()})){
        Some(saved)=>{state.bus.record("assistant_settings",&saved);Json(saved).into_response()},None=>error(StatusCode::INTERNAL_SERVER_ERROR,"공통 지침을 저장할 수 없습니다."),
    }
}

fn default_owner_profile() -> Value {
    json!({"id":"global","name":"나","avatar":"","updatedAt":null})
}

/// The owner is a person, not an agent. Keep their visual identity in one
/// durable record shared by Chat, Rooms and the workspace navigation.
pub async fn owner_profile(State(state): State<AppState>) -> Json<Value> {
    Json(state.store.get("owner_profile", "global")
        .unwrap_or_else(default_owner_profile))
}

pub(crate) fn valid_profile_avatar(value: &str) -> bool {
    use base64::Engine as _;
    if value.is_empty() { return true; }
    let Some((kind, encoded)) = value.split_once(";base64,") else { return false; };
    if !matches!(kind, "data:image/png" | "data:image/jpeg" | "data:image/webp")
        || encoded.len() > 1_500_000 || encoded.is_empty()
    { return false; }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    if bytes.is_empty() || bytes.len() > 1_048_576 { return false; }
    match kind {
        "data:image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "data:image/jpeg" => bytes.starts_with(&[0xff,0xd8,0xff]),
        "data:image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF"
            && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

pub async fn save_owner_profile(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(name) = body.get("name").and_then(Value::as_str)
        .map(str::trim).filter(|name| !name.is_empty() && name.chars().count() <= 60)
    else { return error(StatusCode::BAD_REQUEST, "사용자 이름은 1~60자여야 합니다."); };
    let Some(avatar) = body.get("avatar").and_then(Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "사용자 아이콘은 문자열이어야 합니다.");
    };
    if !valid_profile_avatar(avatar) {
        return error(StatusCode::BAD_REQUEST,
            "사용자 아이콘은 1MB 이하의 PNG/JPEG/WebP 이미지여야 합니다.");
    }
    match state.store.put("owner_profile", &json!({
        "id":"global", "name":name, "avatar":avatar,
        "updatedAt":chrono::Utc::now().to_rfc3339()
    })) {
        Some(saved) => {
            state.bus.record("owner_profile", &saved);
            Json(saved).into_response()
        },
        None => error(StatusCode::INTERNAL_SERVER_ERROR,
            "사용자 프로필을 저장하지 못했습니다."),
    }
}
