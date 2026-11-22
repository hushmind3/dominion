//! Route composition by product area. HTTP policy is applied once at the root.
mod collections;
mod conversations;
mod execution;
mod integrations;
mod models;
mod providers;
mod research;
mod workspace;

use crate::state::AppState;
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .merge(conversations::router())
        .merge(execution::router())
        .merge(integrations::router())
        .merge(models::router())
        .merge(providers::router())
        .merge(research::router())
        .merge(workspace::router())
        .merge(collections::router())
}
