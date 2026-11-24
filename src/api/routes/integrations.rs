use super::super::{browser, channels, cloud_auth, compat, extensions, extension_registry, extension_process, oauth};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/extensions/releases", get(extension_process::releases))
        .route("/api/extensions/apps", get(extension_registry::list).post(extension_registry::register))
        .route("/api/extensions/apps/:id", get(extension_registry::one).delete(extension_registry::remove))
        .route("/api/extensions/apps/:id/state", post(extension_registry::set_state))
        .route("/api/extensions/apps/:id/grant", get(extension_registry::grant_status).post(extension_registry::issue_grant).delete(extension_registry::revoke_grant))
        .route("/api/extensions/apps/:id/history", get(extension_registry::history))
        .route("/api/extensions/apps/:id/package", get(extension_process::status).post(extension_process::install).delete(extension_process::uninstall))
        .route("/api/extensions/apps/:id/process/start", post(extension_process::start))
        .route("/api/extensions/apps/:id/process/stop", post(extension_process::stop))
        .route("/api/extensions/apps/:id/runs", post(extension_registry::scoped_start))
        .route("/api/extensions/apps/:id/runs/:run", get(extension_registry::scoped_one))
        .route("/api/tools", get(extensions::list_tools))
        .route(
            "/api/extensions/runtime",
            get(extensions::runtime_extensions).post(extensions::set_runtime_extensions),
        )
        .route("/api/browser/status", get(browser::status))
        .route("/api/browser/screenshot", get(browser::screenshot))
        .route("/api/browser/action", post(browser::action))
        .route("/api/upload", post(compat::upload))
        .route("/api/skills/:id/export", get(extensions::export_skill))
        .route("/api/mcp/presets", get(extensions::mcp_presets))
        .route("/api/mcp/catalog", get(extensions::mcp_catalog))
        .route("/api/mcp/discover", get(extensions::mcp_discover))
        .route("/api/skill-catalog", get(extensions::skill_catalog))
        .route(
            "/api/skill-catalog/meta",
            get(extensions::skill_catalog_meta),
        )
        .route("/api/skill-catalog/import", post(extensions::import_skill))
        .route(
            "/api/skills/import-repo",
            post(extensions::import_skill_repo),
        )
        .route("/api/cloud-auth/google/start", post(cloud_auth::start))
        .route("/api/cloud-auth/google/status", get(cloud_auth::status))
        .route("/api/cloud-auth/google/ack", post(cloud_auth::acknowledge))
        .route("/api/cloud-auth/google/callback", get(cloud_auth::callback))
        .route("/api/oauth/providers", get(oauth::providers))
        .route("/api/oauth/connections", get(channels::connections))
        .route("/api/oauth/providers/:id", post(oauth::save))
        .route("/api/oauth/:id/start", get(oauth::start))
        .route("/api/oauth/:id/open", post(oauth::open))
        .route("/api/oauth/:id/callback", get(oauth::callback))
        .route("/api/oauth/:id", axum::routing::delete(oauth::disconnect))
        .route("/api/channels/catalog", get(channels::catalog_handler))
        .route("/api/channels", get(channels::list).post(channels::save))
        .route(
            "/api/channels/:id",
            get(channels::one)
                .post(channels::update)
                .delete(channels::remove),
        )
        .route("/api/channels/:id/diagnose", post(channels::diagnose))
        .route("/api/webhooks/:id", post(compat::webhook))
}
