//! Read-only replay and paired scoring routes for the existing Rust job API.
use crate::{engine::native_replay,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn replay(State(state):State<AppState>,Path(id):Path<String>)->Response{
 if state.store.get("job",&id).is_none(){
  return error(StatusCode::NOT_FOUND,"Native run not found")
 }
 match native_replay::report(&state,&id){
  Ok(report)=>Json(report).into_response(),
  Err(message)=>error(StatusCode::CONFLICT,&message)
 }
}
pub async fn compare(State(state):State<AppState>,Path(id):Path<String>,
 Json(body):Json<Value>)->Response{
 let Some(candidate)=body["candidateRunId"].as_str().filter(|v|!v.is_empty()
  &&v.len()<=128 &&*v!=id) else{
  return error(StatusCode::BAD_REQUEST,"Provide a different candidateRunId")
 };
 if state.store.get("job",&id).is_none()||state.store.get("job",candidate).is_none(){
  return error(StatusCode::NOT_FOUND,"Native run not found")
 }
 match native_replay::compare(&state,&id,candidate){
  Ok(report)=>Json(report).into_response(),
  Err(message)=>error(StatusCode::CONFLICT,&message)
 }
}
/// Evidence-only multi-run forensic comparison. No creation, restart,
/// execution, acceptance override or automatic strategy promotion.
pub async fn adjudicate(State(state):State<AppState>,Path(id):Path<String>,
 Json(body):Json<Value>)->Response{
 let Some(candidates)=body["candidateRunIds"].as_array()
  .filter(|values|!values.is_empty() && values.len()<=8) else{
  return error(StatusCode::BAD_REQUEST,"Provide 1..8 candidateRunIds")
 };
 let Some(ids)=candidates.iter().map(|v|v.as_str().filter(|s|
  !s.is_empty()&&s.len()<=128).map(str::to_owned)).collect::<Option<Vec<_>>>()
 else {
  return error(StatusCode::BAD_REQUEST,"Invalid candidateRunIds");
 };
 match crate::engine::native_adjudication::adjudicate(&state,&id,&ids){
  Ok(report)=>Json(report).into_response(),
  Err(message)=>error(StatusCode::CONFLICT,&message)
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 use crate::{config::Config,db::Store,vault::Vault};
 fn state()->AppState{
  let dir=std::env::temp_dir().join(format!("dominion-replay-api-{}",uuid::Uuid::new_v4()));
  let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir};
  AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()))
 }
 #[tokio::test]
 async fn api_replay_is_read_only_and_rejects_missing_or_mismatched_runs(){
  let s=state();
  assert_eq!(replay(State(s.clone()),Path("missing".into())).await.status(),
   StatusCode::NOT_FOUND);
  s.store.put("job",&json!({"id":"a","kind":"prompt","status":"completed",
   "input":{"prompt":"x"}})).unwrap();
  assert_eq!(replay(State(s.clone()),Path("a".into())).await.status(),
   StatusCode::CONFLICT);
  assert_eq!(compare(State(s.clone()),Path("a".into()),
    Json(json!({"candidateRunId":"a"}))).await.status(),StatusCode::BAD_REQUEST);
  assert_eq!(s.store.count("execution_operation"),0);
 }
}
