use super::super::{compat, models};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/model-library/catalog", get(models::catalog))
        .route("/api/model-library/details", get(models::details))
        .route("/api/model-library/installed", get(models::installed))
        .route(
            "/api/model-library/installed/:id",
            axum::routing::delete(models::remove_installed),
        )
        .route("/api/model-library/pick-file", post(compat::pick_model))
        .route("/api/model-library/jobs", get(models::jobs))
        .route("/api/model-library/runtime", get(models::runtime))
        .route("/api/model-library/runtime/load", post(models::load))
        .route("/api/model-library/runtime/unload", post(models::unload))
        .route(
            "/api/model-library/runtime/install",
            post(models::install_runtime),
        )
        .route("/api/model-library/runtimes", get(models::runtimes))
        .route("/api/model-library/download", post(models::download))
        .route("/api/model-library/import", post(models::import))
        .route(
            "/api/model-library/jobs/:id/:action",
            post(models::job_action),
        )
}
