//! Optional product surfaces: MCP discovery, channel delivery, external
//! account connections, browser control and skill catalogs.
//!
//! Keep endpoints stable for the existing UI and SDK. The core router owns
//! authentication, CORS, maintenance, fallback and static serving, so no
//! integration can install a second independent server or permission policy.
use crate::state::AppState;
use axum::{routing::{get,post},Router};
use super::{browser,channels,compat,extensions,extension_packages,gateways,oauth};

pub(super) fn routes()->Router<AppState>{
    Router::<AppState>::new()
        .route("/api/tools", get(extensions::list_tools))
        .route("/api/extensions/registries", get(extension_packages::sources).post(extension_packages::save_sources))
        .route("/api/extensions/catalog", get(extension_packages::catalog))
        .route("/api/extensions", get(extension_packages::list).post(extension_packages::install))
        .route("/api/extensions/import", post(extension_packages::import_local))
        .route("/api/extensions/:id", axum::routing::delete(extension_packages::remove))
        .route("/api/extensions/:id/enable", post(extension_packages::enable))
        .route("/api/extensions/:id/status", get(extension_packages::diagnose))
        .route("/api/browser/status", get(browser::status))
        .route("/api/browser/screenshot", get(browser::screenshot))
        .route("/api/browser/action", post(browser::action))
        .route("/api/gateways/freellmapi", get(gateways::freellmapi))
        .route("/api/providers/:id/gateway", get(gateways::inspect))
        .route("/api/skills/:id/export", get(extensions::export_skill))
        .route("/api/mcp/presets", get(extensions::mcp_presets))
        .route("/api/mcp/catalog", get(extensions::mcp_catalog))
        .route("/api/mcp/discover", get(extensions::mcp_discover))
        .route("/api/skill-catalog", get(extensions::skill_catalog))
        .route("/api/skill-catalog/meta", get(extensions::skill_catalog_meta))
        .route("/api/skill-catalog/import", post(extensions::import_skill))
        .route("/api/skills/import-repo", post(extensions::import_skill_repo))
        .route("/api/oauth/providers", get(oauth::providers))
        .route("/api/oauth/connections", get(channels::connections))
        .route("/api/oauth/providers/:id", post(oauth::save))
        .route("/api/oauth/:id/start", get(oauth::start))
        .route("/api/oauth/:id/open", post(oauth::open))
        .route("/api/oauth/:id/browser-open", post(oauth::browser_open))
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
