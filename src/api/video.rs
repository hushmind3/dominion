use crate::{engine::{video,video_hub}, state::AppState};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::time::Duration;
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":message.into()}))).into_response()
}
async fn comfy_reachable(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    reqwest::Client::new()
        .get(format!("{}/system_stats", url.trim_end_matches('/')))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}
/// Query the actual running ComfyUI node registry before enabling AI renders.
/// A weight file on disk alone does not mean its graph can execute.
async fn missing_video_nodes(url:&str,cfg:&Value)->Vec<String>{
    let graph=if cfg["comfyWorkflow"].is_object(){
        cfg["comfyWorkflow"].clone()
    }else{
        let model=cfg["videoModel"].as_str().unwrap_or("");
        match video::workflow_for(model,cfg["steps"].as_u64().unwrap_or(20)){
            Some(graph)=>graph,None=>return Vec::new(),
        }
    };
    let Some(nodes)=graph.as_object() else{return Vec::new()};
    let required=nodes.values().filter_map(|node|node["class_type"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let result=reqwest::Client::new()
        .get(format!("{}/object_info",url.trim_end_matches('/')))
        .timeout(Duration::from_secs(4)).send().await;
    let info=match result{
        Ok(response) if response.status().is_success()=>
            match response.json::<Value>().await{
                Ok(info)=>info,
                Err(_)=>return vec!["ComfyUI 노드 정보를 읽을 수 없습니다.".into()],
            },
        _=>return vec!["ComfyUI 노드 목록에 연결할 수 없습니다.".into()],
    };
    required.into_iter().filter(|name|info.get(*name).is_none())
        .map(str::to_owned).collect()
}
pub fn outputs(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("job")
        .iter()
        .filter(|j| j["kind"] == "video" && j["status"] == "completed")
        .map(video::public_job)
        .collect()
}
pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let cfg = video::config(&state);
    let executable = video::ffmpeg(&state);
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let model_inventory = video::video_models(&state);
    let built_in = model_inventory.iter()
        .any(|model| model["id"] == cfg["videoModel"] && model["available"] == true);
    let missing_nodes=if reachable && (cfg["comfyWorkflow"].is_object() || built_in){
        missing_video_nodes(cfg["comfyUrl"].as_str().unwrap_or(""),&cfg).await
    }else{Vec::new()};
    let comfy = reachable && (cfg["comfyWorkflow"].is_object() || built_in)
        && missing_nodes.is_empty();
    let workflows = workflows_from_config(&cfg);
    let jobs = state.store.all("job").iter()
        .filter(|job| job["kind"] == "video")
        .map(video::public_job).collect::<Vec<_>>();
    Json(json!({
        "config": cfg,
        "runtime": "rust",
        "available": executable.is_some() || comfy,
        "native": {"available": executable.is_some()},
        "ffmpeg": {"available": executable.is_some(), "path": executable},
        "comfy": {"reachable": reachable, "available": comfy,
            "missingNodes":missing_nodes},
        "engines": [
            {"id":"native","name":"자막 카드 영상","available":executable.is_some(),"reason":if executable.is_some(){""}else{"FFmpeg 실행 파일 필요"}},
            {"id":"comfyui","name":"ComfyUI","available":comfy,"reason":if comfy{""}else{"서버와 API 워크플로 설정 필요"}}
        ],
        "jobs": jobs,
        "models": model_inventory,
        "modelsReady": built_in,
        "modelsDir": video::models_dir(&state),
        "modelsDirReady": video::models_dir_ready(&state),
        "workflows": workflows,
        "installed": reachable
    }))
}
pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(video::config(&state))
}
pub async fn save_config(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let mut cfg = video::config(&state);
    for key in [
        "engine",
        "duration",
        "aspectRatio",
        "comfyUrl",
        "comfyWorkflow",
        "comfyWorkflowName",
        "workflowDir",
        "outputDir",
        "ffmpegPath",
        "providerId",
        "model",
        "useModel",
        "voice",
        "rate",
        "narration",
        "subtitles",
        "videoModel",
        "resolution",
        "steps",
        "comfyModelsDir",
        "economy",
    ] {
        if let Some(v) = body.get(key) {
            cfg[key] = v.clone();
        }
    }
    if let Some(url) = cfg["comfyUrl"].as_str().filter(|s| !s.is_empty()) {
        if !reqwest::Url::parse(url).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && u.username().is_empty()
                && u.password().is_none()
        }) {
            return error(
                StatusCode::BAD_REQUEST,
                "ComfyUI HTTP(S) 주소를 확인하세요.",
            );
        }
    }
    if !cfg["comfyWorkflow"].is_null()
        && !cfg["comfyWorkflow"].as_object().is_some_and(|nodes| {
            !nodes.is_empty() && nodes.values().all(|node| {
                node["class_type"].is_string() && node["inputs"].is_object()
            })
        }) {
        return error(
            StatusCode::BAD_REQUEST,
            "API 형식 워크플로 JSON 객체가 필요합니다.",
        );
    }
    if cfg["comfyWorkflow"].to_string().len() > 1024 * 1024 {
        return error(
            StatusCode::BAD_REQUEST,
            "워크플로는 1MiB 이하로 저장하세요.",
        );
    }
    if state.store.put("video_config", &cfg).is_none() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "영상 설정을 저장할 수 없습니다.");
    }
    state.bus.record("video_config", &json!({"id":"default"}));
    Json(video::config(&state)).into_response()
}
pub async fn list_jobs(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state
        .store
        .all("job")
        .iter()
        .filter(|j| matches!(j["kind"].as_str(), Some("video" | "youtube_upload")))
        .map(video::public_job)
        .collect::<Vec<_>>()))
}
pub async fn create_job(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match video::enqueue(&state, &body) {
        Ok(j) => (StatusCode::ACCEPTED, Json(j)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
pub async fn job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state
        .store
        .get("job", &id)
        .filter(|j| matches!(j["kind"].as_str(), Some("video" | "youtube_upload")))
    {
        Some(j) => Json(video::public_job(&j)).into_response(),
        None => error(StatusCode::NOT_FOUND, "영상 작업을 찾을 수 없습니다."),
    }
}
pub async fn upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match video::enqueue_upload(&state, &id, &body) {
        Ok(j) => (StatusCode::ACCEPTED, Json(j)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
pub async fn content(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Response {
    let path = match video::output_path(&state, &id) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::NOT_FOUND, e),
    };
    match tower_http::services::ServeFile::new(path)
        .try_call(request)
        .await
    {
        Ok(response) => {
            let mut response = response.into_response();
            response
                .headers_mut()
                .insert("x-content-type-options", "nosniff".parse().unwrap());
            response
        }
        Err(_) => error(StatusCode::NOT_FOUND, "영상 파일을 열 수 없습니다."),
    }
}
/// Download owner-editable SRT with fixed safe filename, only after QA.
pub async fn captions(
    State(state):State<AppState>,Path(id):Path<String>,request:Request<Body>
)->Response{
    let output=match video::output_path(&state,&id){
        Ok(path)=>path,Err(e)=>return error(StatusCode::NOT_FOUND,e),
    };
    let source=output.with_file_name("captions.srt");
    if !source.is_file(){return error(StatusCode::NOT_FOUND,"이 영상에는 자막 파일이 없습니다.");}
    match tower_http::services::ServeFile::new(source).try_call(request).await{
        Ok(result)=>{
            let mut response=result.into_response();
            response.headers_mut().insert(axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8".parse().unwrap());
            response.headers_mut().insert(axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"dominion-captions.srt\"".parse().unwrap());
            response
        }
        Err(_)=>error(StatusCode::NOT_FOUND,"자막 파일을 찾지 못했습니다."),
    }
}
/// Frozen production bundle metadata accompanies the completed video.
pub async fn production(
    State(state):State<AppState>,Path(id):Path<String>,request:Request<Body>
)->Response{
    let output=match video::output_path(&state,&id){
        Ok(path)=>path,Err(e)=>return error(StatusCode::NOT_FOUND,e),
    };
    let source=output.with_file_name("production.json");
    if !source.is_file(){return error(StatusCode::NOT_FOUND,"제작 기록이 없습니다.");}
    match tower_http::services::ServeFile::new(source).try_call(request).await{
        Ok(result)=>{
            let mut response=result.into_response();
            response.headers_mut().insert(axum::http::header::CONTENT_TYPE,
                "application/json; charset=utf-8".parse().unwrap());
            response.headers_mut().insert(axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"dominion-production.json\"".parse().unwrap());
            response
        }
        Err(_)=>error(StatusCode::NOT_FOUND,"제작 기록을 찾지 못했습니다."),
    }
}
pub async fn outputs_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!(outputs(&state)))
}
pub async fn locations(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"outputs":[state.config.data_dir.join("video")],"comfy":[],"workflows":[]}))
}
fn workflows_from_config(cfg: &Value) -> Vec<Value> {
    if !cfg["comfyWorkflow"].is_object() {
        return Vec::new();
    }
    vec![json!({
        "id": "default",
        "name": cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),
        "bytes": cfg["comfyWorkflow"].to_string().len(),
    })]
}

