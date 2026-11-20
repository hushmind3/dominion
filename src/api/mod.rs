mod response;
mod addons;
mod surfaces;
#[cfg(test)]
mod tests;
pub mod translation;
pub mod settings;
pub mod improvements;
pub mod executions;
pub mod tasks;
pub mod experience;
pub mod memory;
pub mod browser;
pub mod extensions;
pub mod approvals;
pub mod sessions;
pub mod agents;
pub mod providers;
pub mod cli_oauth;
pub mod benchmark;
pub mod experiments;
pub mod chat;
pub mod jobs;
pub mod sdk_bridge;
pub mod system;
pub mod attachments;
pub mod channels;
pub mod collections;
pub mod compat;
pub mod dev;
pub mod gateways;
pub mod models;
pub mod oauth;
pub mod personas;
pub mod rooms;

use crate::state::AppState;
use axum::{
    extract::{OriginalUri, Request, State},
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use serde_json::json;
use tower_http::{cors::CorsLayer, services::ServeDir};

pub fn router(state: AppState) -> Router {
    let mut router = Router::new()
        .route("/api/health", get(system::health))
        .route("/api/events", get(system::events))
        .route("/api/chat", post(chat::chat))
        .route("/api/sdk/runs", get(sdk_bridge::list).post(sdk_bridge::start))
        .route("/api/sdk/runs/:id", get(sdk_bridge::one))
        .route("/api/sdk/runs/:id/cancel", post(sdk_bridge::cancel))
        .route("/api/sdk/runs/:id/approve", post(sdk_bridge::approve))
        .route("/api/sdk/runs/:id/resume", post(sdk_bridge::resume))
        .route("/api/sdk/runs/:id/reconcile", post(sdk_bridge::reconcile))
        .route(
            "/api/executions/:id/operations",
            get(executions::execution_operations),
        )
        .route(
            "/api/approvals",
            get(approvals::list_approvals).post(approvals::resolve_approval),
        )
        ;

    // Optional integrations share the same global guard and runtime, but
    // own no core API routes or independent listener.
    router=router.merge(addons::routes());

    // Execution is the only intrinsic HTTP surface. Everything else is a
    // service module sharing this router's middleware and persisted state.
    router=router.merge(surfaces::routes());

    let static_files =
        ServeDir::new(state.config.web_dir.clone()).append_index_html_on_directories(true);

    let port = state.config.port;
    let origins = vec![
        format!("http://127.0.0.1:{port}")
            .parse::<HeaderValue>()
            .expect("origin"),
        format!("http://localhost:{port}")
            .parse::<HeaderValue>()
            .expect("origin"),
    ];
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-dominion-token"),
        ]);

    router
        .route("/api/*rest", any(api_not_found))
        .fallback_service(static_files)
        .layer(middleware::from_fn(no_stale_ui))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            maintenance_guard,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), guard_writes))
        .layer(cors)
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

/// HTML is served without a long-lived cache so a rebuilt UI is picked up
/// immediately instead of showing an old bundle after a restart.
async fn no_stale_ui(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// While maintenance mode is on, only reads and `/api/system/*` mutations are
/// allowed. Restore cannot race with normal writes.
async fn maintenance_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !state.maintenance_mode() {
        return next.run(request).await;
    }
    if request.method() == Method::GET || request.uri().path().starts_with("/api/system/") {
        return next.run(request).await;
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "유지보수 모드입니다. 복구가 끝난 뒤 다시 시도하세요.",
            "maintenance": true
        })),
    )
        .into_response()
}

/// Rejects cross-origin writes so a random website cannot drive the local
/// computer-control API. An optional admin token protects writes further.
async fn guard_writes(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let method = request.method().clone();
    if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        let port = state.config.port;
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            let allowed = origin == format!("http://127.0.0.1:{port}")
                || origin == format!("http://localhost:{port}");
            if !allowed {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "cross-origin write blocked" })),
                )
                    .into_response();
            }
        }
        if let Ok(token) = std::env::var("DOMINION_ADMIN_TOKEN") {
            if !token.trim().is_empty() {
                let provided = request
                    .headers()
                    .get("x-dominion-token")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("");
                if provided != token {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({ "error": "admin token required" })),
                    )
                        .into_response();
                }
            }
        }
    }
    next.run(request).await
}
