use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use dominion_core::run_event::{RunEventPage,RunEventRef};
use serde_json::{json, Value};

#[derive(Deserialize, Default)]
pub struct Page {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Executions are engine-owned audit records. Clients may read and cancel
/// them, never create or overwrite status/evidence. The list is bounded by
/// default because individual execution records include large evidence blobs.
pub async fn list(State(state): State<AppState>, Query(page): Query<Page>) -> Json<Value> {
    let mut items = state.store.all("execution");
    items.sort_by(|a, b| {
        b["startedAt"]
            .as_str()
            .or_else(|| b["createdAt"].as_str())
            .unwrap_or("")
            .cmp(
                a["startedAt"]
                    .as_str()
                    .or_else(|| a["createdAt"].as_str())
                    .unwrap_or(""),
            )
    });
    let limit = page.limit.unwrap_or(200).clamp(1, 1000);
    let offset = page.offset.unwrap_or(0);
    Json(json!(
        items
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<Value>>()
    ))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("execution", &id) {
        Some(mut execution) => {
            if let Some(tree)=execution["budgetBinding"]["treeId"].as_str().and_then(|id|state.store.get("budget_tree",id)){
                execution["hierarchicalBudget"]=tree;
            }
            Json(execution).into_response()
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "실행 기록을 찾을 수 없습니다." })),
        )
            .into_response(),
    }
}


