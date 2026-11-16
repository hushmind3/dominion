//! DOMINION's durable media pipeline. Native cards are rendered by Rust; FFmpeg
//! only encodes them. Optional ComfyUI is an explicit HTTP rendering connector.
use crate::{
    engine::{
        jobs::{self, JobContext},
        oauth, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn config(state: &AppState) -> Value {
    let mut value = json!({"id":"default","engine":"native","duration":15,"aspectRatio":"9:16","comfyUrl":"","comfyWorkflow":null,"workflowDir":"","outputDir":"","ffmpegPath":"","providerId":"","model":"","useModel":false,"narration":true,"voice":"Yuna","rate":170,"videoModel":"","resolution":"480x832","steps":20,"comfyModelsDir":"","economy":false});
    if let Some(saved) = state.store.get("video_config", "default") {
        if let Some(map) = saved.as_object() {
            for (k, v) in map {
                if k != "videoApiKey" {
                    value[k] = v.clone();
                }
            }
        }
    }
    value["hasVideoKey"] = json!(state.store.has_secret("video:api"));
    value
}
/// Where ComfyUI keeps downloaded model files.
pub fn models_dir(state: &AppState) -> PathBuf {
    let cfg = config(state);
    if let Some(path)=cfg["comfyModelsDir"].as_str().filter(|value|!value.is_empty()) {
        return PathBuf::from(path);
    }
    // Discover portable installs too; never advertise the process working
    // directory's arbitrary "models" as a ready ComfyUI model library.
    let mut candidates=Vec::new();
    if let Some(path)=std::env::var_os("COMFYUI_MODELS_DIR") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home)=std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")) {
        let home=PathBuf::from(home);
        candidates.push(home.join("ComfyUI").join("models"));
        candidates.push(home.join("ComfyUI_windows_portable").join("ComfyUI").join("models"));
    }
    if let Ok(cwd)=std::env::current_dir() {
        candidates.push(cwd.join("ComfyUI").join("models"));
        candidates.push(cwd.join("ComfyUI_windows_portable").join("ComfyUI").join("models"));
    }
    candidates.into_iter().find(|candidate|candidate.is_dir())
        .unwrap_or_else(||state.config.data_dir.join("comfy-models-unconfigured"))
}
pub fn models_dir_ready(state:&AppState)->bool{
    models_dir(state).is_dir()
}
/// Start a local, already-installed ComfyUI runtime without opening a terminal.
/// Only a recognized Python environment alongside the selected ComfyUI root
/// may be launched. Never execute a downloaded model or arbitrary shell text.
pub async fn launch_comfy(state:AppState)->Result<Value,String>{
    let cfg=config(&state);
    let url=cfg["comfyUrl"].as_str().filter(|s|!s.is_empty())
        .unwrap_or("http://127.0.0.1:8188");
    let endpoint=reqwest::Url::parse(url).map_err(|_|"ComfyUI 주소를 확인하세요.")?;
    if endpoint.scheme()!="http"||
        !matches!(endpoint.host_str(),Some("127.0.0.1"|"localhost"|"::1"))||
        endpoint.username()!=""||endpoint.password().is_some()||
        endpoint.path()!="/"||endpoint.query().is_some(){
        return Err("자동 실행은 로컬 ComfyUI 주소(http://127.0.0.1:포트)에서만 지원합니다.".into());
    }
    let port=endpoint.port_or_known_default().ok_or("ComfyUI 포트가 없습니다.")?;
    if reqwest::Client::new().get(format!("{url}/system_stats"))
        .timeout(Duration::from_secs(2)).send().await
        .is_ok_and(|r|r.status().is_success()){
        return Ok(json!({"status":"running","url":url}));
    }
    let models=models_dir(&state);
    let root=models.parent().ok_or("ComfyUI 모델 폴더를 먼저 선택하세요.")?;
    let script=root.join("main.py");
    if !script.is_file(){
        return Err("선택한 models 폴더 옆에 ComfyUI/main.py가 없습니다. ComfyUI를 설치한 경로를 선택하세요.".into());
    }
    let portable=root.parent().map(|p|p.join("python_embeded").join("python.exe"));
    let executables=[
        portable.unwrap_or_default(),
        root.join(".venv").join("Scripts").join("python.exe"),
        root.join("venv").join("Scripts").join("python.exe"),
        root.join(".venv").join("bin").join("python"),
        root.join("venv").join("bin").join("python"),
    ];
    let python=executables.into_iter().find(|path|path.is_file())
        .ok_or("ComfyUI Python 실행 환경을 찾지 못했습니다. ComfyUI Portable 또는 가상환경을 설치하세요.")?;
    let slot="comfy-runtime-launch".to_string();
    {
        let mut active=state.active.lock().map_err(|_|"프로세스 실행 상태 오류")?;
        if !active.insert(slot.clone()){
            return Ok(json!({"status":"starting","url":url}));
        }
    }
    let launch=||->Result<tokio::process::Child,String>{
        std::fs::create_dir_all(&state.config.data_dir).map_err(|e|e.to_string())?;
        let log=state.config.data_dir.join("comfy-launch.log");
        let out=std::fs::OpenOptions::new().create(true).append(true)
            .open(&log).map_err(|e|e.to_string())?;
        let err=out.try_clone().map_err(|e|e.to_string())?;
        tokio::process::Command::new(&python)
            .arg("-s").arg(&script)
            .args(["--listen","127.0.0.1","--port"]).arg(port.to_string())
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(out))
            .stderr(std::process::Stdio::from(err))
            .kill_on_drop(false).spawn()
            .map_err(|e|format!("ComfyUI 실행 실패: {e}"))
    };
    let child=match launch(){
        Ok(child)=>child,
        Err(error)=>{
            state.active.lock().unwrap().remove(&slot);
            return Err(error);
        }
    };
    let pid=child.id();
    let watched=state.clone();
    tokio::spawn(async move{
        let mut child=child;
        let _=child.wait().await;
        if let Ok(mut active)=watched.active.lock(){active.remove(&slot);}
    });
    Ok(json!({"status":"starting","url":url,"pid":pid,
        "log":state.config.data_dir.join("comfy-launch.log")}))
}
/// Single setup action: choose the real ComfyUI models folder, persist the
/// model choice, and begin/resume all weight downloads without a shell.
pub fn setup_model(state:AppState,id:&str,body:&Value)->Result<Value,String>{
    let files=model_files(id).ok_or("지원하지 않는 영상 모델입니다.")?;
    let mut cfg=config(&state);
    if let Some(path)=body["modelsDir"].as_str().filter(|v|!v.trim().is_empty()){
        if path.len()>4096{return Err("모델 폴더 경로가 너무 깁니다.".into());}
        let folder=PathBuf::from(path);
        // The native picker often returns ComfyUI's root, or the portable
        // distribution root. Resolve both into the actual models directory.
        let candidates=[folder.join("ComfyUI").join("models"),
            folder.join("models"),folder.clone()];
        let candidate=candidates.into_iter().find(|candidate|candidate.is_dir())
            .ok_or("존재하는 ComfyUI/models 폴더를 선택하세요.")?;
        let absolute=candidate.canonicalize()
            .map_err(|_|"존재하는 ComfyUI/models 폴더를 선택하세요.")?;
        if !absolute.is_dir(){return Err("모델 저장 경로가 폴더가 아닙니다.".into());}
        cfg["comfyModelsDir"]=json!(absolute.to_string_lossy().to_string());
    }
    if let Some(url)=body["comfyUrl"].as_str().filter(|v|!v.trim().is_empty()){
        let parsed=reqwest::Url::parse(url).map_err(|_|"ComfyUI 연결 주소가 올바르지 않습니다.")?;
        if !matches!(parsed.scheme(),"http"|"https")||!parsed.username().is_empty()
            ||parsed.password().is_some(){return Err("ComfyUI 연결 주소가 올바르지 않습니다.".into());}
        cfg["comfyUrl"]=json!(url.trim());
    }else if cfg["comfyUrl"].as_str().unwrap_or("").trim().is_empty(){
        cfg["comfyUrl"]=json!("http://127.0.0.1:8188");
    }
    // Validate before changing saved configuration. Otherwise a failed
    // install can silently redirect subsequent renders to an invalid folder.
    let configured=cfg["comfyModelsDir"].as_str().map(PathBuf::from);
    let library=configured.unwrap_or_else(||models_dir(&state));
    if !library.is_dir(){
        return Err("ComfyUI의 models 폴더를 선택하세요. 설치되지 않은 폴더에 모델을 받지 않습니다.".into());
    }
    if !files.iter().all(|(relative,_)| {
        let name=Path::new(relative);
        !name.is_absolute()&&name.components().all(|component|
            matches!(component,std::path::Component::Normal(_)))
    }) {return Err("모델 경로 검증에 실패했습니다.".into())}
    cfg["engine"]=json!("comfyui");
    cfg["videoModel"]=json!(id);
    // A previously imported user graph would otherwise override the newly
    // selected model and make one-click setup appear to do nothing.
    cfg["comfyWorkflow"]=Value::Null;
    cfg["comfyWorkflowName"]=Value::Null;
    cfg["comfyModelsDir"]=json!(library.to_string_lossy().to_string());
    state.store.put("video_config",&cfg).ok_or("영상 모델 설정을 저장하지 못했습니다.")?;
    state.bus.record("video_config",&json!({"id":"default"}));
    let installed=files.iter().all(|(relative,_)|model_file_ready(relative,&library.join(relative)));
    if installed{
        return Ok(json!({"model":id,"status":"ready","modelsDir":library}));
    }
    match start_download(state.clone(),id){
        Ok(_) => Ok(json!({"model":id,"status":"downloading","modelsDir":library})),
        Err(error) if error=="이미 다운로드 중입니다." =>
            Ok(json!({"model":id,"status":"downloading","modelsDir":library})),
        Err(error)=>Err(error),
    }
}

