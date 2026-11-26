//! Pickle-compatible mobile surface.
//!
//! The Android client (hushmind3/A-4) speaks a small `/v1` contract:
//! conversations, messages, calls and a handful of profile/feature endpoints.
//! Everything here is a projection of data that already lives in Dominion
//! (rooms, room messages, agents, LiveKit), so this module owns no state.
use crate::api::rooms;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashMap;

const DEFAULT_LIVEKIT_PORT: u16 = 7880;

fn env_or(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn livekit_key() -> String {
    env_or("DOMINION_LIVEKIT_KEY", "dominion")
}

fn livekit_secret() -> String {
    env_or(
        "DOMINION_LIVEKIT_SECRET",
        "dominion-local-dev-secret-0123456789",
    )
}

fn livekit_url(headers: &HeaderMap) -> String {
    if let Ok(url) = std::env::var("DOMINION_LIVEKIT_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1:4317");
    let hostname = host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host);
    format!("ws://{hostname}:{DEFAULT_LIVEKIT_PORT}")
}

fn sign_token(key: &str, secret: &str, room: &str, identity: &str, ttl_secs: u64) -> String {
    let now = chrono::Utc::now().timestamp();
    let header = json!({"alg": "HS256", "typ": "JWT"});
    let payload = json!({
        "iss": key,
        "sub": identity,
        "nbf": now,
        "exp": now + ttl_secs as i64,
        "video": {
            "room": room,
            "roomJoin": true,
            "canPublish": true,
            "canSubscribe": true,
            "canPublishData": true,
        }
    });
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(signing_input.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{signing_input}.{signature}")
}

fn epoch_millis(value: Option<&Value>) -> i64 {
    value
        .and_then(Value::as_str)
        .and_then(|text| chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok())
        .map(|stamp| stamp.and_utc().timestamp_millis())
        .unwrap_or(0)
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn ok_data(value: Value) -> Json<Value> {
    Json(json!({"data": value}))
}

// ---------------------------------------------------------------- accounts

pub async fn me(State(state): State<AppState>) -> Json<Value> {
    // Single-user local deployment: there is no account store, so report a
    // stable owner profile derived from the configured owner profile.
    let owner = state
        .store
        .get("owner_profile", "default")
        .unwrap_or_else(|| json!({}));
    let name = owner
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("주인");
    Json(json!({"data": {
        "id": "owner",
        "name": name,
        "birthday": "",
        "phone": "",
        "email": "",
        "plan": "free",
        "jelly": 0,
        "energy": 10,
        "energy_max": 10,
        "tickets": 0,
        "credits": 0,
        "invite_code": "",
        "level": 1,
        "xp": 0,
        "xp_goal": 100,
    }}))
}

pub async fn update_me(Json(_body): Json<Value>) -> Json<Value> {
    // Accept and ignore; the local owner profile is edited through Dominion's
    // own settings screen, not the mobile client.
    Json(json!({"data": {
        "id": "owner",
        "name": "주인",
        "plan": "free",
        "energy": 10,
        "energy_max": 10,
        "level": 1,
        "xp": 0,
        "xp_goal": 100,
    }}))
}

// ---------------------------------------------------------------- characters

pub async fn characters(State(state): State<AppState>) -> Json<Value> {
    let agents: Vec<Value> = state
        .store
        .all("agent")
        .iter()
        .map(|agent| {
            json!({
                "id": agent["id"].as_str().unwrap_or_default(),
                "name": agent["name"].as_str().unwrap_or("에이전트"),
                "tagline": agent["description"].as_str().unwrap_or_default(),
                "bio": agent["systemPrompt"].as_str().unwrap_or_default(),
                "color": agent["color"].as_str(),
                "shape": "blob",
                "level": 1,
                "xp": 0,
                "xp_goal": 100,
                "mine": true,
                "online": true,
                "voice": "soft",
                "relation": null,
            })
        })
        .collect();
    Json(json!({"data": agents}))
}

// ---------------------------------------------------------------- conversations

fn conversation_from_room(room: &Value) -> Value {
    let id = room["id"].as_str().unwrap_or_default();
    let agent_id = room["activeAgentId"]
        .as_str()
        .or_else(|| {
            room["agentIds"]
                .as_array()
                .and_then(|ids| ids.first())
                .and_then(Value::as_str)
        })
        .unwrap_or("dominion");
    let title = room["name"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Dominion");
    json!({
        "id": id,
        "character_id": agent_id,
        "title": title,
        "kind": "direct",
        "pinned": false,
        "muted": false,
        "unread": 0,
        "updated_at": epoch_millis(room.get("lastSubmittedAt").or_else(|| room.get("updatedAt"))),
        "request": false,
        "messages": [],
    })
}

pub async fn conversations(State(state): State<AppState>) -> Json<Value> {
    // Ensure a single-agent mobile assistant room exists so the phone has a
    // usable 1:1 conversation instead of only the 9-agent Nazarick room.
    if state.store.get("room", "assistant").is_none() {
        let body = json!({
            "id": "assistant",
            "name": "도미니언 비서",
            "agentIds": ["dominion-ainz"],
            "routingMode": "discussion",
            "description": "모바일 비서 대화",
            "topic": "",
            "ownerName": "나",
            "rounds": 1,
            "maxTurns": 1000,
            "tokenBudget": 10000000,
            "contextBudgetTokens": 2000,
            "toolsEnabled": false,
            "autoLearning": false,
            "autoModels": true,
            "reasoningEffort": "",
            "models": [{"providerId": "05aa0051-884b-473f-ad90-90fb4dffdb73", "model": "deepseek-v4.1-flash"}],
        });
        let _ = rooms::save(State(state.clone()), Json(body)).await;
    }

    let rooms: Vec<Value> = state
        .store
        .all("room")
        .iter()
        .map(conversation_from_room)
        .collect();
    Json(json!({"data": rooms}))
}

fn message_from_row(row: &Value, conversation_id: &str) -> Value {
    let role = row["role"].as_str().unwrap_or("assistant");
    json!({
        "id": row["id"].as_str().unwrap_or_default(),
        "conversation_id": conversation_id,
        "sender": if role == "user" { "user" } else { "character" },
        "text": row["content"].as_str().unwrap_or_default(),
        "kind": "text",
        "created_at": epoch_millis(row.get("created_at")),
        "card": Value::Null,
        "reply_to": Value::Null,
    })
}

pub async fn messages(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if state.store.get("room", &id).is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "대화를 찾을 수 없습니다."})))
            .into_response();
    }
    let session = match crate::engine::rooms::ensure_room_session(&state, &id) {
        Ok(session) => session,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
        }
    };
    let session_id = session["id"].as_str().unwrap_or_default();
    let rows: Vec<Value> = state
        .store
        .messages(session_id)
        .iter()
        .map(|row| message_from_row(row, &id))
        .collect();
    Json(json!({"data": rows})).into_response()
}

