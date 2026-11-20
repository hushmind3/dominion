//! Optional product service APIs: no second server, policy, middleware or UI.
use crate::state::AppState;
use axum::{Router,routing::get};
use super::collections;
mod provider_routes;
mod conversation_routes;
mod automation_routes;
mod evaluation_routes;
mod management_routes;

pub(super) fn routes()->Router<AppState>{
    let mut router=Router::<AppState>::new()
        .merge(provider_routes::routes())
        .merge(conversation_routes::routes())
        .merge(automation_routes::routes())
        .merge(evaluation_routes::routes())
        .merge(management_routes::routes())
        ;

    for kind in collections::COLLECTIONS {
        router = router
            .route(
                &format!("/api/{kind}"),
                get(collections::list).post(collections::save),
            )
            .route(
                &format!("/api/{kind}/:id"),
                get(collections::one)
                    .delete(collections::remove)
                    .post(collections::save),
            );
    }
    router
}
