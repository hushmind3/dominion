//! HTTP routes for integrations. Handler behavior is unchanged.
use crate::api::{browser, channels, compat, extensions, gateways, plugins};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tools", get(extensions::list_tools))
        .route("/api/browser/status", get(browser::status))
        .route("/api/browser/screenshot", get(browser::screenshot))
        .route("/api/browser/action", post(browser::action))
        .route("/api/gateways/freellmapi", get(gateways::freellmapi))
        .route("/api/skills/:id/export", get(extensions::export_skill))
        .route("/api/mcp/presets", get(extensions::mcp_presets))
        .route("/api/mcp/catalog", get(extensions::mcp_catalog))
        .route("/api/plugins/search", get(extensions::plugin_search))
        .route("/api/plugins/managed", get(plugins::list).post(plugins::install))
        .route("/api/plugins/managed/:id/start", post(plugins::start))
        .route("/api/plugins/managed/:id/stop", post(plugins::stop))
        .route("/api/plugins/managed/:id/logs", get(plugins::logs))
        .route("/api/plugins/managed/:id/update", post(plugins::versions::update))
        .route("/api/plugins/managed/:id/rollback", post(plugins::versions::rollback))
        .route("/api/plugins/managed/:id/bridge/pull", post(plugins::bridge::pull))
        .route("/api/plugins/managed/:id/auto-chat", get(plugins::bridge::auto_chat::status).post(plugins::bridge::auto_chat::change))
        .route("/api/plugins/managed/:id/bridge/:request", get(plugins::bridge::status))
        .route("/api/plugins/managed/:id/bridge/:request/approve", post(plugins::bridge::approve))
        .route("/api/plugins/managed/:id/bridge/:request/deliver", post(plugins::bridge::deliver))
        .route("/api/plugins/managed/:id", axum::routing::delete(plugins::remove))
        .route("/api/mcp/discover", get(extensions::mcp_discover))
        .route(
            "/api/skills/import-repo",
            post(extensions::import_skill_repo),
        )
        .route("/api/channels/catalog", get(channels::catalog_handler))
        .route("/api/channels", get(channels::list).post(channels::save))
        .route(
            "/api/channels/:id",
            get(channels::one)
                .post(channels::update)
                .delete(channels::remove),
        )
        .route("/api/channels/:id/diagnose", post(channels::diagnose))
        .route(
            "/api/channels/:id/delivery/reviews",
            get(channels::delivery_reviews),
        )
        .route(
            "/api/channels/:id/delivery/:delivery_id/reconcile",
            post(channels::reconcile_delivery),
        )
        .route("/api/webhooks/:id", post(compat::webhook))
}
