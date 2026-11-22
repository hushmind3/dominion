//! HTTP routes for knowledge. Handler behavior is unchanged.
use crate::api::{algorithms, experience, improvements, memory};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/algorithms",
            get(algorithms::list).post(algorithms::register),
        )
        .route("/api/algorithms/:id/activate", post(algorithms::activate))
        .route("/api/algorithms/evaluation", get(algorithms::evaluation))
        .route("/api/algorithms/:id/disable", post(algorithms::disable))
        .route("/api/improvements", get(improvements::list))
        .route("/api/improvements/:id/:action", post(improvements::decide))
        .route("/api/memory/overview", get(memory::memory_overview))
        .route("/api/experience", get(experience::list))
        .route("/api/transfer", get(experience::transfer))
}
