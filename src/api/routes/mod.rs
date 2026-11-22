//! Product-area composition; middleware and static serving belong to api::router.
use crate::state::AppState;
use axum::Router;
mod connections;
mod conversations;
mod integrations;
mod knowledge;
mod system;
mod work;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .merge(system::router())
        .merge(connections::router())
        .merge(conversations::router())
        .merge(work::router())
        .merge(knowledge::router())
        .merge(integrations::router())
}
