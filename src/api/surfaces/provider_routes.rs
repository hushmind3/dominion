//! Product-domain HTTP endpoints, outside the minimal turn API.
//! Shared root router applies auth, CORS, maintenance and fallback.
use super::super::*;
pub(super) fn routes()->Router<AppState>{
    Router::<AppState>::new()
        .route("/api/model-usage", get(system::model_usage))
        .route("/api/model-route", get(system::model_route))
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
}
