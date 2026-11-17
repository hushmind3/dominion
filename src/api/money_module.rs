//! Owner switch for the detachable Money domain on the existing SQLite store.
use crate::{engine::money_tool,state::AppState};
use axum::{extract::State,http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn status(State(state):State<AppState>)->Json<Value>{
    Json(state.store.get("money_module","global")
        .unwrap_or(json!({"id":"global","enabled":false})))
}
pub async fn set(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let Some(enabled)=body["enabled"].as_bool()else{
        return error(StatusCode::BAD_REQUEST,"enabled는 true/false여야 합니다.");
    };
    let (setting,paused,jobs)=match state.store.money_set_module_enabled(enabled){
        Ok(result)=>result,
        Err(message)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    };
    for job in &jobs{
        if let Some(session)=job["sessionId"].as_str(){state.cancel_session(session);}
        state.bus.record("job",job);
    }
    for auto in &paused{state.bus.record("money_autopilot",auto);}
    state.bus.record("money_module",&setting);
    state.store.audit("money.module_toggle",json!({
        "enabled":enabled,"pausedAutopilots":paused.len(),"cancelledMoneyJobs":jobs.len()
    }));
    if enabled{money_tool::schedule(&state);}
    Json(json!({"id":"global","enabled":enabled,
        "updatedAt":setting["updatedAt"],"pausedAutopilots":paused.len(),
        "cancelledMoneyJobs":jobs.len()})).into_response()
}
