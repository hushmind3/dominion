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
