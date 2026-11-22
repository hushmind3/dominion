use crate::{engine::personas, state::AppState};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub async fn preview(State(state): State<AppState>) -> Json<Value> {
    Json(personas::preview(&state))
}
pub async fn import(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match personas::import(&state, &body) {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))).into_response(),
    }
}
pub async fn avatar(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match personas::avatar(&state, &id) {
        Ok((bytes, mime)) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "private, max-age=60".into()),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            ],
            bytes,
        )
            .into_response(),
        // A missing portrait is a data gap, not a client error. Serving a
        // neutral placeholder keeps the console clean and the list calm.
        Err(_) => (
            [
                (header::CONTENT_TYPE, "image/svg+xml".to_string()),
                (header::CACHE_CONTROL, "private, max-age=30".to_string()),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            ],
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="18" fill="#F3EEE5"/><circle cx="32" cy="25" r="9" fill="#A79C8E"/><path d="M14 56c3-11 10-16 18-16s15 5 18 16z" fill="#A79C8E"/></svg>"##,
        )
            .into_response(),
    }
}
