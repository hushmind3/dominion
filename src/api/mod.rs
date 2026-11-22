mod response;
#[cfg(test)]
mod tests;
pub mod translation;
pub mod algorithms;
pub mod settings;
pub mod executions;
pub mod runs;
pub mod tasks;
pub mod task_teams;
pub mod experience;
pub mod memory;
pub mod browser;
pub mod extensions;
pub mod approvals;
pub mod sessions;
pub mod agents;
pub mod providers;
pub mod cli_oauth;
pub mod chat;
pub mod jobs;
pub mod sdk_runs;
pub mod native_replay;
pub mod sdk;
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
pub mod remote_workers;

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
        // core
        .route("/api/health", get(system::health))
        .route("/api/status", get(system::status))
        .route(
            "/api/settings/assistant",
            get(settings::assistant_settings).post(settings::save_assistant_settings),
        )
        .route(
            "/api/settings/owner-profile",
            get(settings::owner_profile).post(settings::save_owner_profile),
        )
        .route("/api/attachments", post(attachments::upload))
        .route("/api/attachments/:id/content", get(attachments::content))
        .route("/api/events", get(system::events))
        .route("/api/model-usage", get(system::model_usage))
        .route("/api/run-wakes", get(system::run_wakes))
        // Explicit owner-driven algorithm configuration only; no generation,
        // campaign scheduling, independent experimentation or auto promotion.
        .route("/api/algorithms",get(algorithms::list).post(algorithms::register))
        .route("/api/algorithms/:id/activate",post(algorithms::activate))
        .route("/api/algorithms/evaluation",get(algorithms::evaluation))
        .route("/api/algorithms/:id/disable",post(algorithms::disable))
        .route("/api/policies", get(system::policies))
        .route("/api/system/backup", post(system::create_backup))
        .route("/api/system/backup-keys", get(system::backup_keys))
        .route(
            "/api/system/backup-keys/rotate",
            post(system::rotate_backup_keys),
        )
        .route("/api/system/secret-key", get(system::secret_key))
        .route(
            "/api/system/secret-key/rotate",
            post(system::rotate_secret_key),
        )
        .route("/api/system/backups", get(system::list_backups))
        .route(
            "/api/system/maintenance",
            get(system::maintenance_status).post(system::maintenance),
        )
        .route("/api/system/restore", post(system::restore_backup))
        .route("/api/chat", post(chat::chat))
        .route("/api/jobs",get(jobs::list).post(jobs::create))
        .route("/api/jobs/:id",get(jobs::one))
        .route("/api/jobs/:id/cancel",post(jobs::cancel))
        .route("/api/jobs/:id/directive",post(jobs::directive))
        .route("/api/jobs/:id/steer",post(jobs::steer))
        // Keep the historic API paths, not a second active harness for NEW jobs.
        // Old FileJournal sessions retain their original recovery protocol.
        // New /api/harness/runs requests share the native SDK job kernel.
        .route("/api/harness/runs",get(sdk::list).post(sdk::start))
        .route("/api/harness/runs/:id",get(sdk::one))
        .route("/api/harness/runs/:id/approve",post(sdk::approve))
        .route("/api/harness/runs/:id/cancel",post(sdk::cancel))
        .route("/api/harness/runs/:id/resume",post(sdk::resume))
        .route("/api/harness/runs/:id/reconcile",post(sdk::reconcile))
        .route("/api/sdk/runs",get(sdk_runs::list).post(sdk_runs::create))
        .route("/api/sdk/runs/:id",get(sdk_runs::one))
        .route("/api/sdk/runs/:id/events",get(sdk_runs::events))
        .route("/api/sdk/runs/:id/receipts",get(sdk_runs::receipts))
        .route("/api/sdk/runs/:id/artifact-preview",post(sdk_runs::artifact_preview))
        .route("/api/sdk/runs/:id/replay",get(native_replay::replay))
        .route("/api/sdk/runs/:id/compare",post(native_replay::compare))
        .route("/api/sdk/runs/:id/adjudicate",post(native_replay::adjudicate))
        .route("/api/jobs/:id/adjudicate",post(native_replay::adjudicate))
        .route("/api/jobs/:id/replay",get(native_replay::replay))
        .route("/api/jobs/:id/compare",post(native_replay::compare))
        .route("/api/sdk/runs/:id/approve",post(sdk_runs::approve))
        .route("/api/sdk/runs/:id/deny",post(sdk_runs::deny))
        .route("/api/sdk/runs/:id/cancel",post(sdk_runs::cancel))
        .route("/api/sdk/runs/:id/resume",post(sdk_runs::resume))
        .route("/api/sdk/runs/:id/reconcile",post(sdk_runs::reconcile))
        .route("/api/sdk/runs/:id/steer",post(sdk_runs::steer))
        .route("/api/tasks", get(tasks::list))
        .route("/api/tasks/:id", get(tasks::one))
        .route("/api/runs/:id",get(runs::one))
        .route("/api/remote/workers",get(remote_workers::owner_workers))
        .route("/api/remote/tasks",get(remote_workers::owner_list).post(remote_workers::owner_create))
        .route("/api/remote/tasks/:id",get(remote_workers::owner_one))
        .route("/api/remote/tasks/:id/cancel",post(remote_workers::owner_cancel))
        .route("/api/task-teams",get(task_teams::list).post(task_teams::create))
        .route("/api/task-teams/:id",get(task_teams::one))
        .route("/api/task-teams/:id/cancel",post(task_teams::cancel))
        .route("/api/task-teams/bridges",get(task_teams::bridges))
        .route("/api/task-teams/:id/retry",post(task_teams::retry_team))
        .route("/api/task-teams/:id/worktrees/:task/preview",
            get(task_teams::worktree_preview))
        .route("/api/task-teams/:id/worktrees/:task/promote",
            post(task_teams::worktree_promote))
        .route("/api/tools", get(extensions::list_tools))
        .route("/api/browser/status", get(browser::status))
        .route("/api/browser/screenshot", get(browser::screenshot))
        .route("/api/browser/action", post(browser::action))
        .route("/api/policy", get(approvals::get_policy).post(approvals::save_policy))
        .route("/api/audit", get(system::audit_log))
        .route("/api/model-route", get(system::model_route))
        .route("/api/memory/overview", get(memory::memory_overview))
        .route("/api/translate", post(translation::translate))
        .route("/api/translate/config", get(translation::translate_config))
        .route("/api/upload", post(compat::upload))
        // providers
        .route(
            "/api/providers",
            get(providers::list_providers).post(providers::save_provider),
        )
        .route(
            "/api/providers/:id",
            axum::routing::delete(providers::delete_provider),
        )
        .route("/api/providers/catalog", get(providers::provider_catalog))
        .route("/api/providers/catalog/:id/models", get(providers::public_catalog_models))
        .route("/api/providers/openrouter/free-models", get(providers::public_openrouter_free_catalog))
        .route("/api/providers/free-catalog", get(providers::free_catalog))
        .route("/api/providers/free-catalog/refresh", post(providers::free_catalog_refresh))
        .route("/api/providers/cli", get(providers::cli_credentials))
        .route("/api/providers/cli/:source/model", post(providers::set_cli_model))
        .route("/api/providers/cli/:source/login", post(cli_oauth::start))
        .route("/api/providers/cli/:source/login/status", get(cli_oauth::status))
        .route("/api/providers/cli/:source/login/input", post(cli_oauth::input))
        .route("/api/providers/cli/:source/login/cancel", post(cli_oauth::cancel))
        .route("/api/providers/codex/login", get(providers::codex_login_start))
        .route("/api/providers/codex/device", post(providers::codex_device_start))
        .route("/api/providers/codex/login/status", get(providers::codex_login_status))
        .route("/api/providers/cli/import", post(providers::import_cli_credential))
        .route("/api/providers/:id/models", get(providers::provider_models))
        .route("/api/providers/:id/test", post(providers::test_provider))
        .route("/api/gateways/freellmapi", get(gateways::freellmapi))
        .route("/api/providers/:id/gateway", get(gateways::inspect))
        // agents
        .route("/api/agents", get(agents::list_agents).post(agents::save_agent))
        .route("/api/agents/bulk-model", post(agents::bulk_agent_model))
        .route("/api/personas/import/preview", get(personas::preview))
        .route("/api/personas/import", post(personas::import))
        .route("/api/agents/:id/growth", get(agents::growth))
        .route("/api/synergy", get(agents::synergy))
        .route("/api/agents/:id/avatar", get(personas::avatar))
        .route("/api/agents/:id", axum::routing::delete(agents::delete_agent))
        .route("/api/rooms", get(rooms::list).post(rooms::save))
        .route(
            "/api/rooms/:id",
            get(rooms::one).post(rooms::update).delete(rooms::remove),
        )
        .route(
            "/api/rooms/:id/messages",
            get(rooms::messages).post(rooms::send_message),
        )
        .route("/api/rooms/:id/events", get(rooms::events))
        .route("/api/rooms/:id/state", get(rooms::state))
        .route("/api/rooms/:id/knowledge", get(rooms::knowledge))
        .route("/api/rooms/:id/system-prompt", get(rooms::system_prompt))
        .route("/api/rooms/:id/knowledge/learn", post(rooms::learn))
        .route("/api/rooms/:id/run", post(rooms::run))
        .route("/api/rooms/:id/reset", post(rooms::reset))
        .route("/api/rooms/:id/stop", post(rooms::stop))
        // sessions
        .route(
            "/api/sessions",
            get(sessions::list_sessions).post(sessions::create_session),
        )
        .route(
            "/api/sessions/:id",
            get(sessions::get_session)
                .delete(sessions::delete_session)
                .post(sessions::rename_session),
        )
        .route("/api/sessions/:id/messages", get(sessions::session_messages))
        .route("/api/sessions/:id/usage", get(sessions::session_usage))
        .route("/api/sessions/:id/rename", post(sessions::rename_session))
        .route("/api/sessions/:id/clear", post(sessions::clear_session))
        .route("/api/sessions/:id/fork", post(sessions::fork_session))
        .route("/api/sessions/:id/compact", post(sessions::compact_session))
        .route("/api/sessions/:id/cancel", post(sessions::cancel_session))
        .route("/api/sessions/:id/archive", post(sessions::archive_session))
        .route("/api/sessions/:id/learn", post(sessions::learn))
        .route(
            "/api/executions/:id/operations",
            get(executions::execution_operations),
        )
        .route("/api/skills/:id/export", get(extensions::export_skill))
        // approvals
        .route(
            "/api/approvals",
            get(approvals::list_approvals).post(approvals::resolve_approval),
        )
        // capabilities + commands
        .route("/api/capabilities", get(system::capabilities))
        .route("/api/commands", get(system::commands))
        // compatibility surface
        .route("/api/mcp/presets", get(extensions::mcp_presets))
        .route("/api/mcp/catalog", get(extensions::mcp_catalog))
        .route("/api/mcp/discover", get(extensions::mcp_discover))
        .route("/api/skill-catalog", get(extensions::skill_catalog))
        .route("/api/skill-catalog/meta", get(extensions::skill_catalog_meta))
        .route("/api/skill-catalog/import", post(extensions::import_skill))
        .route("/api/skills/import-repo", post(extensions::import_skill_repo))
        .route("/api/model-library/catalog", get(models::catalog))
        .route("/api/model-library/details", get(models::details))
        .route("/api/model-library/installed", get(models::installed))
        .route("/api/model-library/installed/:id", axum::routing::delete(models::remove_installed))
        .route("/api/model-library/pick-file", post(compat::pick_model))
        .route("/api/model-library/jobs", get(models::jobs))
        .route("/api/model-library/runtime", get(models::runtime))
        .route("/api/model-library/runtime/load", post(models::load))
        .route("/api/model-library/runtime/unload", post(models::unload))
        .route("/api/model-library/runtime/install", post(models::install_runtime))
        .route("/api/model-library/runtimes", get(models::runtimes))
        .route("/api/model-library/download", post(models::download))
        .route("/api/model-library/import", post(models::import))
        .route(
            "/api/model-library/jobs/:id/:action",
            post(models::job_action),
        )
        .route("/api/dev/jobs/:id", get(dev::get_job))
        .route("/api/dev/update", get(dev::update_status).post(dev::update))
        .route("/api/oauth/providers", get(oauth::providers))
        .route("/api/oauth/connections", get(channels::connections))
        .route("/api/oauth/providers/:id", post(oauth::save))
        .route("/api/oauth/:id/start", get(oauth::start))
        .route("/api/oauth/:id/open", post(oauth::open))
        .route("/api/oauth/:id/callback", get(oauth::callback))
        .route("/api/oauth/:id", axum::routing::delete(oauth::disconnect))
        .route("/api/system/pick-folder", post(compat::pick_folder))
        .route("/api/channels/catalog", get(channels::catalog_handler))
        .route("/api/channels", get(channels::list).post(channels::save))
        .route(
            "/api/channels/:id",
            get(channels::one)
                .post(channels::update)
                .delete(channels::remove),
        )
        .route("/api/channels/:id/diagnose", post(channels::diagnose))
        .route("/api/channels/:id/delivery/reviews",get(channels::delivery_reviews))
        .route("/api/channels/:id/delivery/:delivery_id/reconcile",post(channels::reconcile_delivery))
        .route("/api/executions", get(executions::list))
        .route("/api/executions/active", get(executions::active_executions))
        .route("/api/experience", get(experience::list))
        .route("/api/transfer", get(experience::transfer))
        .route("/api/executions/:id", get(executions::one))
        .route("/api/executions/:id/cancel", post(executions::cancel))
        .route("/api/webhooks/:id", post(compat::webhook));

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

