//! Normalized lifecycle lookup across chat, room, team and SDK, read-only.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,
    response::{IntoResponse,Response},Json};
use serde_json::json;
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match crate::host::execution::run_view::lookup(&state.store,&id){
        Some(run)=>Json(run).into_response(),
        None=>(StatusCode::NOT_FOUND,Json(json!({"error":"Run not found"})))
            .into_response(),
    }
}
