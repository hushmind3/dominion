use crate::state::AppState;
use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// Record kinds that behave as plain JSON collections. The React client
/// already talks to these paths, so the Rust backend just needs to persist
/// them with the same contract.
pub const COLLECTIONS: &[&str] = &[
    "memories",
    "skills",
    "mcp",
    "knowledge",
];

/// Only the four routes in COLLECTIONS use this mapper. Other domain routes
/// have dedicated API handlers and should not leak aliases into this layer.
pub fn store_kind(route_kind: &str) -> &str {
    match route_kind {
        "memories" => "memory",
        "skills" => "skill",
        other => other,
    }
}

fn kind_from_uri(uri: &axum::http::Uri) -> String {
    store_kind(
        uri.path()
            .trim_start_matches('/')
            .split('/')
            .nth(1)
            .unwrap_or_default(),
    )
    .to_string()
}

fn id_from_uri(uri: &axum::http::Uri) -> Option<String> {
    uri.path()
        .trim_start_matches('/')
        .split('/')
        .nth(2)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

pub async fn list(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Json<Value> {
    let kind = kind_from_uri(&uri);
    let mut records = state.store.all(&kind);
    if kind == "memory" {
        for record in &mut records {
            let eligible = crate::engine::memory::retrieval_eligible(record);
            if let Some(fields) = record.as_object_mut() {
                fields.insert("retrievalEligible".into(), json!(eligible));
            }
        }
    }
    Json(json!(records))
}

pub async fn save(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let kind = kind_from_uri(&uri);
    if !body.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "레코드 객체가 필요합니다." })),
        )
            .into_response();
    }
    let mut body = body;
    if kind == "mcp" {
        let id = body
            .get("id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        body["id"] = json!(id);
        if let Some(key) = body
            .get("apiKey")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty() && !value.starts_with("••"))
            .map(str::to_string)
        {
            state.save_secret(&format!("mcp:{id}"), &key);
        }
        if let Value::Object(ref mut map) = body {
            map.remove("apiKey");
        }
        crate::engine::mcp::invalidate();
    }
    if kind == "skill" {
        // The execution path ignores `draft` even when `enabled` is true.
        // Make the public toggle and the engine contract agree.
        let enabled = body["enabled"].as_bool() == Some(true);
        if enabled {
            let status = body["status"].as_str().unwrap_or("");
            if status.is_empty() || status == "draft" {
                body["status"] = json!("active");
            }
        }
    }
    match state.store.put(&kind, &body) {
        Some(record) => {
            state.bus.record(&kind, &record);
            Json(record).into_response()
        }
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "레코드를 저장할 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn one(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Response {
    let kind = kind_from_uri(&uri);
    match id_from_uri(&uri) {
        Some(id) => match state.store.get(&kind, &id) {
            Some(record) => Json(record).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("{kind} 레코드를 찾을 수 없습니다.") })),
            )
                .into_response(),
        },
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "id 가 필요합니다." })),
        )
            .into_response(),
    }
}

pub async fn remove(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Json<Value> {
    let kind = kind_from_uri(&uri);
    if let Some(id) = id_from_uri(&uri) {
        if kind == "mcp" {
            state.store.remove_secret(&format!("mcp:{id}"));
            crate::engine::mcp::invalidate();
        }
        state.store.remove(&kind, &id);
        state
            .bus
            .emit(json!({ "type": "record", "kind": kind, "removed": id }));
    }
    Json(json!({ "ok": true }))
}

/// Legacy collection actions. Room execution and channel diagnostics have
/// dedicated API routes; do not duplicate dispatch inside generic storage.
pub async fn action(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let kind = kind_from_uri(&uri);
    let id = id_from_uri(&uri).unwrap_or_default();
    let action = uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .nth(3)
        .unwrap_or_default()
        .to_string();

    match (kind.as_str(), action.as_str()) {
        ("memory", "correct") => {
            let Some(text)=body["text"].as_str().map(str::trim)
                .filter(|text|!text.is_empty()&&text.chars().count()<=16_000) else {
                return (StatusCode::BAD_REQUEST,
                    Json(json!({"error":"기억 수정 내용은 1~16,000자여야 합니다."}))).into_response();
            };
            let now=chrono::Utc::now().to_rfc3339();
            let new_id=uuid::Uuid::new_v4().to_string();
            let mut corrected=Value::Null;
            let old=state.store.mutate_record("memory",&id,Value::Null,|previous|{
                if !crate::engine::memory::retrieval_eligible(previous){
                    return Err("비활성 또는 만료된 기억은 수정할 수 없습니다.".into());
                }
                if previous["text"].as_str().is_some_and(|old|old.trim()==text){
                    return Err("기존 기억과 내용이 동일합니다.".into());
                }
                let mut revision=previous.clone();
                revision["id"]=json!(new_id);
                revision["text"]=json!(text);
                revision["status"]=json!("active");
                revision["source"]=json!("user");
                revision["verification"]=json!("owner_correction");
                revision["supersedes"]=json!(id);
                revision["createdAt"]=json!(now);
                revision["updatedAt"]=json!(now);
                revision["lastUsedAt"]=Value::Null;
                revision["sourceMessageIds"]=json!([]);
                revision["evidenceQuote"]=Value::Null;
                revision["sourceRole"]=json!("owner");
                revision["sourceAuthors"]=json!([]);
                revision["sourceAuthorKey"]=json!("owner");
                if let Some(map)=revision.as_object_mut(){
                    for field in ["supersededBy","archiveReason","forgetReason","learnedAt","retrievalEligible"]{
                        map.remove(field);
                    }
                }
                revision["revision"]=json!(crate::engine::memory::content_revision(&revision));
                previous["status"]=json!("superseded");
                previous["supersededBy"]=json!(new_id);
                previous["archiveReason"]=json!("owner_correction");
                previous["revision"]=json!(crate::engine::memory::content_revision(previous));
                corrected=revision.clone();
                Ok(vec![("memory".into(),revision)])
            });
            match old{
                Ok(previous)=>{
                    state.bus.record("memory",&previous);
                    state.bus.record("memory",&corrected);
                    Json(json!({"previous":previous,"record":corrected})).into_response()
                },
                Err(message)=>(StatusCode::BAD_REQUEST,Json(json!({"error":message}))).into_response(),
            }
        },
        ("skill", "export") => match state.store.get("skill", &id) {
            Some(skill) => Json(skill).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "스킬을 찾을 수 없습니다." })),
            )
                .into_response(),
        },
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 작업입니다: {kind}/{action}") })),
        )
            .into_response(),
    }
}