pub async fn workflows(State(state): State<AppState>) -> Json<Value> {
    Json(json!(workflows_from_config(&video::config(&state))))
}

pub async fn models(State(state): State<AppState>) -> Json<Value> {
    let cfg = video::config(&state);
    let native = video::ffmpeg(&state).is_some();
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let comfy = reachable && cfg["comfyWorkflow"].is_object();
    let mut items = vec![json!({
        "id": "native",
        "name": "DOMINION native (ffmpeg)",
        "kind": "native",
        "available": native,
        "hint": if native { "로컬 ffmpeg으로 장면과 자막을 합성합니다." } else { "ffmpeg를 찾을 수 없습니다. 설정에서 경로를 지정하세요." },
    })];
    if comfy {
        items.push(json!({
            "id": "comfyui",
            "name": cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),
            "kind": "comfyui",
            "available": true,
            "hint": "연결된 ComfyUI 워크플로로 생성합니다.",
        }));
    }
    Json(json!(items))
}

/// User-facing one-action model setup: save selected engine, validate actual
/// ComfyUI models folder, and start/resume missing model assets.
pub async fn start_comfy(State(state):State<AppState>)->Response{
    match video::launch_comfy(state).await {
        Ok(info)=>(StatusCode::ACCEPTED,Json(info)).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message),
    }
}
pub async fn setup_model(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    match video::setup_model(state,&id,&body){
        Ok(value)=>(StatusCode::ACCEPTED,Json(value)).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message),
    }
}
pub async fn download_model(
    State(state): State<AppState>, Path(id): Path<String>,
) -> Response {
    match video::start_download(state.clone(), &id) {
        Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}

/// Discovery is video-only and does not mutate any selected chat or video model.
pub async fn hub_search(Query(query):Query<std::collections::HashMap<String,String>>)->Response{
    let term=query.get("q").map(String::as_str).unwrap_or("");
    match video_hub::search(term).await{
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message)
    }
}
pub async fn hub_files(Query(query):Query<std::collections::HashMap<String,String>>)->Response{
    let repo=query.get("repo").map(String::as_str).unwrap_or("");
    match video_hub::files(repo).await{
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message)
    }
}
pub async fn hub_download(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let repo=body["repo"].as_str().unwrap_or("");
    let path=body["path"].as_str().unwrap_or("");
    match video_hub::download(state,repo,path).await{
        Ok(value)=>(StatusCode::ACCEPTED,Json(value)).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message)
    }
}
pub async fn model_downloads(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"downloads": video::downloads(&state)}))
}

