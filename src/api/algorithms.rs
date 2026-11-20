//! Owner-triggered configuration of existing execution algorithms.
//! No synthetic generation, evaluator campaign or automatic core promotion.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};

pub async fn evaluation(State(state):State<AppState>)->Json<Value>{
    Json(crate::features::cognition::strategy_eval::project(
        &state.store.all("job")))
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
 Json(crate::engine::algorithms::list(&state))
}
pub async fn register(State(state):State<AppState>,Json(body):Json<Value>)->Response{
 match crate::engine::algorithms::register(&state,&body){
  Ok(value)=>Json(value).into_response(),
  Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error}))).into_response()
 }
}
pub async fn activate(State(state):State<AppState>,Path(id):Path<String>,
 body:Option<Json<Value>>)->Response{
 let force=body.and_then(|Json(v)|v["force"].as_bool()).unwrap_or(false);
 match crate::engine::algorithms::activate_checked(&state,&id,force){
  Ok(value)=>Json(value).into_response(),
  Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error}))).into_response()
 }
}
pub async fn disable(State(state):State<AppState>,Path(id):Path<String>)->Response{
 match crate::engine::algorithms::disable(&state,&id){
  Some(value)=>Json(value).into_response(),
  None=>(StatusCode::NOT_FOUND,Json(json!({"error":"algorithm not found"}))).into_response()
 }
}
