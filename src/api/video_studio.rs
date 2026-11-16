//! Project APIs for the native autonomous content pipeline.
use crate::{engine::video_studio,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
fn error(message:String)->Response{
 let code=if message.contains("없습니다.")||message.contains("없습니다"){StatusCode::NOT_FOUND}
  else if message.contains("충돌")||message.contains("다른 창")||
   message.contains("버전")||message.contains("완료 후"){StatusCode::CONFLICT}
  else{StatusCode::BAD_REQUEST};
 (code,Json(json!({"error":message}))).into_response()
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
 Json(json!(video_studio::list(&state)))
}
pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
 match video_studio::create(&state,&body){
  Ok(project)=>(StatusCode::ACCEPTED,Json(project)).into_response(),
  Err(message)=>error(message),
 }
}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
 match video_studio::get(&state,&id){
  Ok(project)=>Json(project).into_response(),Err(message)=>error(message),
 }
}
pub async fn scenes(State(state):State<AppState>,Path(id):Path<String>,
 Json(body):Json<Value>)->Response{
 match video_studio::edit(&state,&id,&body){
  Ok(project)=>Json(project).into_response(),Err(message)=>error(message),
 }
}
pub async fn retry_plan(State(state):State<AppState>,Path(id):Path<String>,
 Json(body):Json<Value>)->Response{
 let Some(revision)=body["revision"].as_u64() else{
  return error("기획 버전이 필요합니다.".into())
 };
 match video_studio::retry_plan(&state,&id,revision,
  body["acknowledgeUnknown"]==true){
  Ok(project)=>(StatusCode::ACCEPTED,Json(project)).into_response(),
  Err(message)=>error(message),
 }
}
pub async fn render(State(state):State<AppState>,Path(id):Path<String>,
 Json(body):Json<Value>)->Response{
 match video_studio::render_with_ack(&state,&id,body["revision"].as_u64(),
  body["acknowledgeUnknown"]==true){
  Ok(project)=>(StatusCode::ACCEPTED,Json(project)).into_response(),
  Err(message)=>error(message),
 }
}
