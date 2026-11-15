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
use tower_http::services::{ServeDir, ServeFile};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health",get(health))
        .route("/api/providers",get(providers))
        .route("/api/tools",get(tools_catalog))
        .route("/api/settings/owner-profile",get(owner_profile).post(save_owner_profile))
        .route("/api/agents",get(agents).post(agent_create))
        .route("/api/agents/:id",axum::routing::delete(agent_delete))
        .route("/api/workflows",get(workflow_list).post(workflow_create))
        .route("/api/workflows/:id",get(workflow_get))
        .route("/api/workflows/:id/runs",get(workflow_runs).post(workflow_start))
        .route("/api/workflow-runs/:id",get(workflow_run))
        .route("/api/tasks",get(list).post(submit))
        .route("/api/tasks/:id",get(one))
        .route("/api/tasks/:id/cancel",post(cancel))
        .route("/api/tasks/:id/steer",post(steer))
        .route("/api/tasks/:id/resume",post(resume))
        .route("/api/tasks/:id/events",get(events))
        .route("/api/tasks/:id/children",get(subagent_children))
        .route("/api/tasks/:id/delegate",post(subagent_delegate))
        .route("/api/groups",get(group_list).post(group_create))
        .route("/api/groups/:id",get(group_get))
        .route("/api/groups/:id/messages",post(group_say))
        .route("/api/groups/:id/next",post(group_next))
        .route("/api/videos",get(video_list).post(video_create))
        .route("/api/videos/:id",get(video_get))
        .route("/api/videos/:id/render",post(video_render))
        .route("/api/videos/:id/scenes/:index",post(video_scene))
        .route("/api/videos/:id/content",get(video_content))
        .route("/api/improvements",get(improvement_list).post(improvement_create))
        .route("/api/improvements/:id",get(improvement_get))
        .route("/api/improvements/:id/evaluate",post(improvement_evaluate))
        .route("/api/improvements/:id/adopt",post(improvement_adopt))
        .route("/api/improvements/:id/descendant",post(improvement_descendant))
        .route("/api/memory",get(memory_list).post(memory_create))
        .route("/api/memory/search",get(memory_search))
        .route("/api/memory/:id/disable",post(memory_disable))
        .route("/api/skills",get(skill_list).post(skill_create))
        .route("/api/skills/:id/enable",post(skill_enable))
        .fallback_service({
            let root = std::env::var_os("DOMINION_WEB_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist"));
            ServeDir::new(&root).fallback(ServeFile::new(root.join("index.html")))
        })
        .with_state(state)
}

