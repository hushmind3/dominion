//! Read-only media archive. Native video/audio production is not part of the agent runtime.
use crate::state::AppState;
use axum::{body::Body,extract::{Path,State},http::{Request,StatusCode},
    response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
fn error(code:StatusCode,reason:&str)->Response {
    (code,Json(json!({"error":reason}))).into_response()
}
pub fn public_job(job:&Value)->Value {
    let id=job["id"].as_str().unwrap_or("");
    let available=job["kind"]=="video" && job["status"]=="completed";
    json!({"id":id,"kind":job["kind"],"title":job["input"]["title"],
        "brief":job["input"]["brief"],"status":job["status"],
        "phase":job.get("phase").unwrap_or(&job["status"]),
        "progress":job["progress"],"error":job["error"],"createdAt":job.get("createdAt"),
        "videoJobId":job["input"]["videoJobId"],"outputUrl":available.then(||
            format!("/api/video/jobs/{id}/content")),
        "storyboard":job["videoState"]["scenes"],"quality":job["result"]["quality"],
        "result":job["result"],"deliveryJobId":job["deliveryJobId"]})
}
pub fn outputs(state:&AppState)->Vec<Value>{
    state.store.all("job").iter().filter(|j|j["kind"]=="video" && j["status"]=="completed")
        .map(public_job).collect()
}
pub async fn status(State(state):State<AppState>)->Json<Value>{
    Json(json!({"retired":true,"available":false,"engines":[],
        "jobs":state.store.all("job").iter().filter(|j|j["kind"]=="video")
            .map(public_job).collect::<Vec<_>>() }))
}
pub async fn get_config(State(_state):State<AppState>)->Json<Value>{
    Json(json!({"retired":true,"available":false}))
}
pub async fn outputs_handler(State(state):State<AppState>)->Json<Value>{Json(json!(outputs(&state)))}
pub async fn files(State(state):State<AppState>)->Json<Value>{Json(json!(outputs(&state)))}
pub async fn list_jobs(State(state):State<AppState>)->Json<Value>{
    Json(json!(state.store.all("job").iter().filter(|job|
        matches!(job["kind"].as_str(),Some("video"|"youtube_upload")))
        .map(public_job).collect::<Vec<_>>()))
}
pub async fn job(State(state):State<AppState>,Path(id):Path<String>)->Response{
    state.store.get("job",&id).filter(|job|
        matches!(job["kind"].as_str(),Some("video"|"youtube_upload")))
        .map(|job|Json(public_job(&job)).into_response())
        .unwrap_or_else(||error(StatusCode::NOT_FOUND,"영상 작업 기록이 없습니다."))
}
pub async fn content(State(state):State<AppState>,Path(id):Path<String>,request:Request<Body>)->Response{
    // UUID validation and canonical containment prevent archive path traversal.
    if uuid::Uuid::parse_str(&id).is_err() ||
        !state.store.get("job",&id).is_some_and(|job|
            job["kind"]=="video" && job["status"]=="completed") {
        return error(StatusCode::NOT_FOUND,"완료된 영상 기록이 없습니다.");
    }
    let Ok(root)=state.config.data_dir.join("video").join(&id).canonicalize() else {
        return error(StatusCode::NOT_FOUND,"보관된 파일이 없습니다.");
    };
    let Ok(output)=root.join("output.mp4").canonicalize() else {
        return error(StatusCode::NOT_FOUND,"보관된 파일이 없습니다.");
    };
    if !output.starts_with(&root) {return error(StatusCode::NOT_FOUND,"파일 경로가 올바르지 않습니다.");}
    use tower::Service;
    match tower_http::services::ServeFile::new(output).try_call(request).await {
        Ok(response)=>{
            let mut response=response.into_response();
            response.headers_mut().insert("x-content-type-options","nosniff".parse().unwrap());
            response
        },
        Err(_)=>error(StatusCode::NOT_FOUND,"보관된 파일을 읽을 수 없습니다."),
    }
}
