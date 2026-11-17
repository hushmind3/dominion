//! Optional Money capability HTTP bindings. Core router registers one adapter.
use super::{money, money_find, money_implement, money_module};
use crate::state::AppState;
use axum::{routing::{get,post},Router,extract::{Request,State},
    http::Method,middleware::{self,Next},response::{Response,IntoResponse}};
use serde_json::json;

async fn require_enabled(State(state):State<AppState>,request:Request,next:Next)->Response{
    if request.method()!=Method::GET && !crate::engine::money_tool::enabled(&state){
        return (axum::http::StatusCode::CONFLICT,axum::Json(json!({
            "error":"머니 도구가 꺼져 있습니다. 설정에서 활성화하세요."
        }))).into_response();
    }
    next.run(request).await
}

pub fn router(state:AppState) -> Router<AppState> {
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
        .route_layer(middleware::from_fn_with_state(state,require_enabled))
        .route("/api/settings/money",get(money_module::status).post(money_module::set))
}