async fn health(State(state):State<AppState>) -> Json<Value> {
    Json(json!({"ok":true,"engine":"rust","model":state.model_name,
        "workspace":state.workspace.to_string_lossy(),"localOnly":true}))
}
async fn providers(State(state):State<AppState>)->Json<Value>{
    Json(state.provider.catalog())
}
async fn tools_catalog(State(state):State<AppState>)->Json<Value>{
    let builtins=[
        ("workspace_read","Read UTF-8 workspace files","read"),
        ("workspace_list","Inspect workspace file inventory","read"),
        ("workspace_search","Search source content","read"),
        ("workspace_replace","Verified exact source edit","write"),
        ("workspace_write","Verified local source write","write"),
        ("workspace_check","Run approved local tests","shell"),
        ("subagent_spawn","Delegate bounded native worker","delegate"),
        ("subagent_status","Inspect native child status","read")
    ];
    let mut tools=builtins.into_iter().map(|(name,description,permission)|json!({
        "name":name,"description":description,"permission":permission,
        "source":"native"
    })).collect::<Vec<_>>();
    let mcp=state.mcp.definitions().await;
    for descriptor in mcp{
        let function=&descriptor["function"];
        tools.push(json!({
            "name":function["name"],"description":function["description"],
            "parameters":function["parameters"],"permission":"mcp",
            "source":"mcp"
        }));
    }
    Json(json!({"tools":tools,"mcpServers":state.mcp.server_count()}))
}
async fn agents(State(state):State<AppState>)->Json<Value>{
    Json(json!(crate::runtime::DominionSdk::from_state(state).agents()))
}
async fn agent_create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.agent_create(
        body["name"].as_str().unwrap_or(""),
        body["description"].as_str().unwrap_or(""),
        body["instructions"].as_str().unwrap_or("")
    ) {
        Ok(record)=>(StatusCode::CREATED,Json(record)).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn agent_delete(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.agent_delete(&id) {
        Ok(changed)=>Json(json!({"ok":changed})).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn workflow_list(State(state):State<AppState>)->Json<Value>{
    Json(json!(crate::runtime::DominionSdk::from_state(state).workflows()))
}
async fn workflow_create(State(state):State<AppState>,Json(body):Json<Value>)->Response {
    let Some(name)=body["name"].as_str() else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"워크플로 이름이 필요합니다."}))).into_response();
    };
    let Some(steps)=body["steps"].as_array().filter(|steps|!steps.is_empty()) else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"실행할 단계가 필요합니다."}))).into_response();
    };
    let mut prompts=Vec::with_capacity(steps.len());
    for step in steps{
        let Some(prompt)=step.as_str() else{
            return (StatusCode::BAD_REQUEST,Json(json!({"error":"단계는 문자열이어야 합니다."}))).into_response();
        };
        prompts.push(prompt.to_string());
    }
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.workflow_create(name,&prompts){
        Ok(item)=>(StatusCode::CREATED,Json(item)).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error}))).into_response()
    }
}
async fn workflow_get(State(state):State<AppState>,Path(id):Path<String>)->Response {
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.workflows().into_iter().find(|flow|flow["id"]==id){
        Some(item)=>Json(item).into_response(),
        None=>(StatusCode::NOT_FOUND,Json(json!({"error":"워크플로를 찾지 못했습니다."}))).into_response()
    }
}
async fn workflow_runs(State(state):State<AppState>,Path(id):Path<String>)->Json<Value>{
    Json(json!(crate::runtime::DominionSdk::from_state(state).workflow_runs(&id)))
}
async fn workflow_run(State(state):State<AppState>,Path(id):Path<String>)->Response {
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.workflow_run(&id){
        Some(item)=>Json(item).into_response(),
        None=>(StatusCode::NOT_FOUND,Json(json!({"error":"실행 기록을 찾지 못했습니다."}))).into_response()
    }
}
async fn workflow_start(State(state):State<AppState>,Path(id):Path<String>,
    Json(mut options):Json<Value>)->Response {
    let Some(request_id)=options["requestId"].as_str().map(String::from) else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"requestId를 지정하세요."}))).into_response();
    };
    options.as_object_mut().map(|map|{map.remove("requestId");});
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.workflow_start(&id,&request_id,options).await{
        Ok(run)=>(StatusCode::ACCEPTED,Json(run)).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error}))).into_response()
    }
}

async fn owner_profile(State(state):State<AppState>) -> Json<Value> {
    Json(state.store.owner_profile())
}
async fn save_owner_profile(State(state):State<AppState>,Json(body):Json<Value>) -> Response {
    let Some(name)=body["name"].as_str() else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"사용자 이름이 필요합니다."}))).into_response();
    };
    let avatar=body["avatar"].as_str().unwrap_or("");
    match state.store.set_owner_profile(name,avatar) {
        Ok(profile)=>Json(profile).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error}))).into_response()
    }
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
async fn resume(State(state):State<AppState>,Path(id):Path<String>,
    Json(raw):Json<Value>) -> Response {
    let Ok(input)=serde_json::from_value::<crate::runtime::TaskInput>(raw) else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"새 작업 입력이 필요합니다."}))).into_response();
    };
    if input.request_id.is_none() {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"requestId가 필요합니다."}))).into_response();
    }
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.resume(&id,input).await {
        Ok(record)=>(StatusCode::ACCEPTED,Json(json!(record))).into_response(),
        Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error.to_string()}))).into_response()
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

