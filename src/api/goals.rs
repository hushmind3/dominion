//! Public goal endpoints. All work runs through the native Rust job worker.
use crate::{engine, state::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use super::response::error;

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("goal")))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("goal", &id) {
        Some(goal) => Json(goal).into_response(),
        None => error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다."),
    }
}

/// Only user-editable fields are accepted. Runtime status, evidence and IDs
/// belong to the engine and cannot be supplied by an external SDK/client.
pub async fn create(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let Some(text) = body.get("goal").and_then(Value::as_str).map(str::trim)
        .filter(|text| !text.is_empty() && text.len() <= 64_000)
    else {
        return error(StatusCode::BAD_REQUEST, "1~64000바이트의 목표 내용이 필요합니다.");
    };
    let criteria = body.get("successCriteria").cloned().unwrap_or_else(|| json!([]));
    if let Err(message) = engine::goals::validate_criteria(&criteria) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let max_iterations = match bounded(&body, "maxIterations", 16, 1, 64) {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    let max_replans = match bounded(&body, "maxReplans", 6, 0, 16) {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    let replan_after = match bounded(&body, "replanAfter", 2, 1, 8) {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    let mut roles = Vec::new();
    for key in ["executor", "planner", "reviewer"] {
        let value = body.get(key).cloned().unwrap_or_else(|| json!({}));
        if !value.is_object() {
            return error(StatusCode::BAD_REQUEST, "실행·계획·검토 설정은 객체여야 합니다.");
        }
        if let Some(id) = value.get("agentId").and_then(Value::as_str) {
            if state.store.get("agent", id).is_none() {
                return error(StatusCode::BAD_REQUEST, "등록되지 않은 에이전트입니다.");
            }
        }
        // Never copy arbitrary client-provided control state into a role.
        let mut clean = json!({});
        for field in ["agentId", "providerId", "model"] {
            if let Some(value) = value.get(field).and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty() && value.len() <= 512)
            {
                clean[field] = json!(value);
            }
        }
        roles.push(clean);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let job_id = format!("goal:{id}");
    let record = json!({
        "id": id,
        "jobId": job_id,
        "goal": text,
        "status": "queued",
        "phase": "queued",
        "iterations": 0,
        "maxIterations": max_iterations,
        "maxReplans": max_replans,
        "replanAfter": replan_after,
        "successCriteria": criteria,
        "executor": roles[0],
        "planner": roles[1],
        "reviewer": roles[2],
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    if state.store.put_if_absent("goal", &record).is_none() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "목표를 저장할 수 없습니다.");
    }
    // Deterministic job ID links the goal to its single, durable execution.
    let job = engine::jobs::enqueue_once(
        &state, &job_id, "goal", json!({"goalId": id}),
    );
    let Some(job) = job else {
        state.store.remove("goal", &id);
        return error(StatusCode::INTERNAL_SERVER_ERROR, "목표 작업을 등록할 수 없습니다.");
    };
    state.bus.record("goal", &record);
    (StatusCode::ACCEPTED, Json(json!({"goal": record, "job": job}))).into_response()
}

fn bounded(body: &Value, key: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    match body.get(key) {
        None => Ok(default),
        Some(value) => match value.as_u64() {
            Some(number) if (min..=max).contains(&number) => Ok(number),
            _ => Err(format!("{key} 범위는 {min}~{max}입니다.")),
        },
    }
}

/// Cancellation is durable. A stopped goal is never silently restarted.
pub async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(goal) = state.store.get("goal", &id) else {
        return error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다.");
    };
    if goal["status"] == "cancelled" {
        return Json(goal).into_response();
    }
    if !matches!(goal["status"].as_str(), Some("queued" | "running")) {
        return error(StatusCode::CONFLICT, "진행 중인 목표만 중단할 수 있습니다.");
    }
    let job_id = goal["jobId"].as_str().map(str::to_string)
        .unwrap_or_else(|| format!("goal:{id}"));
    let Some(job) = state.store.get("job", &job_id) else {
        return error(StatusCode::CONFLICT, "목표 작업 기록을 찾을 수 없습니다.");
    };
    if !matches!(job["status"].as_str(), Some("queued" | "running")) {
        return error(StatusCode::CONFLICT, "이미 종료된 목표 작업입니다.");
    }
    let Some(saved_job) = state.store.patch_if(
        "job", &job_id, "status", &job["status"],
        &json!({"status":"cancelled","cancelRequested":true,
                "finishedAt":chrono::Utc::now().to_rfc3339()}),
    ) else {
        return error(StatusCode::CONFLICT, "목표 작업 상태가 변경되었습니다.");
    };
    if let Some(session_id) = goal["sessionId"].as_str() {
        state.cancel_session(session_id);
    }
    state.bus.record("job", &saved_job);
    let saved = state.store.patch_if(
        "goal", &id, "status", &goal["status"],
        &json!({"status":"cancelled","phase":"cancelled",
                "finishedAt":chrono::Utc::now().to_rfc3339()}),
    );
    match saved {
        Some(goal) => {
            state.bus.record("goal", &goal);
            Json(goal).into_response()
        }
        None => state.store.get("goal", &id)
            .map(|goal| Json(goal).into_response())
            .unwrap_or_else(|| error(StatusCode::CONFLICT, "목표 기록이 변경되었습니다.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppState {
        let root = std::env::temp_dir().join(
            format!("dominion-goal-api-{}", uuid::Uuid::new_v4())
        );
        let config = crate::config::Config {
            port: 0, data_dir: root.clone(), web_dir: root.join("web"), workspace: root,
        };
        AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        )
    }

    #[tokio::test]
    async fn goal_api_rejects_invalid_criteria_and_never_trusts_client_status() {
        let state = state();
        let bad = create(State(state.clone()), Json(json!({
            "goal":"write report.md",
            "successCriteria":[{"type":"unverifiable"}]
        }))).await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        assert!(state.store.all("goal").is_empty());

        let response = create(State(state.clone()), Json(json!({
            "goal":"write report.md",
            "status":"completed",
            "jobId":"forged",
            "evidence":[{"tool":"forged"}],
            "maxIterations":3,
            "successCriteria":[{"type":"file_exists","path":"report.md"}]
        }))).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let result: Value = serde_json::from_slice(&body).unwrap();
        let goal = &result["goal"];
        assert_eq!(goal["status"], "queued");
        assert_eq!(goal["maxIterations"], 3);
        assert!(goal["evidence"].is_null());
        assert_ne!(goal["jobId"], "forged");
        assert_eq!(result["job"]["kind"], "goal");
        assert_eq!(result["job"]["input"]["goalId"], goal["id"]);

        let id = goal["id"].as_str().unwrap();
        let stopped = cancel(State(state.clone()), Path(id.to_string())).await;
        assert_eq!(stopped.status(), StatusCode::OK);
        assert_eq!(state.store.get("goal", id).unwrap()["status"], "cancelled");
        assert_eq!(
            state.store.get("job", goal["jobId"].as_str().unwrap()).unwrap()["status"],
            "cancelled"
        );
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }
}