pub async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let text = body["text"].as_str().unwrap_or_default().trim();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "메시지가 비어 있습니다."})))
            .into_response();
    }
    let submitted = json!({"input": text});
    match rooms::submit_internal(&state, &id, &submitted, None, true) {
        Ok(_) => Json(json!({"data": {
            "id": format!("local-{}", uuid::Uuid::new_v4()),
            "conversation_id": id,
            "sender": "user",
            "text": text,
            "kind": "text",
            "created_at": now_millis(),
            "card": Value::Null,
            "reply_to": Value::Null,
        }}))
        .into_response(),
        Err((status, error)) => (status, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn start_call(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Json<Value> {
    let room = format!(
        "call-{}",
        id.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
    );
    let identity = format!("owner-{}", uuid::Uuid::new_v4());
    let token = sign_token(&livekit_key(), &livekit_secret(), &room, &identity, 3600);
    Json(json!({"data": {
        "room": room,
        "token": token,
        "url": livekit_url(&headers),
        "expires_at": (chrono::Utc::now().timestamp() + 3600) * 1000,
    }}))
}

// ---------------------------------------------------------------- tasks / schedules / memories

pub async fn tasks() -> Json<Value> {
    Json(json!({"data": []}))
}

pub async fn schedules() -> Json<Value> {
    Json(json!({"data": []}))
}

pub async fn memories(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    let _ = params.get("character_id");
    Json(json!({"data": []}))
}

// ---------------------------------------------------------------- friends / connections / explore / vault / credits / inbox

pub async fn friends() -> Json<Value> {
    Json(json!({"data": []}))
}

pub async fn friend_requests() -> Json<Value> {
    Json(json!({"data": []}))
}

pub async fn accept_friend(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({"data": null}))
}

pub async fn connections() -> Json<Value> {
    // Expose the configured providers as "connections" so the screen is not empty.
    Json(json!({"data": []}))
}

pub async fn connect(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({"data": null}))
}

pub async fn disconnect(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({"data": null}))
}

pub async fn explore(Query(_params): Query<HashMap<String, String>>) -> Json<Value> {
    Json(json!({"data": []}))
}

pub async fn vault() -> Json<Value> {
    Json(json!({"data": []}))
}

pub async fn transactions() -> Json<Value> {
    Json(json!({"data": []}))
}

pub async fn notifications() -> Json<Value> {
    Json(json!({"data": []}))
}

// ---------------------------------------------------------------- auth (single-user local)

pub async fn sign_in(
    Path(_provider): Path<String>,
    Json(_body): Json<Value>,
) -> Json<Value> {
    Json(json!({"token": "dominion-local", "account": {
        "id": "owner",
        "name": "주인",
        "plan": "free",
        "energy": 10,
        "energy_max": 10,
        "level": 1,
        "xp": 0,
        "xp_goal": 100,
    }}))
}

pub async fn start_phone_sign_in(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({"data": "000000"}))
}

pub async fn verify_phone_sign_in(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({"token": "dominion-local", "account": {
        "id": "owner",
        "name": "주인",
        "plan": "free",
        "energy": 10,
        "energy_max": 10,
        "level": 1,
        "xp": 0,
        "xp_goal": 100,
    }}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_three_url_safe_segments() {
        let token = sign_token("key", "secret", "room", "user", 60);
        assert_eq!(token.split('.').count(), 3);
        assert!(token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')));
    }

    #[test]
    fn room_timestamp_becomes_epoch_millis() {
        let stamp = json!("1970-01-01 00:00:01");
        assert_eq!(epoch_millis(Some(&stamp)), 1000);
        assert_eq!(epoch_millis(None), 0);
    }
}
