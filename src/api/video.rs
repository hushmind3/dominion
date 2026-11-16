//! Historical video and upload records only. Media generation and publishing
//! are external MCP/Connections capabilities; no built-in renderer is loaded.
use crate::state::AppState;
use axum::{body::Body,extract::{Path,State},http::{Request,StatusCode},
 response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::path::PathBuf;

fn error(status:StatusCode,message:&str)->Response{
 (status,Json(json!({"error":message}))).into_response()
}
pub fn public_job(job:&Value)->Value{
 let id=job["id"].as_str().unwrap_or("");
 let ready=job["kind"]=="video" && job["status"]=="completed";
 json!({
  "id":job["id"],"kind":job["kind"],"title":job["input"]["title"],
  "brief":job["input"]["brief"],"status":job["status"],
  "engine":job["input"]["engine"],"progress":job["progress"],
  "phase":job.get("phase").unwrap_or(&job["status"]),
  "createdAt":job.get("createdAt").or(job.get("created_at")),
  "error":job["error"],"result":job["result"],
  "outputUrl":ready.then(||format!("/api/video/jobs/{id}/content")),
  "storyboard":job["videoState"]["scenes"],"remoteCancel":job["remoteCancel"],
  "archived":true,"generationAvailable":false
 })
}
fn legacy_job(state:&AppState,id:&str)->Option<Value>{
 state.store.get("job",id).filter(|j|
 matches!(j["kind"].as_str(),Some("video"|"youtube_upload")))
}
pub fn output_path(state:&AppState,id:&str)->Result<PathBuf,String>{
 uuid::Uuid::parse_str(id).map_err(|_|"잘못된 영상 작업 ID")?;
 let job=legacy_job(state,id).ok_or("영상 작업을 찾을 수 없습니다.")?;
 if job["kind"]!="video" || job["status"]!="completed"{
  return Err("완료된 영상 파일만 열 수 있습니다.".into());
 }
 let root=state.config.data_dir.join("video").join(id).canonicalize()
  .map_err(|_|"영상 폴더를 찾지 못했습니다.")?;
 let file=root.join("output.mp4").canonicalize()
  .map_err(|_|"영상 파일을 찾지 못했습니다.")?;
 if !file.starts_with(&root) || !file.is_file(){
  return Err("영상 파일이 지정된 작업 폴더 밖에 있습니다.".into());
 }
 Ok(file)
}
pub async fn status(State(state):State<AppState>)->Json<Value>{
 Json(json!({"deprecated":true,"generationAvailable":false,
  "historyCount":state.store.all("job").iter().filter(|j|
   matches!(j["kind"].as_str(),Some("video"|"youtube_upload"))).count(),
  "replacement":"MCP/Connections"}))
}
pub async fn get_config(State(state):State<AppState>)->Json<Value>{
 Json(state.store.get("video_config","default").unwrap_or(Value::Null))
}
pub async fn list_jobs(State(state):State<AppState>)->Json<Value>{
 Json(json!(state.store.all("job").into_iter().filter(|j|
   matches!(j["kind"].as_str(),Some("video"|"youtube_upload")))
  .map(|j|public_job(&j)).collect::<Vec<_>>()))
}
pub async fn job(State(state):State<AppState>,Path(id):Path<String>)->Response{
 match legacy_job(&state,&id){
  Some(job)=>Json(public_job(&job)).into_response(),
  None=>error(StatusCode::NOT_FOUND,"해당 작업 기록이 없습니다.")
 }
}
pub async fn outputs_handler(State(state):State<AppState>)->Json<Value>{
 Json(json!(state.store.all("job").into_iter().filter(|j|
    j["kind"]=="video" && j["status"]=="completed")
  .map(|j|public_job(&j)).collect::<Vec<_>>()))
}
pub async fn content(State(state):State<AppState>,Path(id):Path<String>,request:Request<Body>)->Response{
 let file=match output_path(&state,&id){
  Ok(path)=>path,Err(reason)=>return error(StatusCode::NOT_FOUND,&reason)
 };
 match tower_http::services::ServeFile::new(file).try_call(request).await{
  Ok(response)=>{
   let mut response=response.into_response();
   response.headers_mut().insert("x-content-type-options","nosniff".parse().unwrap());
   response
  },
  Err(_)=>error(StatusCode::NOT_FOUND,"기록된 MP4가 없습니다.")
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn archived_content_links_require_completed_video(){
  let complete=json!({"id":"old","kind":"video","status":"completed","input":{"title":"Archive"}});
  let pending=json!({"id":"old","kind":"video","status":"queued","input":{}});
  assert_eq!(public_job(&complete)["outputUrl"],"/api/video/jobs/old/content");
  assert!(public_job(&pending)["outputUrl"].is_null());
  assert_eq!(public_job(&complete)["generationAvailable"],false);
 }
}
