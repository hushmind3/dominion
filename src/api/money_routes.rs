//! Optional Money capability HTTP bindings. Core router registers one adapter.
use super::{money, money_find, money_implement};
use crate::state::AppState;
use axum::{routing::{get, post}, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/money", get(money::overview))
        .route("/api/money/find", get(money_find::latest).post(money_find::find))
        .route("/api/money/research", get(money::research))
        .route("/api/money/stats", get(money::stats))
        .route("/api/money/rotation", get(money::rotation))
        .route("/api/money/campaigns", post(money::create_campaign))
        .route("/api/money/campaigns/:id", get(money::campaign).post(money::update_campaign))
        .route("/api/money/campaigns/:id/run", post(money::start))
        .route("/api/money/campaigns/:id/drafts/:tension_id/promote", post(money::promote_draft))
        .route("/api/money/campaigns/:id/autopilot", get(money::autopilot_status).post(money::autopilot_set))
        .route("/api/money/candidates/:id", get(money::candidate).post(money::update_candidate))
        .route("/api/money/candidates/:id/implement", post(money_implement::launch))
        .route("/api/money/candidates/:id/experiments", post(money::record_experiment))
        .route("/api/money/candidates/:id/validation-kit", post(money::create_validation_kit))
}