// A zero-byte or tiny pointer/partial file is not a usable video weight.
// All built-in diffusion, text-encoder and VAE assets are comfortably >1 MiB.
const MIN_MODEL_ASSET_BYTES: u64 = 1024 * 1024;
fn model_asset_ready(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.len() >= MIN_MODEL_ASSET_BYTES)
}
// Each listed checkpoint has a verified multi-hundred-MB or multi-GB
// upstream payload. A 1-MB interrupted transfer is not a usable model.
fn asset_min_bytes(relative:&str)->u64{
    if relative.contains("Wan2.1-T2V-1.3B-Q4_K_M") {700_000_000}
    else if relative.contains("wan2.1_t2v_1.3B_fp16") {2_500_000_000}
    else if relative.contains("umt5-xxl-encoder-Q4_K_M") {3_000_000_000}
    else if relative.contains("umt5_xxl_fp8") {2_000_000_000}
    else if relative.contains("wan_2.1_vae") {200_000_000}
    else {MIN_MODEL_ASSET_BYTES}
}
fn model_file_ready(relative:&str,path:&Path)->bool{
    std::fs::metadata(path).is_ok_and(|meta|
        meta.is_file()&&meta.len()>=asset_min_bytes(relative))
}
fn present(state: &AppState, relative: &str) -> bool {
    model_file_ready(relative,&models_dir(state).join(relative))
}

/// Built-in one-click models. Availability is a real file check, not a claim.
pub fn video_models(state: &AppState) -> Vec<Value> {
    let entries = [
        ("wan21-t2v-1.3b-q4", "Wan2.1 T2V 1.3B · 4비트", "gguf",
            ["unet/Wan2.1-T2V-1.3B-Q4_K_M.gguf",
             "text_encoders/umt5-xxl-encoder-Q4_K_M.gguf",
             "vae/wan_2.1_vae.safetensors"]),
        ("wan21-t2v-1.3b-fp16", "Wan2.1 T2V 1.3B · fp16", "fp16",
            ["diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors",
             "text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors",
             "vae/wan_2.1_vae.safetensors"]),
    ];
    entries.iter().map(|(id, name, kind, files)| {
        let missing = files.iter().filter(|file| !present(state, file)).count();
        json!({
            "id": id, "name": name, "kind": kind, "files": files,
            "available": missing == 0,
            "missingFiles": missing,
            "hint": if missing == 0 { "설치됨".to_string() } else { format!("파일 {missing}개 없음") },
        })
    }).collect()
}

/// Workflow used when no imported workflow is set. Four-bit and fp16 loaders
/// differ only in the loader node; the sampler graph is identical.
pub fn workflow_for(model: &str, steps: u64) -> Option<Value> {
    let (unet, clip, vae, loader) = match model {
        "wan21-t2v-1.3b-q4" => (
            "Wan2.1-T2V-1.3B-Q4_K_M.gguf",
            "umt5-xxl-encoder-Q4_K_M.gguf",
            "wan_2.1_vae.safetensors",
            "gguf",
        ),
        "wan21-t2v-1.3b-fp16" => (
            "wan2.1_t2v_1.3B_fp16.safetensors",
            "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
            "wan_2.1_vae.safetensors",
            "fp16",
        ),
        _ => return None,
    };
    let (unet_node, clip_node) = if loader == "gguf" {
        (json!({"class_type":"UnetLoaderGGUF","inputs":{"unet_name":unet}}),
         json!({"class_type":"CLIPLoaderGGUF","inputs":{"clip_name":clip,"type":"wan"}}))
    } else {
        (json!({"class_type":"UNETLoader","inputs":{"unet_name":unet,"weight_dtype":"default"}}),
         json!({"class_type":"CLIPLoader","inputs":{"clip_name":clip,"type":"wan","device":"default"}}))
    };
    Some(json!({
        "1": unet_node,
        "2": clip_node,
        "3": {"class_type":"VAELoader","inputs":{"vae_name":vae}},
        "4": {"class_type":"CLIPTextEncode","inputs":{"text":"{{prompt}}","clip":["2",0]}},
        "5": {"class_type":"CLIPTextEncode","inputs":{"text":"{{negative}}","clip":["2",0]}},
        "6": {"class_type":"EmptyHunyuanLatentVideo","inputs":{
            "width":"{{width}}","height":"{{height}}","length":"{{frames}}","batch_size":1}},
        "7": {"class_type":"KSampler","inputs":{
            "model":["1",0],"positive":["4",0],"negative":["5",0],"latent_image":["6",0],
            "seed":"{{seed}}","steps":steps,"cfg":6.0,"sampler_name":"euler",
            "scheduler":"simple","denoise":1.0}},
        "8": {"class_type":"VAEDecode","inputs":{"samples":["7",0],"vae":["3",0]}},
        "9": {"class_type":"CreateVideo","inputs":{"images":["8",0],"fps":"{{fps}}"}},
        "10": {"class_type":"SaveVideo","inputs":{
            "video":["9",0],"filename_prefix":"dominion/wan","format":"mp4","codec":"h264"}}
    }))
}

/// Download sources for the built-in models. Kept out of the repository:
/// weights are large and licensed separately from the source.
pub fn model_files(id: &str) -> Option<Vec<(&'static str, &'static str)>> {
    let files = match id {
        "wan21-t2v-1.3b-q4" => vec![
            ("unet/Wan2.1-T2V-1.3B-Q4_K_M.gguf",
             "https://huggingface.co/samuelchristlie/Wan2.1-T2V-1.3B-GGUF/resolve/main/Wan2.1-T2V-1.3B-Q4_K_M.gguf"),
            ("text_encoders/umt5-xxl-encoder-Q4_K_M.gguf",
             "https://huggingface.co/city96/umt5-xxl-encoder-gguf/resolve/main/umt5-xxl-encoder-Q4_K_M.gguf"),
            ("vae/wan_2.1_vae.safetensors",
             "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/vae/wan_2.1_vae.safetensors"),
        ],
        "wan21-t2v-1.3b-fp16" => vec![
            ("diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors",
             "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors"),
            ("text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors",
             "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"),
            ("vae/wan_2.1_vae.safetensors",
             "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/vae/wan_2.1_vae.safetensors"),
        ],
        _ => return None,
    };
    Some(files)
}

