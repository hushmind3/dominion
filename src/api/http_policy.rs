//! Shared transport policy. Product route modules never attach middleware.
use crate::state::AppState;
use axum::{
    extract::{OriginalUri, Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub(super) async fn api_not_found(OriginalUri(uri): OriginalUri) -> Response {
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

/// Validate a supplied browser Host on every local-owner API request,
/// including GET, SSE and credential metadata reads. The standalone product
/// listens on 127.0.0.1 only; its remote worker gateway is a separate router.
///
/// Internal in-process requests may omit Host. A real browser cannot set or
/// remove its Host header, and HTTP/2 supplies the authority field.
fn local_host_allowed(request: &Request, port: u16) -> bool {
    let actual = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .or_else(|| request.uri().authority().map(|v| v.as_str()));
    match actual {
        None => true, // internal tests/in-process invocations, not browser traffic
        Some(host) => {
            host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}"))
                || host.eq_ignore_ascii_case(&format!("localhost:{port}"))
        }
    }
}
pub(super) async fn guard_local_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.uri().path().starts_with("/api/") && !local_host_allowed(&request, state.config.port)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error":"Untrusted Host for local owner API"
            })),
        )
            .into_response();
    }
    next.run(request).await
}
#[cfg(test)]
mod host_security_tests {
    use super::*;
    use crate::api::router;

    #[test]
    fn browser_authority_is_exact_loopback_host_and_port() {
        for (host, expected) in [
            ("127.0.0.1:4317", true),
            ("localhost:4317", true),
            ("LOCALHOST:4317", true),
            ("localhost:4318", false),
            ("attacker.invalid:4317", false),
            ("localhost.attacker.invalid:4317", false),
            ("127.0.0.1:4317.attacker.invalid", false),
            ("127.0.0.2:4317", false),
            ("[::1]:4317", false),
        ] {
            let request = Request::builder()
                .uri("/api/sessions")
                .header(header::HOST, host)
                .body(axum::body::Body::empty())
                .unwrap();
            assert_eq!(local_host_allowed(&request, 4317), expected, "{host}");
        }
    }

    #[tokio::test]
    async fn dns_rebinding_host_cannot_read_private_owner_api() {
        let root =
            std::env::temp_dir().join(format!("dominion-owner-host-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = crate::config::Config {
            port,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        state
            .store
            .put(
                "session",
                &json!({"id":"private-session",
            "title":"PRIVATE OWNER CONTENT"}),
            )
            .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router(state)).await });
        let url = format!("http://127.0.0.1:{port}/api/sessions");
        let client = reqwest::Client::new();
        let valid = client.get(&url).send().await.unwrap();
        assert_eq!(valid.status(), StatusCode::OK);
        let wrong = client
            .get(&url)
            .header(header::HOST, format!("evil.example:{port}"))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
        let wrong_body = wrong.text().await.unwrap();
        assert!(!wrong_body.contains("PRIVATE OWNER CONTENT"));
        server.abort();
        let _ = server.await;
        crate::test_support::cleanup_dir(root);
    }
}

/// Rejects cross-origin writes so a random website cannot drive the local
/// computer-control API. An optional admin token protects writes further.
pub(super) async fn guard_writes(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
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
