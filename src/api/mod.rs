mod http_policy;
mod routes;
use http_policy::{api_not_found, guard_local_host, guard_writes, maintenance_guard, no_stale_ui};
pub mod agents;
pub mod algorithms;
pub mod approvals;
pub mod attachments;
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
pub mod extensions;
pub mod plugins;
pub mod gateways;
pub mod improvements;
pub mod jobs;
pub mod memory;
pub mod models;
pub mod native_replay;
pub mod oauth;
pub mod personas;
pub mod providers;
pub mod remote_artifacts;
pub mod remote_jobs;
pub mod remote_registry;
pub mod remote_workers;
mod response;
pub mod rooms;
pub mod runs;
pub mod sdk;
pub mod sdk_runs;
pub mod sessions;
pub mod settings;
pub mod system;
pub mod task_teams;
pub mod tasks;
#[cfg(test)]
mod tests;
pub mod translation;

use crate::state::AppState;
use axum::{
    http::{header, HeaderName, HeaderValue, Method},
    middleware,
    routing::{any, get, post},
    Router,
};
use tower_http::{cors::CorsLayer, services::ServeDir};

pub fn router(state: AppState) -> Router {
    let mut router = routes::router();

    for kind in collections::COLLECTIONS {
        router = router
            .route(
                &format!("/api/{kind}"),
                get(collections::list).post(collections::save),
            )
            .route(
                &format!("/api/{kind}/:id"),
                get(collections::one)
                    .delete(collections::remove)
                    .post(collections::save),
            )
            .route(
                &format!("/api/{kind}/:id/:action"),
                post(collections::action),
            );
    }

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
        // Browsers must not read local sessions or API metadata by using a
        // rebinding DNS name as the HTTP Host/HTTP2 authority. CORS and the
        // write-only Origin check alone do not protect same-origin rebinding.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            guard_local_host,
        ))
        .layer(cors)
        .layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024))
        .with_state(state)
}
