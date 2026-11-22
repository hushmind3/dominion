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

/// URL segments are plural (`/api/jobs`) while the store groups records by a
/// singular kind (`job`), matching the Node data model.
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


/// The consumer memory editor creates a new owner-corrected revision while
/// retaining the original evidence and immutable scope on the old record.
fn memory_revision(previous:&Value,text:&str)->(Value,Value){
    let new_id=uuid::Uuid::new_v4().to_string();
    let mut revised=previous.clone();
    revised["id"]=json!(new_id);
    revised["text"]=json!(text);
    revised["status"]=json!("active");
    revised["verification"]=json!("owner_correction");
    revised["source"]=json!("user");
    revised["sourceMessageIds"]=json!([]);
    revised["evidenceQuote"]=Value::Null;
    revised["supersedes"]=previous["id"].clone();
    if let Some(fields)=revised.as_object_mut(){
        fields.remove("supersededBy");
        fields.remove("sourceAuthors");
        fields.remove("sourceRole");
    }
    revised["createdAt"]=json!(chrono::Utc::now().to_rfc3339());
    revised["revision"]=json!(crate::engine::memory::content_revision(&revised));
    let mut prior=previous.clone();
    prior["status"]=json!("superseded");
    prior["supersededBy"]=json!(new_id);
    prior["revision"]=json!(crate::engine::memory::content_revision(&prior));
    (prior,revised)
}
pub async fn correct_memory(
    State(state):State<AppState>,
    axum::extract::Path(id):axum::extract::Path<String>,
    Json(body):Json<Value>,
)->Response{
    let Some(text)=body["text"].as_str().map(str::trim)
        .filter(|text|!text.is_empty()&&text.chars().count()<=1600)
    else{return (StatusCode::BAD_REQUEST,Json(json!({"error":"기억 수정 내용은 1~1600자여야 합니다."}))).into_response()};
    let Some(previous)=state.store.get("memory",&id)else{
        return (StatusCode::NOT_FOUND,Json(json!({"error":"기억 기록이 없습니다."}))).into_response();
    };
    if previous["status"]!="active"{
        return (StatusCode::CONFLICT,Json(json!({"error":"활성 기억만 수정할 수 있습니다."}))).into_response();
    }
    let (prior,revised)=memory_revision(&previous,text);
    // Both records commit together under the original status guard; the
    // memory retriever never sees a superseded original with no replacement.
    match state.store.transaction_if_status("memory",&id,"active", &[
        ("memory".to_string(),prior.clone()),
        ("memory".to_string(),revised.clone()),
    ]){
        Ok(true)=>{
            state.bus.record("memory",&prior);
            state.bus.record("memory",&revised);
            Json(revised).into_response()
        }
        Ok(false)=>(StatusCode::CONFLICT,Json(json!({"error":"기억이 이미 변경되었습니다."}))).into_response(),
        Err(error)=>(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":error}))).into_response(),
    }
}

#[cfg(test)]
mod memory_revision_contract_tests{
    use super::*;
    #[test]
    fn correction_preserves_scope_and_stales_old_evidence(){
        let old=json!({"id":"m1","text":"old","scope":"agent",
            "agentId":"a1","partitionKey":"tenant","status":"active",
            "sourceMessageIds":["message-1"],"evidenceQuote":"old"});
        let (prior,next)=memory_revision(&old,"new corrected fact");
        assert_eq!(prior["status"],"superseded");
        assert_eq!(prior["supersededBy"],next["id"]);
        assert_eq!(next["supersedes"],"m1");
        assert_eq!(next["partitionKey"],"tenant");
        assert_eq!(next["agentId"],"a1");
        assert_eq!(next["verification"],"owner_correction");
        assert_eq!(next["sourceMessageIds"],json!([]));
        assert!(next["evidenceQuote"].is_null());
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