/// Start a background download and record real progress. The UI polls the
/// record; nothing is marked installed until the file exists on disk.
pub fn start_download(state: AppState, id: &str) -> Result<Value, String> {
    let files = model_files(id).ok_or("알 수 없는 영상 모델입니다.")?;
    if !models_dir_ready(&state) {
        return Err("ComfyUI/models 폴더가 설정되지 않았습니다. 자율 콘텐츠 제작 화면에서 모델 저장 폴더를 먼저 선택하세요.".into());
    }
    let key = format!("video_model_download:{id}");
    if state.store.get("video_model_download", &key)
        .is_some_and(|row| row["status"] == "downloading")
    {
        return Err("이미 다운로드 중입니다.".into());
    }
    state.store.put("video_model_download", &json!({
        "id": key, "model": id, "status": "downloading", "progress": 0,
        "bytes": 0, "total": 0, "startedAt": chrono::Utc::now().to_rfc3339(),
    })).ok_or("다운로드 기록을 저장할 수 없습니다.")?;
    let base = models_dir(&state);
    tokio::spawn(async move {
        let outcome = download_files(&state, &key, &base, &files).await;
        let mut record = state.store.get("video_model_download", &key).unwrap_or_else(|| json!({"id": key}));
        match outcome {
            Ok(()) => { record["status"] = json!("completed"); record["progress"] = json!(100); }
            Err(error) => { record["status"] = json!("failed"); record["error"] = json!(error); }
        }
        record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("video_model_download", &record);
    });
    Ok(json!({"model": id, "status": "downloading"}))
}

async fn download_files(
    state: &AppState, key: &str, base: &Path, files: &[(&'static str, &'static str)],
) -> Result<(), String> {
    let client = reqwest::Client::builder().build().map_err(|e| e.to_string())?;
    let mut total = 0u64;
    let mut done = 0u64;
    for (index, (relative, url)) in files.iter().enumerate() {
        let target = base.join(relative);
        if model_file_ready(relative,&target) { continue; }
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }
        // Keep interrupted transfers as .part files. Only atomic promotion
        // after length verification permits a model to appear installed.
        let part = target.with_extension("part");
        let existing = tokio::fs::metadata(&part).await
            .ok().filter(|meta| meta.is_file()).map_or(0, |meta| meta.len());
        let mut request = client.get(*url);
        if existing > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        let status = response.status();
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && existing >= MIN_MODEL_ASSET_BYTES {
            let full = response.headers().get(reqwest::header::CONTENT_RANGE)
                .and_then(|header| header.to_str().ok())
                .and_then(|value| value.strip_prefix("bytes */"))
                .and_then(|value| value.parse::<u64>().ok());
            if full == Some(existing) {
                if target.exists() { tokio::fs::remove_file(&target).await.map_err(|e| e.to_string())?; }
                tokio::fs::rename(&part, &target).await.map_err(|e| e.to_string())?;
                continue;
            }
        }
        if !status.is_success() {
            return Err(format!("다운로드 실패: HTTP {status}"));
        }
        let resumed = existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
        if status == reqwest::StatusCode::PARTIAL_CONTENT {
            let expected_start = format!("bytes {existing}-");
            let range = response.headers().get(reqwest::header::CONTENT_RANGE)
                .and_then(|header| header.to_str().ok()).unwrap_or("");
            if !resumed || !range.starts_with(&expected_start) {
                return Err("다운로드 서버의 재개 범위가 로컬 파일과 일치하지 않습니다.".into());
            }
        }
        let prior = if resumed { existing } else { 0 };
        let remaining = response.content_length();
        let expected_total = remaining.and_then(|bytes| prior.checked_add(bytes));
        done = done.saturating_add(prior);
        total = total.saturating_add(expected_total.unwrap_or(prior));
        let mut file = tokio::fs::OpenOptions::new()
            .create(true).write(true).append(resumed).truncate(!resumed)
            .open(&part).await.map_err(|e| e.to_string())?;
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let mut stream = response.bytes_stream();
        let mut received = 0u64;
        let mut since_update = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            received = received.saturating_add(chunk.len() as u64);
            done = done.saturating_add(chunk.len() as u64);
            since_update += chunk.len() as u64;
            if since_update >= 8 * 1024 * 1024 {
                since_update = 0;
                let mut record = state.store.get("video_model_download", key)
                    .unwrap_or_else(|| json!({"id": key}));
                record["bytes"] = json!(done);
                record["total"] = json!(total.max(done));
                let file_fraction = expected_total.filter(|expected| *expected > 0)
                    .map_or(0, |expected| ((prior.saturating_add(received)).min(expected) * 100) / expected);
                record["progress"] = json!(((index as u64 * 100 + file_fraction)
                    / files.len().max(1) as u64).min(99));
                state.store.put("video_model_download", &record);
            }
        }
        file.flush().await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        if remaining.is_some_and(|length| received != length) {
            return Err(format!("모델 파일이 불완전합니다: {relative} (expected {remaining:?}, received {received})"));
        }
        if !model_file_ready(relative,&part) {
            return Err(format!("모델 파일 크기가 비정상적입니다: {relative}"));
        }
        if target.exists() { tokio::fs::remove_file(&target).await.map_err(|e| e.to_string())?; }
        tokio::fs::rename(&part, &target).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn downloads(state: &AppState) -> Vec<Value> {
    state.store.all("video_model_download")
}

pub fn ffmpeg(state: &AppState) -> Option<PathBuf> {
    ffmpeg_from_config(&config(state))
}