pub async fn catalog(State(state): State<AppState>) -> Json<Value> {
    let cfg = video::config(&state);
    let native = video::ffmpeg(&state).is_some();
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let available=video::video_models(&state).iter()
        .any(|model|model["id"]==cfg["videoModel"]&&model["available"]==true);
    let missing_nodes=if reachable&&(cfg["comfyWorkflow"].is_object()||available){
        missing_video_nodes(cfg["comfyUrl"].as_str().unwrap_or(""),&cfg).await
    }else{Vec::new()};
    let comfy=reachable&&(cfg["comfyWorkflow"].is_object()||available)
        && missing_nodes.is_empty();
    Json(json!({
        "missingNodes":missing_nodes,
        "comfyAvailable":comfy,
        "models": [
            {"id":"native","name":"DOMINION native (ffmpeg)","kind":"native","available":native},
            {"id":"comfyui","name":cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),"kind":"comfyui","available":comfy},
        ],
        "videoModels": video::video_models(&state),
        "modelsDir":video::models_dir(&state),
        "modelsDirReady":video::models_dir_ready(&state),
        "selectedVideoModel": cfg["videoModel"],
        "workflows": workflows_from_config(&cfg),
        "engine": cfg["engine"],
        "comfyUrl": cfg["comfyUrl"],
        "comfyReachable": reachable,
    }))
}

pub async fn files(State(state): State<AppState>) -> Json<Value> {
    Json(json!(outputs(&state)))
}

pub async fn reveal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let path = match video::output_path(&state, &id) {
        Ok(path) => path,
        Err(message) => return error(StatusCode::NOT_FOUND, message),
    };
    #[cfg(target_os = "macos")]
    let launched = tokio::process::Command::new("/usr/bin/open").arg("-R").arg(&path).status().await;
    #[cfg(target_os = "windows")]
    let launched = tokio::process::Command::new("explorer.exe")
        .arg(path.parent().expect("output directory")).status().await;
    #[cfg(all(unix, not(target_os = "macos")))]
    let launched = tokio::process::Command::new("xdg-open")
        .arg(path.parent().expect("output directory")).status().await;
    #[cfg(not(any(unix, windows)))]
    let launched: std::io::Result<std::process::ExitStatus> =
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported));

    match launched {
        Ok(status) if status.success() => Json(json!({"opened":true})).into_response(),
        _ => error(StatusCode::SERVICE_UNAVAILABLE, "영상 폴더를 열 수 없습니다."),
    }
}

