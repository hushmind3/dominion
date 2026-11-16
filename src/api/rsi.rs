//! Read-only archival RSI records. Campaigns, source mutation, self-scoring,
//! generation loops and automated application have no executable API.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};

fn missing()->Response{
 (StatusCode::NOT_FOUND,Json(json!({"error":"과거 자기개선 기록을 찾을 수 없습니다."}))).into_response()
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
 let campaigns=state.store.all("rsi_campaign");
 Json(json!({"version":"retired","retired":true,"readOnly":true,
   "candidates":state.store.all("rsi_candidate"),
   "campaigns":campaigns,
   "minPairedCases":0,
   "replacement":"user-directed code changes and verifier-backed procedure memory"}))
}
pub async fn campaigns(State(state):State<AppState>)->Json<Value>{
 Json(json!({"retired":true,"readOnly":true,
  "campaigns":state.store.all("rsi_campaign")}))
}
pub async fn campaign(State(state):State<AppState>,Path(id):Path<String>)->Response{
 match state.store.get("rsi_campaign",&id){
  Some(record)=>Json(record).into_response(),None=>missing()
 }
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
 match state.store.get("rsi_candidate",&id).or_else(||state.store.get("rsi_run",&id)){
  Some(record)=>Json(record).into_response(),None=>missing()
 }
}
pub async fn issue(State(state):State<AppState>,Path(id):Path<String>)->Response{
 match state.store.get("rsi_issue",&id){
  Some(record)=>Json(record).into_response(),None=>missing()
 }
}
pub async fn evaluation_profiles(State(state):State<AppState>)->Json<Value>{
 Json(json!({"retired":true,"profiles":state.store.all("rsi_evaluation_profile")}))
}
pub async fn signals(State(state):State<AppState>)->Json<Value>{
 Json(json!({"retired":true,"signals":state.store.all("rsi_signal")}))
}
pub async fn index(State(state):State<AppState>)->Json<Value>{
 Json(json!({"retired":true,"records":state.store.all("rsi_index")}))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn retired_api_still_exposes_history_shape(){
  let shape=json!({"version":"retired","readOnly":true,"campaigns":[]});
  assert_eq!(shape["readOnly"],true);
 }
}