/// Resolve the encoder from the job's frozen config so a later settings change
/// cannot redirect an already queued render.
fn ffmpeg_from_config(cfg: &Value) -> Option<PathBuf> {
    let selected = cfg["ffmpegPath"].as_str().unwrap_or("");
    if !selected.is_empty() {
        let path = PathBuf::from(selected);
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let mut paths: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(name))
            .collect();
    paths.extend(
        [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    paths.into_iter().find(|p| p.is_file())
}
pub fn directory(state: &AppState, id: &str) -> Result<PathBuf, String> {
    // Job IDs are database-owned UUIDs, never arbitrary request paths.
    uuid::Uuid::parse_str(id).map_err(|_| "잘못된 영상 작업 ID")?;
    Ok(state.config.data_dir.join("video").join(id))
}
pub fn output_path(state: &AppState, id: &str) -> Result<PathBuf, String> {
    let record = state
        .store
        .get("job", id)
        .filter(|j| j["kind"] == "video")
        .ok_or("영상 작업이 없습니다.")?;
    if record["status"] != "completed" {
        return Err("완료된 영상이 없습니다.".into());
    }
    let root = directory(state, id)?
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    let path = root
        .join("output.mp4")
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    if !path.starts_with(root) {
        return Err("영상 경로를 확인할 수 없습니다.".into());
    }
    Ok(path)
}
/// Produce independently usable subtitle files for Shorts/Reels/TikTok.
fn subtitle_time(seconds:u64)->String{
    format!("{:02}:{:02}:{:02},000",seconds/3600,(seconds/60)%60,seconds%60)
}
fn subtitles(input:&Value)->Option<String>{
    let scenes=input["scenes"].as_array()?;
    let mut cursor=0u64;
    let mut result=String::new();
    for (i,scene) in scenes.iter().enumerate(){
        let seconds=scene["seconds"].as_u64()?;
        let caption=scene["overlay"].as_str().filter(|s|!s.trim().is_empty())
            .or_else(||scene["script"].as_str()).unwrap_or("");
        if caption.trim().is_empty()||seconds==0{return None}
        result.push_str(&format!("{}\n{} --> {}\n{}\n\n",
            i+1,subtitle_time(cursor),subtitle_time(cursor.saturating_add(seconds)),
            caption.trim()));
        cursor=cursor.saturating_add(seconds);
    }
    (!result.is_empty()).then_some(result)
}
pub fn public_job(job: &Value) -> Value {
    let id = job["id"].as_str().unwrap_or("");
    let ready = job["kind"] == "video" && job["status"] == "completed";
    json!({"id":id,"kind":job["kind"],"title":job["input"]["title"],"brief":job["input"]["brief"],"engine":job["input"]["engine"],"status":job["status"],"phase":job.get("phase").unwrap_or(&job["status"]),"progress":job["progress"],"error":job["error"],"createdAt":job.get("createdAt").or(job.get("created_at")),"videoJobId":job["input"]["videoJobId"],"privacyStatus":job["input"]["privacyStatus"],"outputUrl":ready.then(||format!("/api/video/jobs/{id}/content")),
        "captionsUrl":(ready&&job["input"]["scenes"].is_array())
            .then(||format!("/api/video/jobs/{id}/captions")),"storyboard":job["videoState"]["scenes"],"result":job["result"],"duration":job["input"]["duration"],"aspectRatio":job["input"]["aspectRatio"],"renderBudget":job["input"]["render"]["budget"]})
}
/// Deterministic preflight work estimate: frames x diffusion steps x pixels.
//// Economy is explicit opt-in; it never silently changes the user's quality.
fn render_budget(duration: u64, fps: u64, steps: u64,
    width: u64, height: u64, economy: bool) -> Value {
    let effective_fps = if economy { fps.min(12) } else { fps };
    let effective_steps = if economy { steps.min(12) } else { steps };
    let units = |f: u64, s: u64| -> u128 {
        u128::from(duration) * u128::from(f) * u128::from(s)
            * u128::from(width) * u128::from(height)
    };
    let baseline = units(fps,steps);
    let reduced = units(effective_fps,effective_steps);
    json!({
        "mode":if economy{"economy"}else{"standard"},
        "fps":effective_fps,"steps":effective_steps,
        "frames":duration.saturating_mul(effective_fps),
        "estimatedPixelSteps":reduced.to_string(),
        "baselinePixelSteps":baseline.to_string(),
        "estimatedWorkReductionPercent":if baseline > 0 {
            ((baseline.saturating_sub(reduced))*100/baseline) as u64
        } else { 0 },
        "note":"Compute proxy only; runtime, electricity and provider charges are not inferred"
    })
}

pub fn enqueue(state: &AppState, body: &Value) -> Result<Value, String> {
    enqueue_inner(state,body,None)
}
pub fn enqueue_project(state:&AppState,body:&Value,job_id:&str)->Result<Value,String>{
    uuid::Uuid::parse_str(job_id).map_err(|_|"영상 렌더 ID 오류")?;
    enqueue_inner(state,body,Some(job_id))
}
fn enqueue_inner(state: &AppState, body: &Value, job_id:Option<&str>) -> Result<Value, String> {
    let cfg = config(state);
    let brief = body["brief"].as_str().unwrap_or("").trim();
    if brief.is_empty() || brief.len() > 16_000 {
        return Err("영상 주제를 1~16,000바이트로 입력하세요.".into());
    }
    let engine = body["engine"]
        .as_str()
        .or(cfg["engine"].as_str())
        .unwrap_or("native");
    if !matches!(engine, "native" | "comfyui") {
        return Err("지원하지 않는 영상 엔진입니다.".into());
    }
    if engine == "native" && ffmpeg(state).is_none() {
        return Err("FFmpeg가 필요합니다. 고급 설정에서 실행 파일을 지정하세요.".into());
    }
    if engine == "comfyui" {
        let built_in = video_models(state).iter()
            .any(|model| model["id"] == cfg["videoModel"] && model["available"] == true);
        if cfg["comfyUrl"].as_str().unwrap_or("").is_empty()
            || (!cfg["comfyWorkflow"].is_object() && !built_in)
        {
            return Err("ComfyUI 주소와 영상 모델(또는 API 형식 워크플로 JSON)을 먼저 저장하세요.".into());
        }
    }
    let duration = body["duration"]
        .as_u64()
        .or(cfg["duration"].as_u64())
        .unwrap_or(15);
    if !(3..=180).contains(&duration) {
        return Err("영상 길이는 3~180초입니다.".into());
    }
    let aspect = body["aspectRatio"]
        .as_str()
        .or(cfg["aspectRatio"].as_str())
        .unwrap_or("9:16");
    if !matches!(aspect, "9:16" | "16:9" | "1:1") {
        return Err("지원 비율은 9:16,16:9,1:1입니다.".into());
    }
    let title = body["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(brief)
        .chars()
        .take(90)
        .collect::<String>();
    let fps = body["fps"].as_u64().unwrap_or(16);
    let seed = body["seed"].as_i64().unwrap_or(-1);
    let economy = body["economy"].as_bool()
        .or(cfg["economy"].as_bool()).unwrap_or(false);
    let negative_prompt = body["negativePrompt"].as_str().unwrap_or("");
    if !(1..=60).contains(&fps) || seed < -1 || negative_prompt.len() > 16_000 {
        return Err("FPS, 시드, 네거티브 프롬프트 값을 확인하세요.".into());
    }
    // A caller that picked a concrete resolution keeps it. Only the engine
    // that actually consumes width/height (ComfyUI) may override the ratio.
    let configured_size = cfg["resolution"].as_str()
        .and_then(|value| value.split_once('x'))
        .and_then(|(width, height)| Some((width.parse::<u64>().ok()?, height.parse::<u64>().ok()?)));
    let requested_size = if engine == "comfyui" {
        body["width"].as_u64().or(configured_size.map(|size| size.0))
            .zip(body["height"].as_u64().or(configured_size.map(|size| size.1)))
            .filter(|(width, height)| {
                (128..=1024).contains(width)
                    && (128..=1024).contains(height)
                    && width % 16 == 0
                    && height % 16 == 0
            })
    } else {
        None
    };
    let (width, height) = requested_size.unwrap_or_else(|| match aspect {
        "16:9" => (640, 360),
        "1:1" => (480, 480),
        _ => (360, 640),
    });
    // Imported workflows may contain hard-coded nodes, so don't claim that
    // changing a budget field will reduce their actual compute.
    let applicable_economy=economy&&(engine!="comfyui"||!cfg["comfyWorkflow"].is_object());
    let plan = render_budget(duration,fps,cfg["steps"].as_u64().unwrap_or(20).clamp(1,60),
        width,height,applicable_economy && engine == "comfyui");
    let mut frozen_cfg = cfg.clone();
    frozen_cfg["steps"] = plan["steps"].clone();
    if economy && engine == "native" {
        // Explicit economy mode always skips the optional paid storyboard call.
        frozen_cfg["useModel"] = json!(false);
    }
    let effective_fps = if engine == "comfyui" { plan["fps"].as_u64().unwrap_or(fps) } else { fps };
    let seed = if seed == -1 {
        // Freeze one seed for this job so a safe retry uses identical input.
        i64::from(u32::from_be_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap()))
    } else {
        seed
    };
    let input = json!({
        "brief":brief, "title":title, "engine":engine, "duration":duration,
        "aspectRatio":aspect, "config":frozen_cfg, "publish":false,
        "projectId":body["projectId"],"projectRevision":body["projectRevision"],
        "scenes":body["scenes"],"studioAutoUpload":body["studioAutoUpload"],
        "studioPrivacyStatus":body["studioPrivacyStatus"],
        "render":{"prompt":brief,"negative":negative_prompt,"negativePrompt":negative_prompt,
                  "duration":duration,"width":width,"height":height,
                  "fps":effective_fps,"frames":duration*effective_fps,
                  "steps":plan["steps"],"economy":applicable_economy,
                  "economyRequested":economy,"budget":plan,"seed":seed}
    });
    let queued=if let Some(job_id)=job_id {
        jobs::enqueue_once(state,job_id,"video",input)
    }else{jobs::enqueue(state,"video",input)};
    queued.map(|j|public_job(&j))
        .ok_or("영상 작업을 저장할 수 없습니다.".into())
}
fn progress(ctx: &JobContext, phase: &str, percent: u64) -> Result<(), String> {
    ctx.checkpoint(json!({"phase":phase,"progress":percent}))
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut line = String::new();
    let mut used = 0;
    for c in s.chars().take(280) {
        if c == '\n' || used >= width {
            lines.push(std::mem::take(&mut line));
            used = 0;
            if c == '\n' {
                continue;
            }
        }
        line.push(c);
        used += if c.is_ascii() { 1 } else { 2 };
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.truncate(8);
    lines
}
fn make_cards(dir: &Path, title: &str, scenes: &[String], aspect: &str) -> Result<(), String> {
    let (w, h) = match aspect {
        "16:9" => (1280, 720),
        "1:1" => (900, 900),
        _ => (720, 1280),
    };
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let family = options
        .fontdb
        .faces()
        .find(|f| {
            f.families.iter().any(|(name, _)| {
                name.contains("Apple SD Gothic")
                    || name.contains("Malgun")
                    || name.contains("Noto Sans CJK")
            })
        })
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone())
        .unwrap_or_else(|| "sans-serif".into());
    for (i, scene) in scenes.iter().enumerate() {
        let font = if aspect == "16:9" { 42 } else { 38 };
        let lines = wrap(scene, if aspect == "16:9" { 48 } else { 29 });
        let mut text = String::new();
        let top = (h as i32 / 2) - (lines.len() as i32 * (font + 18) / 2);
        for (n, line) in lines.iter().enumerate() {
            text.push_str(&format!("<text x='{}' y='{}' text-anchor='middle' fill='#fff9ef' font-size='{font}' font-family='{}'>{}</text>",w/2,top+n as i32*(font+18),escape(&family),escape(line)));
        }
        let heading = wrap(title, if aspect == "16:9" { 70 } else { 38 })
            .first()
            .cloned()
            .unwrap_or_default();
        let svg=format!("<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}'><defs><linearGradient id='bg' x2='1' y2='1'><stop stop-color='#171f32'/><stop offset='1' stop-color='#314862'/></linearGradient></defs><rect width='{w}' height='{h}' fill='url(#bg)'/><circle cx='{}' cy='{}' r='240' fill='#c96442' opacity='.15'/><rect x='48' y='86' width='80' height='5' rx='2' fill='#edb78e'/><text x='48' y='140' font-family='{}' font-size='22' fill='#edb78e'>{}</text>{text}<text x='48' y='{}' font-family='sans-serif' font-size='20' fill='#d0d9e4'>{:02} / {:02}</text><rect x='48' y='{}' width='{}' height='4' fill='#edb78e'/></svg>",w,h,escape(&family),escape(&heading),h-55,i+1,scenes.len(),h-30,(w-96)*(i+1)as u32/scenes.len()as u32);
        // Deterministic scene-specific art direction: vivid variation without
        // image-generation API calls or cross-scene model drift.
        let themes = [
            ("#171f32", "#314862", "#c96442", "#edb78e"),
            ("#102b35", "#28616b", "#51b5ad", "#b4eee1"),
            ("#292038", "#62476b", "#bd79a1", "#f9c6e3"),
            ("#302720", "#756043", "#e3aa5b", "#ffe6b8"),
            ("#182b2b", "#30564b", "#79b68e", "#c5eaca"),
        ];
        let (background, shade, halo, accent) = themes[i % themes.len()];
        let svg = svg
            .replace("#171f32", background)
            .replace("#314862", shade)
            .replace("#c96442", halo)
            .replace("#edb78e", accent);
        let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
        let mut bitmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("영상 프레임 메모리 부족")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::default(),
            &mut bitmap.as_mut(),
        );
        bitmap
            .save_png(dir.join(format!("card-{i:02}.png")))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
/// Offline Korean narration. Each scene is spoken to its own file so the
/// video length follows the audio, not a fixed duration.
/// Offline narration without an account or paid API. Apple uses `say`;
/// Windows uses built-in System.Speech. Other systems keep a playable silent
/// video rather than pretending audio was generated.
async fn speak_scene(path: &Path, scene: &str, voice: &str, rate: &str) -> Option<()> {
    let status = if cfg!(target_os = "macos") {
        tokio::process::Command::new("say")
            .args(["-v", voice, "-r", rate, "-o"])
            .arg(path)
            .arg("--")
            .arg(scene)
            .status().await.ok()?
    } else if cfg!(target_os = "windows") {
        // Never interpolate arbitrary user text into PowerShell source.
        // Environment variables pass speech content as data.
        let script = r#"
Add-Type -AssemblyName System.Speech
$tts = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {
    $tts.Rate = [int]$env:DOMINION_SPEECH_RATE
    if ($env:DOMINION_SPEECH_VOICE) {
        try { $tts.SelectVoice($env:DOMINION_SPEECH_VOICE) } catch {}
    }
    $tts.SetOutputToWaveFile($env:DOMINION_SPEECH_FILE)
    $tts.Speak($env:DOMINION_SPEECH_TEXT)
} finally { $tts.Dispose() }
"#;
        let windows_rate = ((rate.parse::<i32>().unwrap_or(170) - 170) / 15)
            .clamp(-10, 10).to_string();
        tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("DOMINION_SPEECH_FILE", path)
            .env("DOMINION_SPEECH_TEXT", scene)
            .env("DOMINION_SPEECH_RATE", windows_rate)
            .env("DOMINION_SPEECH_VOICE", voice)
            .status().await.ok()?
    } else {
        return None;
    };
    status.success().then_some(())
}

async fn say_scenes(
    dir: &Path, scenes: &[String], cfg: &Value, encoder: &Path,
) -> Option<(Vec<f64>, std::path::PathBuf)> {
    if cfg["narration"] == false { return None; }
    let voice = cfg["voice"].as_str().filter(|value| !value.is_empty()).unwrap_or("Yuna");
    let rate = cfg["rate"].as_u64().unwrap_or(170).clamp(80, 320).to_string();
    let probe_name = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    let sibling = encoder.with_file_name(probe_name);
    let ffprobe = if sibling.exists() { sibling } else { PathBuf::from(probe_name) };
    let extension = if cfg!(windows) { "wav" } else { "aiff" };
    let mut durations = Vec::new();
    let mut list = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        let file = format!("narration-{index:02}.{extension}");
        let path = dir.join(&file);
        speak_scene(&path, scene, voice, &rate).await?;
        let seconds = probe_duration(&ffprobe, &path).await?;
        if seconds < 0.2 { return None; }
        durations.push((seconds + 0.35).max(1.0));
        list.push_str(&format!("file '{file}'\n"));
    }
    std::fs::write(dir.join("narration.txt"), list).ok()?;
    let status = tokio::process::Command::new(encoder)
        .current_dir(dir)
        .args(["-hide_banner","-loglevel","error","-y","-f","concat","-safe","0",
            "-i","narration.txt","-c:a","aac","-b:a","128k","narration.m4a"])
        .status().await.ok()?;
    if !status.success() { return None; }
    Some((durations, dir.join("narration.m4a")))
}

async fn probe_duration(ffprobe: &Path, file: &Path) -> Option<f64> {
    let output = tokio::process::Command::new(ffprobe)
        .args(["-v","error","-show_entries","format=duration","-of","default=nw=1:nk=1"])
        .arg(file)
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn native_motion_filter(aspect: &str) -> String {
    let size = match aspect {
        "16:9" => "1280x720",
        "1:1" => "900x900",
        _ => "720x1280",
    };
    // Gentle digital camera movement upgrades otherwise static scene cards
    // without requiring a GPU or remote image/video model.
    format!("fps=24,zoompan=z='min(zoom+0.0011,1.10)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s={size}:fps=24,format=yuv420p")
}

async fn native(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    progress(ctx, "storyboard", 5)?;
    let brief = input["brief"].as_str().unwrap_or("");
    let title = input["title"].as_str().unwrap_or("");
    let cfg = &input["config"];
    let authored = input["scenes"].as_array();
    let mut scenes: Vec<String> = brief
        .split(['\n', '.', '!', '?', '。'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(12)
        .map(str::to_string)
        .collect();
    if let Some(authored) = authored {
        scenes=authored.iter().filter_map(|v|v["script"].as_str())
            .map(str::to_owned).collect();
        if scenes.is_empty() || scenes.len()>12 {
            return Err("저장된 스토리보드의 장면 수가 올바르지 않습니다.".into());
        }
    } else if cfg["useModel"] == true {
        let text=crate::engine::llm::complete(&ctx.state,cfg["providerId"].as_str().unwrap_or(""),cfg["model"].as_str().unwrap_or(""),"영상 자막 스토리보드를 만드세요. JSON 문자열 배열만 반환하세요. 3~8개 장면, 장면당 한국어 100자 이내. 사실을 지어내지 마세요.",brief).await?;
        scenes = serde_json::from_str::<Vec<String>>(
            text.trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim(),
        )
        .map_err(|_| "모델 스토리보드가 JSON 문자열 배열이 아닙니다.")?;
        if scenes.is_empty() || scenes.len() > 12 || scenes.iter().any(|s| s.len() > 2000) {
            return Err("스토리보드 장면 수나 길이가 범위를 넘었습니다.".into());
        }
    }
    if scenes.is_empty() {
        scenes.push(brief.into());
    }
    ctx.checkpoint(json!({"videoState":{"scenes":scenes},"phase":"rendering","progress":20}))?;
    // The creative editor controls both spoken words and optional on-screen text.
    let cards=authored.map(|shots|shots.iter().enumerate().map(|(i,shot)|
        shot["overlay"].as_str().filter(|s|!s.trim().is_empty())
            .unwrap_or_else(||scenes.get(i).map(String::as_str).unwrap_or(""))
            .to_string()).collect::<Vec<_>>()).unwrap_or_else(||scenes.clone());
    make_cards(
        dir,
        title,
        &cards,
        input["aspectRatio"].as_str().unwrap_or("9:16"),
    )?;
    let encoder = ffmpeg_from_config(&input["config"]).ok_or("FFmpeg를 찾을 수 없습니다.")?;
    let requested = input["duration"].as_u64().unwrap_or(15);
    // Turnkey: speak every scene first. Fall back to a silent slideshow when
    // no voice engine is available, never failing the whole render for audio.
    let narration = say_scenes(dir, &scenes, &input["config"], &encoder).await;
    let mut concat = String::new();
    let total_seconds;
    if let Some((durations, _)) = &narration {
        for (index, seconds) in durations.iter().enumerate() {
            concat.push_str(&format!("file 'card-{index:02}.png'\nduration {seconds:.5}\n"));
        }
        concat.push_str(&format!("file 'card-{:02}.png'\n", scenes.len() - 1));
        total_seconds = durations.iter().sum::<f64>();
    } else {
        for index in 0..scenes.len() {
            let seconds=authored.and_then(|a|a.get(index))
                .and_then(|v|v["seconds"].as_f64())
                .unwrap_or(requested as f64 / scenes.len() as f64);
            concat.push_str(&format!("file 'card-{index:02}.png'\nduration {seconds:.5}\n"));
        }
        concat.push_str(&format!("file 'card-{:02}.png'\n", scenes.len() - 1));
        total_seconds = requested as f64;
    }
    std::fs::write(dir.join("frames.txt"), concat).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("storyboard.json"),
        json!({"title":title,"scenes":scenes,"duration":total_seconds.round() as u64,
            "audio":if narration.is_some() {"narration"} else {"silent"}}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    let mut command = tokio::process::Command::new(&encoder);
    command.current_dir(dir).args([
        "-hide_banner","-loglevel","error","-nostdin","-y","-f","concat","-safe","1","-i","frames.txt",
    ]);
    if narration.is_some() {
        command.args(["-i","narration.m4a","-map","0:v:0","-map","1:a:0"]);
    } else {
        command.args(["-f","lavfi","-i","anullsrc=r=48000:cl=stereo"]);
    }
    let motion_filter = native_motion_filter(input["aspectRatio"].as_str().unwrap_or("9:16"));
    command.args([
        "-t",&format!("{:.3}", total_seconds + 0.2),
        "-vf",&motion_filter,"-c:v","libx264","-preset","veryfast","-crf","22",
        "-c:a","aac","-movflags","+faststart","-shortest","output.part.mp4",
    ]);
    progress(ctx, "encoding", 50)?;
    let process = tools::run_command(&mut command, Duration::from_secs(600));
    tokio::pin!(process);
    let output = loop {
        tokio::select! {result=&mut process=>break result?,_ = tokio::time::sleep(Duration::from_millis(500))=>{ctx.checkpoint(json!({}))?;}}
    };
    if !output.status.success() {
        return Err(format!(
            "FFmpeg 인코딩 실패: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(1500).collect::<String>()
        ));
    }
    std::fs::rename(dir.join("output.part.mp4"), dir.join("output.mp4"))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "engine":"native",
        "description":if narration.is_some() {"한국어 내레이션 카드 영상"} else {"Rust 자막 카드 영상 · 무음"},
        "duration":total_seconds.round() as u64,
        "scenes":scenes.len(),
        "narrated":narration.is_some(),
        "motion":"subtle-zoom",
        "sceneArt":"deterministic-palettes",
    }))
}

fn substitute(value: &mut Value, variables: &Value) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = text.strip_prefix("{{").and_then(|s| s.strip_suffix("}}"))
                .and_then(|key| variables.get(key)) {
                *value = replacement.clone();
                return;
            }
            let mut expanded = String::new();
            let mut rest = text.as_str();
            while let Some((prefix, candidate)) = rest.split_once("{{") {
                let Some((key, suffix)) = candidate.split_once("}}") else { break };
                expanded.push_str(prefix);
                match variables.get(key) {
                    Some(Value::String(replacement)) => expanded.push_str(replacement),
                    Some(replacement) => expanded.push_str(&replacement.to_string()),
                    None => { expanded.push_str("{{"); expanded.push_str(key); expanded.push_str("}}"); }
                }
                rest = suffix;
            }
            expanded.push_str(rest);
            *text = expanded;
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, variables)),
        Value::Object(fields) => fields.values_mut().for_each(|item| substitute(item, variables)),
        _ => {}
    }
}
async fn comfy(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let Some(shots)=input["scenes"].as_array() else {
        return comfy_single(ctx,input,dir).await;
    };
    if shots.len()<=1 {
        let mut single=input.clone();
        if let Some(shot)=shots.first(){
            let prompt=shot["visualPrompt"].as_str().filter(|s|!s.trim().is_empty())
                .or_else(||shot["script"].as_str()).unwrap_or("");
            single["brief"]=json!(prompt);
            single["render"]["prompt"]=json!(prompt);
        }
        return comfy_single(ctx,&single,dir).await;
    }
    // Custom graphs may ignore the input prompt or frame budget. Never silently
    // run the same hard-coded graph 8 times and label it a scene montage.
    if input["config"]["comfyWorkflow"].is_object()
        && !input["config"]["comfyWorkflow"].to_string().contains("{{prompt}}") {
        return Err("멀티 장면 생성에는 {{prompt}} 변수를 사용하는 ComfyUI API 워크플로가 필요합니다.".into());
    }
    let encoder=ffmpeg_from_config(&input["config"])
        .ok_or("여러 ComfyUI 장면을 하나의 영상으로 합치려면 FFmpeg가 필요합니다.")?;
    let mut concat=String::new();
    for (index,shot) in shots.iter().enumerate(){
        ctx.checkpoint(json!({"phase":"rendering",
            "progress":10+(index*75/shots.len()) as u64,
            "sceneIndex":index+1,"sceneTotal":shots.len()}))?;
        let prompt=shot["visualPrompt"].as_str().filter(|s|!s.trim().is_empty())
            .or_else(||shot["script"].as_str()).unwrap_or("");
        let seconds=shot["seconds"].as_u64().unwrap_or(3);
        let fps=input["render"]["fps"].as_u64().unwrap_or(16);
        let mut child=input.clone();
        child["brief"]=json!(prompt);
        child["duration"]=json!(seconds);
        child["render"]["prompt"]=json!(prompt);
        child["render"]["duration"]=json!(seconds);
        child["render"]["frames"]=json!(seconds.saturating_mul(fps));
        let base_seed=input["render"]["seed"].as_i64().unwrap_or(0);
        child["render"]["seed"]=json!(base_seed.saturating_add(index as i64));
        comfy_single(ctx,&child,dir).await?;
        let file=format!("segment-{index:02}.mp4");
        std::fs::rename(dir.join("output.mp4"),dir.join(&file))
            .map_err(|e|e.to_string())?;
        concat.push_str(&format!("file '{file}'\n"));
    }
    std::fs::write(dir.join("segments.txt"),concat).map_err(|e|e.to_string())?;
    ctx.checkpoint(json!({"phase":"encoding","progress":90}))?;
    let mut command=tokio::process::Command::new(encoder);
    command.current_dir(dir).args(["-hide_banner","-loglevel","error","-nostdin",
        "-y","-f","concat","-safe","1","-i","segments.txt",
        "-c:v","libx264","-preset","veryfast","-crf","22",
        "-c:a","aac","-movflags","+faststart","output.part.mp4"]);
    let outcome=tools::run_command(&mut command,Duration::from_secs(600)).await?;
    if !outcome.status.success(){
        return Err(format!("AI 장면 합성 실패: {}",
            String::from_utf8_lossy(&outcome.stderr).chars().take(1500).collect::<String>()));
    }
    std::fs::rename(dir.join("output.part.mp4"),dir.join("output.mp4"))
        .map_err(|e|e.to_string())?;
    Ok(json!({"engine":"comfyui","sceneCount":shots.len(),"composition":"scene_montage"}))
}
async fn comfy_single(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let cfg = &input["config"];
    let base = cfg["comfyUrl"].as_str().unwrap_or("").trim_end_matches('/');
    let parsed = reqwest::Url::parse(base).map_err(|_| "ComfyUI 주소 오류")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("ComfyUI HTTP(S) 주소를 확인하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut workflow = if cfg["comfyWorkflow"].is_object() {
        cfg["comfyWorkflow"].clone()
    } else {
        let steps = cfg["steps"].as_u64().unwrap_or(20).clamp(1, 60);
        workflow_for(cfg["videoModel"].as_str().unwrap_or(""), steps)
            .ok_or("영상 모델을 설정에서 선택하세요.")?
    };
    let legacy = json!({"prompt":input["brief"],"duration":input["duration"]});
    substitute(&mut workflow, input.get("render").unwrap_or(&legacy));
    ctx.checkpoint(json!({"phase":"submitting","progress":10,"videoState":{"inFlight":true}}))?;
    let response = client
        .post(format!("{base}/prompt"))
        .json(&json!({"prompt":workflow,"client_id":ctx.job_id}))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 결과를 확인할 수 없습니다.")?;
    if !response.status().is_success() {
        ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
        return Err(format!("ComfyUI 워크플로 거부: HTTP {}", response.status()));
    }
    let answer: Value = response
        .json()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 응답 오류")?;
    let prompt_id = answer["prompt_id"]
        .as_str()
        .ok_or("needs_review: ComfyUI prompt_id가 없습니다.")?;
    ctx.checkpoint(json!({"phase":"rendering","progress":25,"videoState":{"inFlight":true,"promptId":prompt_id}}))?;
    let start = std::time::Instant::now();
    loop {
        ctx.checkpoint(json!({}))?;
        if start.elapsed() > Duration::from_secs(900) {
            return Err(
                "needs_review: ComfyUI 생성 대기시간이 지났습니다. 원격 작업 상태를 확인하세요."
                    .into(),
            );
        }
        let history: Value = client
            .get(format!("{base}/history/{prompt_id}"))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let result = &history[prompt_id];
        if result["status"]["status_str"] == "error" {
            ctx.checkpoint(json!({"videoState":{"inFlight":false,"promptId":prompt_id}}))?;
            return Err("ComfyUI 워크플로 실행 실패".into());
        }
        if let Some(outputs) = result["outputs"].as_object() {
            for node in outputs.values() {
                // SaveVideo reports mp4 files under "images" with animated=true.
                for field in ["videos", "gifs", "images"] {
                    if let Some(files) = node[field].as_array() {
                        for file in files {
                            let filename = file["filename"].as_str().unwrap_or("");
                            if !filename.to_lowercase().ends_with(".mp4") {
                                continue;
                            }
                            let response = client
                                .get(format!("{base}/view"))
                                .query(&[
                                    ("filename", filename),
                                    ("subfolder", file["subfolder"].as_str().unwrap_or("")),
                                    ("type", file["type"].as_str().unwrap_or("output")),
                                ])
                                .timeout(Duration::from_secs(120))
                                .send()
                                .await
                                .map_err(|e| e.to_string())?;
                            if !response.status().is_success() {
                                return Err("ComfyUI 결과 다운로드 실패".into());
                            }
                            use futures_util::StreamExt;
                            use tokio::io::AsyncWriteExt;
                            let mut stream = response.bytes_stream();
                            let mut total = 0usize;
                            let mut output = tokio::fs::File::create(dir.join("output.part.mp4"))
                                .await
                                .map_err(|e| e.to_string())?;
                            while let Some(chunk) = stream.next().await {
                                ctx.checkpoint(json!({}))?;
                                let chunk = chunk.map_err(|e| e.to_string())?;
                                total += chunk.len();
                                if total > 256 * 1024 * 1024 {
                                    return Err("영상 결과가 256MiB 제한을 넘었습니다.".into());
                                }
                                output.write_all(&chunk).await.map_err(|e| e.to_string())?;
                            }
                            drop(output);
                            std::fs::rename(dir.join("output.part.mp4"), dir.join("output.mp4"))
                                .map_err(|e| e.to_string())?;
                            ctx.checkpoint(
                                json!({"videoState":{"inFlight":false,"promptId":prompt_id}}),
                            )?;
                            return Ok(
                                json!({"engine":"comfyui","promptId":prompt_id,"bytes":total}),
                            );
                        }
                    }
                }
            }
            if result["status"]["completed"] == true {
                return Err(
                    "ComfyUI 완료 결과에 MP4가 없습니다. 영상 출력 노드를 확인하세요.".into(),
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
pub async fn run(ctx: &JobContext, job: &Value) -> Result<Value, String> {
    let dir = directory(&ctx.state, &ctx.job_id)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let input = &job["input"];
    let result = if input["engine"] == "comfyui" {
        comfy(ctx, input, &dir).await
    } else {
        native(ctx, input, &dir).await
    };
    match result {
        Ok(mut value) => {
            let bytes = std::fs::metadata(dir.join("output.mp4"))
                .map_err(|e| e.to_string())?
                .len();
            if bytes < 100 {
                return Err("생성된 영상이 비어 있습니다.".into());
            }
            // A finished file must be a genuine ISO-BMFF MP4 container.
            // This is cheap and always enforced before any publishing job.
            use std::io::Read;
            let mut file=std::fs::File::open(dir.join("output.mp4"))
                .map_err(|e|e.to_string())?;
            let mut header=[0u8;12];
            file.read_exact(&mut header).map_err(|e|e.to_string())?;
            if &header[4..8]!=b"ftyp"{
                return Err("렌더 결과가 올바른 MP4 컨테이너가 아닙니다.".into());
            }
            let mut qa=json!({"containerVerified":true,"bytes":bytes,
                "captionTimeline":"source_scene_seconds"});
            if let Some(srt)=subtitles(input) {
                std::fs::write(dir.join("captions.srt"),srt).map_err(|e|e.to_string())?;
                qa["captionsReady"]=json!(true);
            }
            let manifest=json!({"title":input["title"],"projectId":input["projectId"],
                "projectRevision":input["projectRevision"],"scenes":input["scenes"],
                "render":input["render"],"qa":qa});
            std::fs::write(dir.join("production.json"),manifest.to_string())
                .map_err(|e|e.to_string())?;
            value["qa"]=qa;
            value["bytes"] = json!(bytes);
            value["outputUrl"] = json!(format!("/api/video/jobs/{}/content", ctx.job_id));
            progress(ctx, "completed", 100)?;
            Ok(value)
        }
        Err(e) => {
            let _ = std::fs::remove_file(dir.join("output.part.mp4"));
            if ctx
                .state
                .store
                .get("job", &ctx.job_id)
                .is_some_and(|j| j["videoState"]["inFlight"] == true)
                && !e.starts_with("needs_review:")
            {
                Err(format!("needs_review: {e}"))
            } else {
                Err(e)
            }
        }
    }
}
pub fn enqueue_upload(state: &AppState, id: &str, body: &Value) -> Result<Value, String> {
    output_path(state, id)?;
    let video = state.store.get("job", id).ok_or("영상이 없습니다.")?;
    if oauth::access_token(state, "youtube").is_none() {
        return Err("연결 메뉴에서 YouTube 업로드 계정을 먼저 연결하세요.".into());
    }
    let privacy = body["privacyStatus"].as_str().unwrap_or("private");
    if !matches!(privacy, "private" | "unlisted" | "public") {
        return Err("공개 범위 오류".into());
    }
    use sha2::Digest;
    let digest = sha2::Sha256::digest(format!("dominion-youtube:{id}"));
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&digest[..16]);
    let upload_id = uuid::Uuid::from_bytes(uuid).to_string();
    let made_for_kids = body["madeForKids"].as_bool().unwrap_or(false);
    let title = body["title"].as_str().or_else(|| video["input"]["title"].as_str())
        .unwrap_or_default().trim();
    let description = body["description"].as_str().or_else(|| video["input"]["brief"].as_str())
        .unwrap_or_default();
    let tags = body["tags"].as_str().unwrap_or("").split(',')
        .map(str::trim).filter(|tag| !tag.is_empty()).collect::<Vec<_>>();
    if title.is_empty() || title.chars().count() > 100 || description.len() > 5000
        || tags.join(",").chars().count() > 500 {
        return Err("YouTube 제목, 설명, 태그 길이를 확인하세요.".into());
    }
    let candidate = json!({
        "id":upload_id,"kind":"youtube_upload","status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "input":{"videoJobId":id,"privacyStatus":privacy,"madeForKids":made_for_kids,
                 "title":title,"description":description,"tags":tags},
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    let job = if let Some(inserted) = state.store.put_if_absent("job", &candidate) {
        state.bus.record("job", &inserted);
        state.store.audit(
            "job.queued",
            json!({"kind":"youtube_upload","jobId":upload_id}),
        );
        inserted
    } else {
        let existing = state
            .store
            .get("job", &upload_id)
            .ok_or("업로드 작업 조회 실패")?;
        if existing["status"] == "failed" && existing["videoState"]["inFlight"] != true {
            state.store.patch_if("job",&upload_id,"status",&json!("failed"),&json!({"status":"queued","error":null,"attempts":0,"queuedAt":chrono::Utc::now().to_rfc3339(),"input":candidate["input"]})).unwrap_or(existing)
        } else {
            existing
        }
    };
    Ok(public_job(&job))
}
pub async fn upload(ctx: &JobContext, job: &Value) -> Result<Value, String> {
    if let Some(result) = job
        .pointer("/videoState/result")
        .filter(|r| r["youtubeId"].as_str().is_some_and(|s| !s.is_empty()))
    {
        return Ok(result.clone());
    }
    if job["videoState"]["inFlight"] == true {
        return Err("needs_review: 이전 업로드 결과를 먼저 확인하세요.".into());
    }
    let id = job["input"]["videoJobId"]
        .as_str()
        .ok_or("영상 작업 ID 필요")?;
    let path = output_path(&ctx.state, id)?;
    let video = ctx.state.store.get("job", id).ok_or("영상이 없습니다.")?;
    let token = oauth::fresh_access_token(&ctx.state, "youtube").await?;
    let size = tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?
        .len();
    if size > 256 * 1024 * 1024 {
        return Err("업로드 크기 제한은 256MiB입니다.".into());
    }
    let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size {
        return Err("업로드 중 파일 크기가 변경되었습니다. 다시 시도하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    progress(ctx, "authorizing", 5)?;
    let title = job["input"]["title"].as_str().or_else(|| video["input"]["title"].as_str());
    let description = job["input"]["description"].as_str().or_else(|| video["input"]["brief"].as_str());
    let tags = job["input"]["tags"].as_array().cloned().unwrap_or_default();
    let response=client.post("https://www.googleapis.com/upload/youtube/v3/videos").query(&[("uploadType","resumable"),("part","snippet,status")]).bearer_auth(&token).header("X-Upload-Content-Length",bytes.len()).header("X-Upload-Content-Type","video/mp4").json(&json!({"snippet":{"title":title,"description":description,"tags":tags},"status":{"privacyStatus":job["input"]["privacyStatus"],"selfDeclaredMadeForKids":job["input"]["madeForKids"].as_bool().unwrap_or(false)}})).timeout(Duration::from_secs(30)).send().await.map_err(|_|"YouTube 업로드 세션 연결 실패")?;
    if !response.status().is_success() {
        return Err(format!(
            "YouTube 업로드 세션 거부: HTTP {}",
            response.status()
        ));
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or("YouTube 업로드 주소가 없습니다.")?;
    let url = reqwest::Url::parse(location).map_err(|_| "YouTube 업로드 주소 오류")?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("www.googleapis.com" | "upload.youtube.com")
        )
    {
        return Err("YouTube가 반환한 업로드 호스트를 확인할 수 없습니다.".into());
    }
    ctx.checkpoint(json!({"phase":"uploading","progress":20,"videoState":{"inFlight":true}}))?;
    // Persist the opaque session URL encrypted. A crash after PUT is uncertain:
    // it is never automatically submitted again and cannot create duplicates.
    ctx.state
        .save_secret(&format!("youtube-upload:{}", ctx.job_id), location);
    let response = client
        .put(url)
        .bearer_auth(token)
        .header("Content-Type", "video/mp4")
        .header("Content-Length", bytes.len())
        .body(bytes)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|_| {
            "needs_review: YouTube 업로드 결과가 불확실합니다. 업로드 기록을 확인하세요."
        })?;
    if !response.status().is_success() {
        return Err(format!(
            "needs_review: YouTube 업로드 응답 HTTP {}. 원격 상태를 확인하세요.",
            response.status()
        ));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "needs_review: YouTube 업로드 결과 해석 실패")?;
    let youtube_id = value["id"]
        .as_str()
        .filter(|s| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .ok_or("needs_review: YouTube 영상 ID 없음")?;
    let result = json!({"youtubeId":youtube_id,"url":format!("https://www.youtube.com/watch?v={youtube_id}"),"privacyStatus":job["input"]["privacyStatus"]});
    ctx.checkpoint(
        json!({"phase":"completed","progress":100,"videoState":{"inFlight":false,"result":result}}),
    )?;
    ctx.state
        .store
        .remove_secret(&format!("youtube-upload:{}", ctx.job_id));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_comfy_workflows_must_not_claim_unenforced_economy_reduction() {
        let custom=json!({"comfyWorkflow":{"1":{"class_type":"CustomSampler",
            "inputs":{"steps":44,"fps":48}}}});
        assert!(custom["comfyWorkflow"].is_object());
        let proposed=true;
        let applies=proposed&&!custom["comfyWorkflow"].is_object();
        let budget=render_budget(9,24,40,480,832,applies);
        assert_eq!(budget["estimatedWorkReductionPercent"],0);
        assert_eq!(budget["fps"],24);
        assert_eq!(budget["steps"],40);
    }
    #[test]
    fn economy_mode_reduces_real_diffusion_frames_and_steps_without_hiding_the_budget() {
        let normal = render_budget(12,24,40,480,832,false);
        let economy = render_budget(12,24,40,480,832,true);
        assert_eq!(normal["frames"],288);
        assert_eq!(normal["steps"],40);
        assert_eq!(economy["frames"],144);
        assert_eq!(economy["fps"],12);
        assert_eq!(economy["steps"],12);
        assert_eq!(economy["estimatedWorkReductionPercent"],85);
        let normal_units = normal["estimatedPixelSteps"].as_str().unwrap().parse::<u128>().unwrap();
        let cheap_units = economy["estimatedPixelSteps"].as_str().unwrap().parse::<u128>().unwrap();
        assert_eq!(cheap_units*40*24,normal_units*12*12);
    }

    #[test]
    fn video_weights_require_a_real_completed_asset_not_a_placeholder() {
        let dir = std::env::temp_dir().join(format!("dominion-model-ready-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let weight = dir.join("diffusion.safetensors");
        let part = dir.join("diffusion.part");
        assert!(!model_asset_ready(&weight));
        std::fs::write(&weight, b"").unwrap();
        assert!(!model_asset_ready(&weight));
        std::fs::write(&weight, b"git-lfs version https://git-lfs.github.com/spec/v1").unwrap();
        assert!(!model_asset_ready(&weight), "a Git LFS pointer is not model weights");
        std::fs::remove_file(&weight).unwrap();
        std::fs::write(&part, vec![7u8; MIN_MODEL_ASSET_BYTES as usize]).unwrap();
        assert!(!model_asset_ready(&weight), "partial files must never appear installed");
        std::fs::rename(part,&weight).unwrap();
        assert!(model_asset_ready(&weight));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn frozen_ffmpeg_path_wins_over_current_settings() {
        let dir = std::env::temp_dir().join(format!("dominion-ffmpeg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("ffmpeg-custom");
        std::fs::write(&fake, b"binary").unwrap();
        let cfg = json!({"ffmpegPath": fake.to_string_lossy()});
        assert_eq!(ffmpeg_from_config(&cfg).as_deref(), Some(fake.as_path()));
        assert!(ffmpeg_from_config(&json!({"ffmpegPath":"/definitely/missing/ffmpeg"})).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn native_cards_render_unicode_and_escape_markup() {
        let dir =
            std::env::temp_dir().join(format!("dominion-video-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        make_cards(
            &dir,
            "테스트 <제목>",
            &["한글 자막 & 실제 프레임".into()],
            "9:16",
        )
        .unwrap();
        let bytes = std::fs::read(dir.join("card-00.png")).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        assert!(bytes.len() > 10000);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn native_video_motion_matches_all_aspect_ratios() {
        for (aspect, size) in [("9:16", "720x1280"), ("16:9", "1280x720"), ("1:1", "900x900")] {
            let filter = native_motion_filter(aspect);
            assert!(filter.contains("zoompan=z="));
            assert!(filter.contains(&format!("s={size}:")));
            assert!(filter.contains("format=yuv420p"));
        }
    }

    #[test]
    fn template_substitution_never_changes_json_structure() {
        let mut value = json!({"1":{"inputs":{"text":"{{prompt}}","seconds":"{{duration}}"}}});
        substitute(&mut value, &json!({"prompt":"\"},evil:{","duration":15}));
        assert_eq!(value["1"]["inputs"]["text"], "\"},evil:{");
        assert_eq!(value["1"]["inputs"]["seconds"], 15);
        assert_eq!(value.as_object().unwrap().len(), 1);
    }
}
