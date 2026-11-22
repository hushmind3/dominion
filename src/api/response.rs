use axum::{http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json};

pub(super) fn error(status: StatusCode, message: &str) -> Response {
    // Every API error body is read by the owner, so upstream text is turned
    // into a sentence here instead of at each handler.
    (status, Json(json!({ "error": crate::plain::error(message) }))).into_response()
}

