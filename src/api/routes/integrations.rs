//! HTTP routes for integrations. Handler behavior is unchanged.
use crate::api::{browser, channels, compat, extensions, gateways};
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