async fn improvement_list(State(state):State<AppState>)->Json<Value>{
    Json(json!(crate::runtime::DominionSdk::from_state(state).improvement_list().await))
}
async fn improvement_create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let input=match serde_json::from_value::<crate::improvement::NewImprovement>(body){
        Ok(value)=>value,
        Err(error)=>return (StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    };
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.improvement_create(input).await{
        Ok(record)=>(StatusCode::CREATED,Json(json!(record))).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn improvement_get(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.improvement_get(id).await{
        Ok(item)=>Json(json!(item)).into_response(),
        Err(error)=>(StatusCode::NOT_FOUND,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn improvement_evaluate(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.improvement_evaluate(id).await{
        Ok(item)=>(StatusCode::ACCEPTED,Json(json!(item))).into_response(),
        Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn improvement_adopt(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.improvement_adopt(id).await{
        Ok(item)=>Json(json!(item)).into_response(),
        Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn improvement_descendant(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let id=match parse_room(&id){Ok(id)=>id,Err(error)=>return error};
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.improvement_descendant(id).await{
        Ok(item)=>(StatusCode::CREATED,Json(json!(item))).into_response(),
        Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error.to_string()}))).into_response()
    }
}

async fn memory_list(State(state):State<AppState>,
    Query(params):Query<HashMap<String,String>>)->Json<Value>{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    Json(json!(sdk.memory_list(params.get("sessionId").map(String::as_str))))
}
async fn memory_create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let Some(content)=body["content"].as_str() else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"기억 내용이 필요합니다."}))).into_response()
    };
    let session=body["sessionId"].as_str();
    let kind=body["kind"].as_str().unwrap_or("fact");
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.remember(session,content,kind){
        Ok(memory)=>(StatusCode::CREATED,Json(memory)).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn memory_search(State(state):State<AppState>,
    Query(params):Query<HashMap<String,String>>)->Json<Value>{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    Json(json!(sdk.memory_search(
        params.get("sessionId").map(String::as_str),
        params.get("query").map(String::as_str).unwrap_or(""),
        params.get("limit").and_then(|s|s.parse::<usize>().ok())
            .unwrap_or(8).min(10)
    )))
}
async fn memory_disable(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.memory_disable(&id){
        Ok(changed)=>Json(json!({"ok":changed})).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn skill_list(State(state):State<AppState>)->Json<Value>{
    Json(json!(crate::runtime::DominionSdk::from_state(state).skill_list()))
}
async fn skill_create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.skill_create(
        body["name"].as_str().unwrap_or(""),
        body["trigger"].as_str().unwrap_or(""),
        body["instructions"].as_str().unwrap_or(""),
        body["enabled"].as_bool().unwrap_or(false)
    ){
        Ok(skill)=>(StatusCode::CREATED,Json(skill)).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn skill_enable(State(state):State<AppState>,
    Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(enabled)=body["enabled"].as_bool() else {
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"enabled true/false를 지정하세요."}))).into_response()
    };
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.skill_enable(&id,enabled){
        Ok(changed)=>Json(json!({"ok":changed})).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    }
}

async fn subagent_children(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.children(&id){
        Ok(children)=>Json(json!(children)).into_response(),
        Err(error)=>(StatusCode::NOT_FOUND,Json(json!({"error":error.to_string()}))).into_response()
    }
}
async fn subagent_delegate(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let input=match serde_json::from_value::<crate::runtime::TaskInput>(body){
        Ok(input)=>input,
        Err(error)=>return (StatusCode::BAD_REQUEST,Json(json!({"error":error.to_string()}))).into_response()
    };
    let sdk=crate::runtime::DominionSdk::from_state(state);
    match sdk.delegate(&id,input).await{
        Ok(child)=>(StatusCode::ACCEPTED,Json(json!(child))).into_response(),
        Err(error)=>(StatusCode::CONFLICT,Json(json!({"error":error.to_string()}))).into_response()
    }
}
