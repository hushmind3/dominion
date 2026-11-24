pub mod agents;
pub mod algorithms;
pub mod approvals;
pub mod attachments;
pub mod benchmark;
pub mod browser;
pub mod channels;
pub mod chat;
pub mod cli_oauth;
pub mod cloud_auth;
pub mod collections;
pub mod compat;
pub mod dev;
pub mod executions;
pub mod experience;
pub mod experiments;
pub mod extensions;
pub mod extension_registry;
pub mod extension_process;
pub mod gateways;
pub mod improvements;
pub mod jobs;
pub mod memory;
pub mod models;
pub mod oauth;
pub mod personas;
pub mod providers;
pub mod remote_artifacts;
pub mod remote_registry;
pub mod remote_workers;
mod response;
pub mod rooms;
pub mod sdk;
pub mod sessions;
pub mod settings;
pub mod system;
pub mod tasks;
#[cfg(test)]
mod tests;
pub mod translation;

mod http_policy;
mod routes;

use crate::state::AppState;
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use serde_json::json;
use tower_http::services::ServeDir;

/// Compose product routes before attaching the common transport and admission
/// layers. Keep this order: owner-host admission also covers CORS preflights.
pub fn router(state: AppState) -> Router {
    let static_files =
        ServeDir::new(state.config.web_dir.clone()).append_index_html_on_directories(true);
    routes::router()
        .route("/api/*rest", any(api_not_found))
        .fallback_service(static_files)
        .layer(middleware::from_fn(http_policy::no_stale_ui))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            http_policy::maintenance_guard,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            http_policy::guard_writes,
        ))
        .layer(http_policy::cors(state.config.port))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            http_policy::guard_owner_host,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024))
        .with_state(state)
}

async fn api_not_found(OriginalUri(uri): OriginalUri) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": format!("Rust 백엔드에 없는 API 경로입니다: {}", uri.path()),
            "ported": false,
        })),
    )
        .into_response()
}
