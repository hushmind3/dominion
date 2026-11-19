use crate::engine::dev;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub async fn get_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("dev_job", &id) {
        Some(job) => Json(job).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "업데이트 작업을 찾을 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn update_status(State(state): State<AppState>) -> Json<Value> {
    Json(dev::update_status(&state).await)
}

pub async fn update(State(state): State<AppState>, Json(_body): Json<Value>) -> Response {
    match dev::start_update_job(state) {
        Ok(job) => Json(job).into_response(),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": error })),
        )
            .into_response(),
    }
}