pub async fn not_available() -> Response {
    error(
        StatusCode::NOT_IMPLEMENTED,
        "이 작업은 지원하지 않습니다. 영상 작업 만들기를 사용하세요.",
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-video-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir,
        };
        AppState::new(
            cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()),
        )
    }
    #[test]
    fn turnkey_installer_rejects_an_invalid_model_root_without_mutating_video_settings(){
        let s=state();
        let old=video::config(&s);
        let nonexistent=s.config.data_dir.join("missing-comfy-root").join("models");
        let result=video::setup_model(s.clone(),"wan21-t2v-1.3b-fp16",
            &json!({"modelsDir":nonexistent.to_string_lossy(),
                "comfyUrl":"http://127.0.0.1:8188"}));
        assert!(result.is_err());
        assert_eq!(video::config(&s)["videoModel"],old["videoModel"]);
        assert!(s.store.all("video_model_download").is_empty());
        assert!(video::setup_model(s.clone(),"unknown-model",&json!({})).is_err());
        crate::test_support::cleanup_dir(s.config.data_dir.clone());
    }
    #[tokio::test]
    async fn comfy_auto_launcher_refuses_remote_hosts_without_spawning(){
        let s=state();
        let mut cfg=video::config(&s);
        cfg["comfyUrl"]=json!("https://example.com");
        s.store.put("video_config",&cfg);
        assert!(video::launch_comfy(s.clone()).await.is_err());
        assert!(!s.active.lock().unwrap().contains("comfy-runtime-launch"));
        crate::test_support::cleanup_dir(s.config.data_dir.clone());
    }
    #[test]
    fn enqueue_is_durable_and_never_publishes_implicitly() {
        let s = state();
        let result = video::enqueue(
            &s,
            &json!({"brief":"테스트 영상","engine":"native","duration":3,"publish":true}),
        );
        if video::ffmpeg(&s).is_some() {
            let j = result.unwrap();
            let raw = s.store.get("job", j["id"].as_str().unwrap()).unwrap();
            assert_eq!(raw["input"]["publish"], false);
            assert_eq!(raw["status"], "queued");
            assert!(outputs(&s).is_empty());
        }
        assert!(video::enqueue(&s, &json!({"brief":"x","duration":900})).is_err());
        crate::test_support::cleanup_dir(s.config.data_dir.clone());
    }
    #[tokio::test]
    async fn native_pipeline_produces_a_real_playable_container() {
        let s = state();
        let require_ffmpeg = std::env::var("DOMINION_REQUIRE_VIDEO_FFMPEG")
            .ok().as_deref() == Some("1");
        if video::ffmpeg(&s).is_none() {
            assert!(!require_ffmpeg, "FFmpeg is required for the real-video acceptance test");
            return;
        }
        let j = video::enqueue(
            &s,
            &json!({"brief":"실제 영상 테스트. 한글 프레임 확인","duration":3}),
        )
        .unwrap();
        let id = j["id"].as_str().unwrap().to_string();
        let job = s.store.get("job", &id).unwrap();
        let ctx = crate::engine::jobs::JobContext::new(s.clone(), id.clone());
        let result = video::run(&ctx, &job).await.unwrap();
        assert!(result["bytes"].as_u64().unwrap() > 1000);
        let data = std::fs::read(video::directory(&s, &id).unwrap().join("output.mp4")).unwrap();
        assert_eq!(&data[4..8], b"ftyp");
        if require_ffmpeg {
            // The container signature alone is not enough; inspect the
            // decoder-visible stream of the actual finished MP4.
            let output_path = video::directory(&s, &id).unwrap().join("output.mp4");
            let probe = tokio::process::Command::new("ffprobe")
                .args(["-v", "error", "-select_streams", "v:0",
                    "-show_entries", "stream=codec_name,width,height",
                    "-of", "json"])
                .arg(&output_path).output().await.unwrap();
            assert!(probe.status.success(), "ffprobe failed: {}",
                String::from_utf8_lossy(&probe.stderr));
            let info:Value=serde_json::from_slice(&probe.stdout).unwrap();
            assert_eq!(info["streams"][0]["codec_name"], "h264");
            assert_eq!(info["streams"][0]["width"], 720);
            assert_eq!(info["streams"][0]["height"], 1280);
        }
        assert!(
            video::output_path(&s, &id).is_err(),
            "unfinished job must not be served"
        );
        crate::test_support::cleanup_dir(s.config.data_dir.clone());
    }
}