/// Inspect only the requested run's persisted decisions and algorithm usage.
pub async fn decisions(State(state):State<AppState>,Path(id):Path<String>)->Response {
    if state.store.get("execution",&id).is_none(){
        return (StatusCode::NOT_FOUND,Json(json!({"error":"실행 기록을 찾을 수 없습니다."}))).into_response();
    }
    match state.store.algorithm_usages_for_execution(&id){
        Ok(usages)=>Json(json!({"executionId":id,"strategyDecision":state.store.get("strategy_decision",&id),"algorithmUsages":usages})).into_response(),
        Err(_)=>(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":"알고리즘 사용 기록 조회 실패"}))).into_response()
    }
}

#[derive(Deserialize,Default)]
pub struct EventCursor {
    pub after:Option<u64>,
    pub limit:Option<usize>,
}

/// Reconnect-safe native run timeline. Never substitute a room ID, task ID or
/// SDK facade ID for the independently recorded execution ID.
pub async fn run_events(
    State(state):State<AppState>,Path(id):Path<String>,Query(cursor):Query<EventCursor>,
)->Response{
    if state.store.get("execution",&id).is_none(){
        return (StatusCode::NOT_FOUND,Json(json!({"error":"실행 기록을 찾을 수 없습니다."}))).into_response();
    }
    let after=cursor.after.unwrap_or(0);
    let max=cursor.limit.unwrap_or(100).clamp(1,500);
    let mut events=state.bus.replay_run(&id,after,max+1);
    let has_more=events.len()>max;
    events.truncate(max);
    let next_after=events.last().map(|e|e.seq).unwrap_or(after);
    let earliest_available_seq=state.bus.earliest_event_seq();
    let page=RunEventPage{
        schema_version:RunEventRef::SCHEMA_VERSION,
        execution_id:id,after,next_after,has_more,
        history_truncated:earliest_available_seq
            .is_some_and(|first|after<first.saturating_sub(1)),
        earliest_available_seq,events
    };
    Json(page).into_response()
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
    if let Some(session) = execution["sessionId"].as_str() {
        state.cancel_session(session);
    }
    let now = chrono::Utc::now().to_rfc3339();
    match state.store.patch_if(
        "execution",
        &id,
        "status",
        &json!("running"),
        &json!({
            "status":"cancelled",
            "phase": crate::engine::run_state::CANCELLED,
            "phaseUpdatedAt": now,
            "reason":"사용자 요청으로 중단",
            "finishedAt": now,
        }),
    ) {
        Some(saved) => {
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

pub async fn active_executions(State(state): State<AppState>) -> Json<Value> {
    let items = state
        .store
        .all("execution")
        .into_iter()
        .filter(|execution| {
            execution.get("status").and_then(|value| value.as_str()) == Some("running")
        })
        .collect::<Vec<_>>();
    Json(json!(items))
}

pub async fn execution_operations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let items = state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|record| record.get("runId").and_then(|value| value.as_str()) == Some(id.as_str()))
        .collect::<Vec<_>>();
    Json(json!(items))
}

pub async fn reconcile_budget(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(execution)=state.store.get("execution",&id)else{return (StatusCode::NOT_FOUND,Json(json!({"error":"실행이 없습니다."}))).into_response()};
    let (Some(call),Some(charged),Some(details))=(body["callId"].as_str(),body["charged"].as_u64(),body["details"].as_str())else{
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"callId, charged(계정 단위), details가 필요합니다."}))).into_response();
    };
    match crate::engine::execution_budget::reconcile(&state,&execution,call,charged,details){
        Ok(tree)=>Json(tree).into_response(),Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};


    #[tokio::test]
    async fn decision_view_is_scoped_to_exact_native_execution(){
        let dir=std::env::temp_dir().join(format!("dominion-decision-view-{}",uuid::Uuid::new_v4()));
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        for id in ["a","b"]{
            state.store.put("execution",&json!({"id":id})).unwrap();
            state.store.put("strategy_decision",&json!({"id":id,"selected":format!("bundle:{id}")})).unwrap();
            state.store.put("algorithm_usage",&json!({"id":format!("usage:{id}"),"executionId":id,"kind":"memory.retrieval"})).unwrap();
        }
        let response=decisions(State(state.clone()),Path("a".into())).await;
        assert_eq!(response.status(),StatusCode::OK);
        let bytes=axum::body::to_bytes(response.into_body(),65536).await.unwrap();
        let view:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view["executionId"],"a");
        assert_eq!(view["strategyDecision"]["selected"],"bundle:a");
        assert_eq!(view["algorithmUsages"].as_array().unwrap().len(),1);
        assert_eq!(view["algorithmUsages"][0]["executionId"],"a");
        assert_eq!(decisions(State(state.clone()),Path("missing".into())).await.status(),StatusCode::NOT_FOUND);
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn execution_timeline_pages_do_not_leak_other_runs_or_replay_duplicates(){
        let dir=std::env::temp_dir().join(format!("dominion-run-events-{}",uuid::Uuid::new_v4()));
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        state.store.put("execution",&json!({"id":"exec-a","status":"running"})).unwrap();
        state.store.put("execution",&json!({"id":"exec-b","status":"running"})).unwrap();
        state.bus.record("execution",&json!({"id":"exec-a","status":"running"}));
        state.bus.record("execution",&json!({"id":"exec-b","status":"running"}));
        state.bus.record("execution",&json!({"id":"exec-a","status":"completed"}));
        let first=run_events(State(state.clone()),Path("exec-a".into()),
            Query(EventCursor{after:Some(0),limit:Some(1)})).await;
        assert_eq!(first.status(),StatusCode::OK);
        let bytes=axum::body::to_bytes(first.into_body(),1024*1024).await.unwrap();
        let first:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(first["schemaVersion"],1);
        assert_eq!(first["events"].as_array().unwrap().len(),1);
        assert_eq!(first["hasMore"],true);
        assert_eq!(first["events"][0]["runRef"]["executionId"],"exec-a");
        let cursor=first["nextAfter"].as_u64().unwrap();
        let second=run_events(State(state.clone()),Path("exec-a".into()),
            Query(EventCursor{after:Some(cursor),limit:Some(10)})).await;
        let bytes=axum::body::to_bytes(second.into_body(),1024*1024).await.unwrap();
        let second:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(second["events"].as_array().unwrap().len(),1);
        assert_eq!(second["hasMore"],false);
        assert_eq!(second["events"][0]["seq"],3);
        assert_eq!(second["events"][0]["runRef"]["lifecycle"],"completed");
        assert_eq!(run_events(State(state.clone()),Path("nonexistent".into()),
            Query(EventCursor::default())).await.status(),StatusCode::NOT_FOUND);
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn execution_cancel_commits_status_and_phase_in_the_same_patch() {
        let dir = std::env::temp_dir().join(format!("dominion-execution-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"s"}));
        state.store.put("execution", &json!({
            "id":"e","sessionId":"s","status":"running","phase":"running",
            "kernelState":{"native":{"id":"unchanged"}}
        }));
        let response = cancel(State(state.clone()), Path("e".into())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let record = state.store.get("execution", "e").unwrap();
        assert_eq!(record["status"], "cancelled");
        assert_eq!(record["phase"], "cancelled");
        assert_eq!(record["phaseUpdatedAt"], record["finishedAt"]);
        assert!(record["finishedAt"].is_string());
        assert_eq!(record["kernelState"]["native"]["id"], "unchanged");
        // An in-flight stale phase event must never reopen a cancelled run.
        crate::engine::run_state::apply(&state, "e", "verify");
        let after = state.store.get("execution", "e").unwrap();
        assert_eq!(after["status"], "cancelled");
        assert_eq!(after["phase"], "cancelled");
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }
}
