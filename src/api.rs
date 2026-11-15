use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    routing::{get, post},
    Json, Router
};
use serde_json::{json, Value};
use std::{collections::HashMap, convert::Infallible, time::Duration};
use tower_http::services::ServeDir;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health",get(health))
        .route("/api/tasks",get(list).post(submit))
        .route("/api/tasks/:id",get(one))
        .route("/api/tasks/:id/cancel",post(cancel))
        .route("/api/tasks/:id/steer",post(steer))
        .route("/api/tasks/:id/events",get(events))
        .route("/api/groups",get(group_list).post(group_create))
        .route("/api/groups/:id",get(group_get))
        .route("/api/groups/:id/messages",post(group_say))
        .route("/api/groups/:id/next",post(group_next))
        .route("/api/videos",get(video_list).post(video_create))
        .route("/api/videos/:id",get(video_get))
        .route("/api/videos/:id/render",post(video_render))
        .route("/api/videos/:id/scenes/:index",post(video_scene))
        .route("/api/videos/:id/content",get(video_content))
        .fallback_service(ServeDir::new("web"))
        .with_state(state)
}

async fn health(State(state):State<AppState>) -> Json<Value> {
    Json(json!({"ok":true,"engine":"rust","model":state.model_name,
        "workspace":state.workspace.to_string_lossy(),"localOnly":true}))
}
async fn list(State(state):State<AppState>) -> Json<Value> {
    Json(json!(state.store.list()))
}
async fn one(State(state):State<AppState>,Path(id):Path<String>) -> Response {
    match state.store.get(&id) {
        Some(record) => Json(record).into_response(),
        None => (StatusCode::NOT_FOUND,Json(json!({"error":"작업이 없습니다."}))).into_response()
    }
}
async fn submit(State(state):State<AppState>,Json(raw):Json<Value>) -> Response {
    let Ok(input) = serde_json::from_value::<crate::runtime::TaskInput>(raw) else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"작업 입력 형식이 잘못됐습니다."}))).into_response();
    };
    // An HTTP caller owns the retry key. Otherwise a lost response could
    // enqueue duplicate model charges and external tool side effects.
    if input.request_id.is_none() {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"requestId가 필요합니다."}))).into_response();
    }
    let sdk = crate::runtime::DominionSdk::from_state(state);
    match sdk.submit(input).await {
        Ok(record) => (StatusCode::ACCEPTED,Json(json!(record))).into_response(),
        Err(error) => {
            let code = if error.0.contains("서로 다른 작업") {
                StatusCode::CONFLICT
            } else { StatusCode::BAD_REQUEST };
            (code,Json(json!({"error":error.to_string()}))).into_response()
        }
    }
}
async fn cancel(State(state):State<AppState>,Path(id):Path<String>) -> Response {
    match state.store.cancel(&id) {
        Some(record) => {
            state.emit(&id,"cancel_requested",json!({}));
            Json(record).into_response()
        }
        None => (StatusCode::NOT_FOUND,Json(json!({"error":"작업이 없습니다."}))).into_response()
    }
}
async fn steer(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Value>) -> Response {
    let Some(text) = input["text"].as_str().map(str::trim).filter(|s|!s.is_empty() && s.len()<=4096) else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"지시 내용은 1~4096자여야 합니다."}))).into_response()
    };
    match state.store.steer(&id,text) {
        Ok(record) => {
            state.emit(&id,"instruction_queued",json!({"text":text}));
            Json(record).into_response()
        }
        Err(message) => (StatusCode::CONFLICT,Json(json!({"error":message}))).into_response()
    }
}
fn full_history(state:&AppState,id:&str,after:u64)->Vec<Value> {
    let mut cursor=after;
    let mut out=Vec::new();
    // Read all currently retained rows in bounded pages, while live subscribers
    // accumulate newly published events. The cursor is durable SQLite sequence.
    for _ in 0..128 {
        let batch=state.store.events_after(id,cursor,500);
        let length=batch.len();
        if length==0 { break; }
        for row in batch {
            let seq=row["seq"].as_u64().unwrap_or(0);
            if seq>cursor { cursor=seq;out.push(row); }
        }
        if length<500 { break; }
    }
    out
}
async fn events(
    State(state):State<AppState>,
    Path(id):Path<String>,
    Query(params):Query<HashMap<String,String>>,
    headers:HeaderMap
) -> Response {
    if state.store.get(&id).is_none() {
        return (StatusCode::NOT_FOUND,Json(json!({"error":"작업이 없습니다."}))).into_response();
    }
    let after=params.get("after").and_then(|s|s.parse::<u64>().ok())
        .or_else(||headers.get("last-event-id").and_then(|s|s.to_str().ok())
            .and_then(|s|s.parse::<u64>().ok())).unwrap_or(0);
    let mut receiver=state.bus.subscribe();
    let backlog=full_history(&state,&id,after);
    let stream=async_stream::stream! {
        let mut cursor=after;
        for event in backlog {
            let seq=event["seq"].as_u64().unwrap_or(0);
            if seq<=cursor {continue;}
            cursor=seq;
            yield Ok::<Event,Infallible>(Event::default().id(seq.to_string()).data(event.to_string()));
        }
        loop {
            match receiver.recv().await {
                Ok(event) if event["taskId"].as_str()==Some(id.as_str()) => {
                    let seq=event["seq"].as_u64().unwrap_or(0);
                    if seq>cursor {
                        cursor=seq;
                        yield Ok(Event::default().id(seq.to_string()).data(event.to_string()));
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    for event in full_history(&state,&id,cursor) {
                        let seq=event["seq"].as_u64().unwrap_or(0);
                        if seq>cursor {
                            cursor=seq;
                            yield Ok(Event::default().id(seq.to_string()).data(event.to_string()));
                        }
                    }
                }
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response()
}

fn parse_room(id:&str)->Result<uuid::Uuid,Response>{
    uuid::Uuid::parse_str(id).map_err(|_|(StatusCode::BAD_REQUEST,
        Json(json!({"error":"올바른 회의실 ID가 필요합니다."}))).into_response())
}
async fn group_list(State(state):State<AppState>)->Json<Value>{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    Json(json!(sdk.group_list().await))
}
async fn group_create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let input=match serde_json::from_value::<crate::group::NewRoom>(body){
        Ok(value)=>value,
        Err(error)=>return (StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    };
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.group_create(input).await{
        Ok(room)=>(StatusCode::CREATED,Json(json!(room))).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn group_get(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.group_get(id).await{
        Ok(room)=>Json(json!(room)).into_response(),
        Err(error)=>(StatusCode::NOT_FOUND,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn group_say(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let input=match serde_json::from_value::<crate::group::Say>(body){
        Ok(value)=>value,
        Err(error)=>return (StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    };
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.group_say(id,input.content).await{
        Ok(room)=>Json(json!(room)).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn group_next(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.group_next(id).await{
        Ok(room)=>Json(json!(room)).into_response(),
        Err(error)=>(StatusCode::BAD_GATEWAY,Json(json!({"error":error.to_string()}))).into_response()
    }
}

async fn video_list(State(state):State<AppState>)->Json<Value>{
    Json(json!(crate::runtime::DominionSdk::from_state(state).video_list().await))
}
async fn video_create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let input=match serde_json::from_value::<crate::video::NewVideo>(body){
        Ok(value)=>value,
        Err(error)=>return (StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    };
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.video_create(input).await{
        Ok(video)=>(StatusCode::CREATED,Json(json!(video))).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn video_get(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.video_get(id).await{
        Ok(video)=>Json(json!(video)).into_response(),
        Err(error)=>(StatusCode::NOT_FOUND,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn video_render(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.video_render(id).await{
        Ok(video)=>(StatusCode::ACCEPTED,Json(json!(video))).into_response(),
        Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn video_scene(State(state):State<AppState>,
    Path((id,index)):Path<(String,usize)>,Json(body):Json<Value>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let change=match serde_json::from_value::<crate::video::ChangeScene>(body){
        Ok(value)=>value,
        Err(error)=>return (StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    };
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.video_scene(id,index,change).await{
        Ok(video)=>Json(json!(video)).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn video_content(State(state):State<AppState>,Path(id):Path<String>)->Response{
    use axum::body::{Body,Bytes};
    use tokio::io::AsyncReadExt;
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    let path=match sdk.video_output(id).await{
        Ok(path)=>path,
        Err(error)=>return (StatusCode::NOT_FOUND,Json(json!({"error":error.to_string()}))).into_response()
    };
    let file=match tokio::fs::File::open(&path).await {
        Ok(file)=>file,
        Err(error)=>return (StatusCode::NOT_FOUND,Json(json!({"error":error.to_string()}))).into_response()
    };
    let size=match file.metadata().await {
        Ok(meta) if meta.is_file() && meta.len()>0 && meta.len()<=160*1024*1024=>meta.len(),
        _=>return (StatusCode::CONFLICT,Json(json!({"error":"영상 출력 파일이 유효하지 않습니다."}))).into_response()
    };
    let body=async_stream::stream!{
        let mut file=file;
        loop{
            let mut chunk=vec![0u8;64*1024];
            match file.read(&mut chunk).await {
                Ok(0)=>break,
                Ok(count)=>yield Ok::<Bytes,std::io::Error>(Bytes::copy_from_slice(&chunk[..count])),
                Err(error)=>{yield Err(error);break}
            }
        }
    };
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE,"video/mp4".to_string()),
            (axum::http::header::CONTENT_LENGTH,size.to_string()),
            (axum::http::header::CONTENT_DISPOSITION,format!("attachment; filename=\"dominion-{id}.mp4\"")),
            (axum::http::header::CACHE_CONTROL,"no-store".to_string())
        ],
        Body::from_stream(body)
    ).into_response()
}
