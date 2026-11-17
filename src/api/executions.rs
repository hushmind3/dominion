use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Default)]
pub struct Page {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Executions are engine-owned audit records. Clients may read and cancel
/// them, never create or overwrite status/evidence. The list is bounded by
/// default because individual execution records include large evidence blobs.
fn query_result(items:Result<Vec<Value>,String>)->Response{
    match items {
        Ok(items)=>Json(items).into_response(),
        Err(_)=>(StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"실행 기록을 조회할 수 없습니다."}))).into_response(),
    }
}

pub async fn list(State(state): State<AppState>, Query(page): Query<Page>) -> Response {
    let limit=page.limit.unwrap_or(200).clamp(1,1000);
    let offset=page.offset.unwrap_or(0);
    query_result(state.store.execution_page(limit,offset))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("execution", &id) {
        Some(execution) => Json(execution).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "실행 기록을 찾을 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(execution) = state.store.get("execution", &id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "실행 기록을 찾을 수 없습니다." })),
        )
            .into_response();
    };
    if execution["status"].as_str() != Some("running") {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "실행 중인 기록만 중단할 수 있습니다." })),
        )
            .into_response();
    }
    // Claim cancellation in the same SQLite status transition that the
    // finalizer checks. A failed claim (already completed) must not cancel
    // the session or affect unrelated background work.
    let now = chrono::Utc::now().to_rfc3339();
    match state.store.patch_if(
        "execution",
        &id,
        "status",
        &json!("running"),
        &json!({"status":"cancelled","reason":"사용자 요청으로 중단","finishedAt":now}),
    ) {
        Some(saved) => {
            // The running->cancelled patch is durable before notifying the
            // worker. Late provider responses cannot revive the execution.
            if let Some(session) = execution["sessionId"].as_str() {
                state.cancel_session(session);
            }
            state.bus.record("execution", &saved);
            Json(saved).into_response()
        }
        None => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "실행 상태가 이미 변경되었습니다." })),
        )
            .into_response(),
    }
}

pub async fn active_executions(State(state): State<AppState>) -> Response {
    query_result(state.store.active_executions_indexed())
}

pub async fn execution_operations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    // Keep recent-first client ordering, but scope the SQL projection to
    // the requested run instead of parsing every operation in the database.
    query_result(state.store.execution_operations_recent(&id))
}
