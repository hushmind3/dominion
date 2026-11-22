use super::super::{cli_oauth, gateways, providers};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/providers",
            get(providers::list_providers).post(providers::save_provider),
        )
        .route(
            "/api/providers/:id",
            axum::routing::delete(providers::delete_provider),
        )
        .route("/api/providers/catalog", get(providers::provider_catalog))
        .route(
            "/api/providers/catalog/:id/models",
            get(providers::public_catalog_models),
        )
        .route(
            "/api/providers/openrouter/free-models",
            get(providers::public_openrouter_free_catalog),
        )
        .route("/api/providers/free-catalog", get(providers::free_catalog))
        .route(
            "/api/providers/free-catalog/refresh",
            post(providers::free_catalog_refresh),
        )
        .route("/api/providers/cli", get(providers::cli_credentials))
        .route(
            "/api/providers/cli/:source/model",
            post(providers::set_cli_model),
        )
        .route("/api/providers/cli/:source/login", post(cli_oauth::start))
        .route(
            "/api/providers/cli/:source/login/status",
            get(cli_oauth::status),
        )
        .route(
            "/api/providers/cli/:source/login/input",
            post(cli_oauth::input),
        )
        .route(
            "/api/providers/cli/:source/login/cancel",
            post(cli_oauth::cancel),
        )
        .route(
            "/api/providers/codex/login",
            get(providers::codex_login_start),
        )
        .route(
            "/api/providers/codex/device",
            post(providers::codex_device_start),
        )
        .route(
            "/api/providers/codex/login/status",
            get(providers::codex_login_status),
        )
        .route(
            "/api/providers/cli/import",
            post(providers::import_cli_credential),
        )
        .route("/api/providers/:id/models", get(providers::provider_models))
        .route("/api/providers/:id/test", post(providers::test_provider))
        .route("/api/gateways/freellmapi", get(gateways::freellmapi))
        .route("/api/providers/:id/gateway", get(gateways::inspect))
}
