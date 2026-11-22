//! Local owner, remote worker and maintenance admission share one boundary.
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tower_http::cors::CorsLayer;

fn local_origins(port: u16) -> [String; 2] {
    [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
    ]
}

pub(super) fn cors(port: u16) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(
            local_origins(port)
                .map(|origin| origin.parse::<HeaderValue>().expect("loopback origin")),
        )
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
        ])
}

/// HTML is served without a long-lived cache so a rebuilt UI is picked up
/// immediately instead of showing an old bundle after a restart.
pub(super) async fn no_stale_ui(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// While maintenance mode is on, only reads and `/api/system/*` mutations are
/// allowed. Restore cannot race with normal writes.
pub(super) async fn maintenance_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    // A failed rollback or a crash during restore leaves a durable marker.
    // Do not serve potentially mismatched database/key state until restart
    // recovery has settled that marker before Store and Vault are opened.
    if request.uri().path().starts_with("/api/")
        && crate::engine::restore_transaction::pending(&state.config.data_dir)
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"backup restore recovery pending; restart required"})),
        )
            .into_response();
    }
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

/// Worker-only ingress has its own mandatory per-device or legacy secret.
/// It must not require the separate owner admin credential.
fn worker_ingress(path: &str) -> bool {
    path == "/api/remote/claim"
        || (path.starts_with("/api/remote/jobs/")
            && (path.ends_with("/heartbeat")
                || path.ends_with("/finish")
                || path.ends_with("/artifact/upload")))
}
/// Browser DNS rebinding and forwarded Host spoofing must not make owner
/// GET endpoints readable. The listener binds loopback; remote Worker ingress
/// has its own credential, while remote owner control always needs admin auth.
fn is_local_owner_host(host: &str, port: u16) -> bool {
    let normalized = host.to_ascii_lowercase();
    normalized == format!("localhost:{port}") || normalized == format!("127.0.0.1:{port}")
}
pub(super) async fn guard_owner_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    // Remote coordinator endpoints are handled by their dedicated admin token
    // guard; device ingress is authenticated by per-device credentials.
    if path.starts_with("/api/remote/") {
        return next.run(request).await;
    }
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    let local = host.is_some_and(|host| is_local_owner_host(host, state.config.port));
    // In-process Router::oneshot unit tests omit the Host header entirely.
    // Network binaries are compiled without cfg(test) and reject that case.
    let internal_test_request = cfg!(test) && host.is_none();
    if local || internal_test_request {
        return next.run(request).await;
    }
    // Explicitly authenticated reverse-proxied owner control is permitted.
    // Merely choosing a different Host or supplying an Origin cannot grant it.
    let configured = std::env::var("DOMINION_ADMIN_TOKEN").unwrap_or_default();
    let presented = request
        .headers()
        .get("x-dominion-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !configured.trim().is_empty() && presented == configured {
        return next.run(request).await;
    }
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error":"untrusted Host for local owner API"})),
    )
        .into_response()
}

/// Rejects cross-origin writes so a random website cannot drive the local
/// computer-control API. Owner-controlled distributed endpoints require an
/// explicit admin secret whenever exposed, regardless of the local default.
pub(super) async fn guard_writes(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let is_worker = worker_ingress(request.uri().path());
    let is_remote_owner = request.uri().path().starts_with("/api/remote/") && !is_worker;
    let configured = std::env::var("DOMINION_ADMIN_TOKEN").unwrap_or_default();
    if is_remote_owner {
        if configured.trim().is_empty() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Remote coordinator requires DOMINION_ADMIN_TOKEN"})),
            )
                .into_response();
        }
        let presented = request
            .headers()
            .get("x-dominion-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if presented != configured {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"remote owner admin token required"})),
            )
                .into_response();
        }
    }
    let method = request.method().clone();
    if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        let port = state.config.port;
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            let allowed = local_origins(port).iter().any(|allowed| allowed == origin);
            if !allowed {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "cross-origin write blocked" })),
                )
                    .into_response();
            }
        }
        if !is_worker && !configured.trim().is_empty() {
            let token = configured;
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
    next.run(request).await
}

#[cfg(test)]
mod remote_worker_route_tests {
    use super::*;
    #[test]
    fn only_loopback_host_and_exact_listener_port_are_local_owner_authority() {
        assert!(is_local_owner_host("127.0.0.1:4317", 4317));
        assert!(is_local_owner_host("LOCALHOST:4317", 4317));
        for rejected in [
            "evil.example:4317",
            "localhost.evil:4317",
            "127.0.0.1:9999",
            "localhost:4318",
            "127.0.0.1",
            "[::1]:4317",
            "127.0.0.1:4317.evil",
        ] {
            assert!(!is_local_owner_host(rejected, 4317), "{rejected}");
        }
    }
    #[test]
    fn remote_machine_write_paths_never_bypass_owner_admin_policy() {
        assert!(worker_ingress("/api/remote/claim"));
        assert!(worker_ingress("/api/remote/jobs/remote:123/heartbeat"));
        assert!(worker_ingress("/api/remote/jobs/remote:123/finish"));
        assert!(worker_ingress(
            "/api/remote/jobs/remote:123/artifact/upload"
        ));
        assert!(!worker_ingress(
            "/api/remote/jobs/remote:123/artifact/review"
        ));
        for path in [
            "/api/remote/workers",
            "/api/remote/workers/a/rotate",
            "/api/remote/workers/a/state",
            "/api/remote/jobs",
            "/api/remote/reconcile",
            "/api/remote/jobs/a/review",
            "/api/remote/jobs/a/cancel",
        ] {
            assert!(!worker_ingress(path), "{path}");
        }
    }
}
