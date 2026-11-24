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

#[derive(Deserialize,Default)]
pub struct EventPage {
    pub after:Option<u64>,
    pub limit:Option<usize>,
}
pub async fn events(State(state):State<AppState>,Path(id):Path<String>,
    Query(page):Query<EventPage>)->Response{
    if state.store.get("execution",&id).is_none(){
        return super::response::error(StatusCode::NOT_FOUND,"실행 기록을 찾을 수 없습니다.");
    }
    let after=page.after.unwrap_or(0);
    if after>i64::MAX as u64{
        return super::response::error(StatusCode::BAD_REQUEST,"이벤트 위치가 올바르지 않습니다.");
    }
    match state.store.execution_event_page(&id,after,page.limit.unwrap_or(80)){
        Ok(value)=>Json(value).into_response(),
        Err(_)=>super::response::error(StatusCode::INTERNAL_SERVER_ERROR,"실행 이벤트를 조회할 수 없습니다."),
    }
}
#[cfg(test)]
mod event_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[tokio::test]
    async fn execution_events_are_scoped_paginated_and_redacted(){
        let root=std::env::temp_dir().join(format!("dominion-execution-events-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        for id in ["a","b"]{
            state.store.put("execution",&json!({"id":id,"status":"running"})).unwrap();
        }
        for (kind,id,status) in [("execution","a","running"),("execution","b","completed"),
            ("job","a","running"),("execution","a","completed")]{
            state.bus.record(kind,&json!({"id":id,"status":status,"secret":"PRIVATE"}));
        }
        let response=events(State(state.clone()),Path("a".into()),Query(EventPage{after:Some(0),limit:Some(1)})).await;
        assert_eq!(response.status(),StatusCode::OK);
        let page:Value=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),usize::MAX).await.unwrap()).unwrap();
        assert_eq!(page["events"].as_array().unwrap().len(),1);
        assert_eq!(page["events"][0]["runRef"]["runId"],"a");
        assert_eq!(page["events"][0]["phase"],"running");
        assert_eq!(page["hasMore"],true);
        let cursor=page["nextAfter"].as_u64().unwrap();
        let next=state.store.execution_event_page("a",cursor,1).unwrap();
        assert_eq!(next["events"][0]["phase"],"succeeded");
        assert_eq!(next["hasMore"],false);
        assert!(!next.to_string().contains("PRIVATE"));
        assert_eq!(state.store.execution_event_page("a",next["nextAfter"].as_u64().unwrap(),1).unwrap()["events"],json!([]));
        assert_eq!(events(State(state.clone()),Path("missing".into()),Query(EventPage::default())).await.status(),StatusCode::NOT_FOUND);
        for _ in 0..2300{state.bus.emit(json!({"type":"tick"}));}
        assert_eq!(state.store.execution_event_page("a",0,80).unwrap()["historyTruncated"],true);
        drop(state);let _=std::fs::remove_dir_all(root);
    }
}