/// Validate a supplied browser Host on every local-owner API request,
/// including GET, SSE and credential metadata reads. The standalone product
/// listens on 127.0.0.1 only; its remote worker gateway is a separate router.
///
/// Internal in-process requests may omit Host. A real browser cannot set or
/// remove its Host header, and HTTP/2 supplies the authority field.
fn local_host_allowed(request:&Request,port:u16)->bool {
    let actual=request.headers().get(header::HOST)
        .and_then(|v|v.to_str().ok())
        .or_else(||request.uri().authority().map(|v|v.as_str()));
    match actual {
        None=>true, // internal tests/in-process invocations, not browser traffic
        Some(host)=>host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}"))
            ||host.eq_ignore_ascii_case(&format!("localhost:{port}"))
    }
}
async fn guard_local_host(State(state):State<AppState>,request:Request,next:Next)->Response {
    if request.uri().path().starts_with("/api/")
        && !local_host_allowed(&request,state.config.port) {
        return (StatusCode::FORBIDDEN,Json(json!({
            "error":"Untrusted Host for local owner API"
        }))).into_response();
    }
    next.run(request).await
}
#[cfg(test)]
mod host_security_tests {
    use super::*;

    #[test]
    fn browser_authority_is_exact_loopback_host_and_port() {
        for (host,expected) in [
            ("127.0.0.1:4317",true),("localhost:4317",true),
            ("LOCALHOST:4317",true),("localhost:4318",false),
            ("attacker.invalid:4317",false),("localhost.attacker.invalid:4317",false),
            ("127.0.0.1:4317.attacker.invalid",false),("127.0.0.2:4317",false),
            ("[::1]:4317",false),
        ] {
            let request=Request::builder().uri("/api/sessions")
                .header(header::HOST,host)
                .body(axum::body::Body::empty()).unwrap();
            assert_eq!(local_host_allowed(&request,4317),expected,"{host}");
        }
    }

    #[tokio::test]
    async fn dns_rebinding_host_cannot_read_private_owner_api() {
        let root=std::env::temp_dir().join(format!(
            "dominion-owner-host-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let config=crate::config::Config{
            port,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone(),
        };
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"private-session",
            "title":"PRIVATE OWNER CONTENT"})).unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,router(state)).await
        });
        let url=format!("http://127.0.0.1:{port}/api/sessions");
        let client=reqwest::Client::new();
        let valid=client.get(&url).send().await.unwrap();
        assert_eq!(valid.status(),StatusCode::OK);
        let wrong=client.get(&url).header(header::HOST,format!("evil.example:{port}"))
            .send().await.unwrap();
        assert_eq!(wrong.status(),StatusCode::FORBIDDEN);
        let wrong_body=wrong.text().await.unwrap();
        assert!(!wrong_body.contains("PRIVATE OWNER CONTENT"));
        server.abort();
        let _=server.await;
        crate::test_support::cleanup_dir(root);
    }
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
