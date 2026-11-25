use crate::{addons::models, state::AppState};
use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncWriteExt;

const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

fn find_file(dir: &Path, depth: usize, matches: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    if depth > 4 || !dir.is_dir() {
        return None;
    }
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if matches(&path) {
            return Some(path);
        }
    }
    dirs.into_iter().find_map(|dir| find_file(&dir, depth + 1, matches))
}

/// Whisper weights are plain ggml/gguf files; prefer a file that names the
/// engine so a chat model in the same folder is never picked by accident.
fn whisper_model(state: &AppState) -> Option<PathBuf> {
    let root = models::models_dir(state);
    let is_whisper = |path: &Path| -> bool {
        let Some(name) = path.file_name() else { return false };
        let name = name.to_string_lossy().to_ascii_lowercase();
        let Some(ext) = path.extension() else { return false };
        let ext = ext.to_string_lossy().to_ascii_lowercase();
        (ext == "gguf" || ext == "bin")
            && (name.contains("whisper") || name.contains("ggml"))
    };
    find_file(&root, 0, &is_whisper)
}

fn piper_voice(state: &AppState) -> Option<PathBuf> {
    let root = models::models_dir(state);
    let is_onnx = |path: &Path| -> bool {
        if !path
            .extension()
            .map(|value| value.to_string_lossy().eq_ignore_ascii_case("onnx"))
            .unwrap_or(false)
        {
            return false;
        }
        let mut config = path.as_os_str().to_os_string();
        config.push(".json");
        Path::new(&config).is_file()
    };
    find_file(&root, 0, &is_onnx)
}

fn engine_status() -> Value {
    json!({
        "whisperBin": models::voice_bin(&["whisper-cli", "whisper-cpp", "whisper"])
            .map(|path| path.to_string_lossy().to_string()),
        "piperBin": models::voice_bin(&["piper"]).map(|path| path.to_string_lossy().to_string()),
    })
}

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let engines = engine_status();
    let whisper = whisper_model(&state);
    let piper = piper_voice(&state);
    Json(json!({
        "whisper": {
            "available": engines["whisperBin"].is_string() && whisper.is_some(),
            "engine": engines["whisperBin"],
            "model": whisper.map(|path| path.to_string_lossy().to_string()),
            "language": "ko",
        },
        "piper": {
            "available": engines["piperBin"].is_string() && piper.is_some(),
            "engine": engines["piperBin"],
            "model": piper.map(|path| path.to_string_lossy().to_string()),
        },
    }))
}

pub async fn transcribe(State(state): State<AppState>, body: Bytes) -> Response {
    if body.len() < 128 {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "오디오가 비어 있습니다."}))).into_response();
    }
    if body.len() > MAX_AUDIO_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({"error": "오디오가 너무 큽니다."}))).into_response();
    }
    let Some(engine) = models::voice_bin(&["whisper-cli", "whisper-cpp", "whisper"]) else {
        return (StatusCode::CONFLICT, Json(json!({"error": "whisper 실행기가 설치되어 있지 않습니다."}))).into_response();
    };
    let Some(model) = whisper_model(&state) else {
        return (StatusCode::CONFLICT, Json(json!({"error": "whisper 모델이 설치되어 있지 않습니다."}))).into_response();
    };
    let work = std::env::temp_dir().join(format!("dominion-voice-{}", uuid::Uuid::new_v4()));
    if std::fs::create_dir_all(&work).is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "임시 폴더를 만들지 못했습니다."}))).into_response();
    }
    let audio = work.join("input.wav");
    let out = work.join("out");
    let write = async {
        let mut file = tokio::fs::File::create(&audio).await?;
        file.write_all(&body).await?;
        file.flush().await
    };
    if let Err(error) = write.await {
        let _ = std::fs::remove_dir_all(&work);
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": error.to_string()}))).into_response();
    }
    let run = tokio::time::timeout(
        Duration::from_secs(300),
        tokio::process::Command::new(&engine)
            .arg("-m").arg(&model)
            .arg("-f").arg(&audio)
            .arg("-l").arg("ko")
            .arg("-otxt")
            .arg("-of").arg(&out)
            .stdin(Stdio::null())
            .output(),
    )
    .await;
    let text = match run {
        Ok(Ok(output)) if output.status.success() => {
            let file = out.with_extension("txt");
            std::fs::read_to_string(&file).unwrap_or_default().trim().to_string()
        }
        Ok(Ok(output)) => {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let _ = std::fs::remove_dir_all(&work);
            return (StatusCode::BAD_GATEWAY, Json(json!({"error": error}))).into_response();
        }
        Ok(Err(error)) => {
            let _ = std::fs::remove_dir_all(&work);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": error.to_string()}))).into_response();
        }
        Err(_) => {
            let _ = std::fs::remove_dir_all(&work);
            return (StatusCode::GATEWAY_TIMEOUT, Json(json!({"error": "음성인식 시간 초과"}))).into_response();
        }
    };
    let _ = std::fs::remove_dir_all(&work);
    Json(json!({"text": text})).into_response()
}

pub async fn speak(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let text = body.get("text").and_then(|value| value.as_str()).unwrap_or("").trim();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "읽을 문장이 없습니다."}))).into_response();
    }
    if text.chars().count() > 2000 {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "문장이 너무 깁니다."}))).into_response();
    }
    let Some(engine) = models::voice_bin(&["piper"]) else {
        return (StatusCode::CONFLICT, Json(json!({"error": "piper 실행기가 설치되어 있지 않습니다."}))).into_response();
    };
    let Some(model) = piper_voice(&state) else {
        return (StatusCode::CONFLICT, Json(json!({"error": "piper 음성 모델이 설치되어 있지 않습니다."}))).into_response();
    };
    let work = std::env::temp_dir().join(format!("dominion-tts-{}", uuid::Uuid::new_v4()));
    if std::fs::create_dir_all(&work).is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "임시 폴더를 만들지 못했습니다."}))).into_response();
    }
    let out = work.join("speech.wav");
    let mut child = match tokio::process::Command::new(&engine)
        .arg("-m").arg(&model)
        .arg("-f").arg(&out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": error.to_string()}))).into_response();
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(format!("{text}\n").as_bytes()).await;
        let _ = stdin.shutdown().await;
    }
    let result = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output()).await;
    match result {
        Ok(Ok(output)) if output.status.success() => {}
        Ok(Ok(output)) => {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let _ = std::fs::remove_dir_all(&work);
            return (StatusCode::BAD_GATEWAY, Json(json!({"error": error}))).into_response();
        }
        Ok(Err(error)) => {
            let _ = std::fs::remove_dir_all(&work);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": error.to_string()}))).into_response();
        }
        Err(_) => {
            let _ = std::fs::remove_dir_all(&work);
            return (StatusCode::GATEWAY_TIMEOUT, Json(json!({"error": "음성합성 시간 초과"}))).into_response();
        }
    }
    match std::fs::read(&out) {
        Ok(bytes) => {
            let _ = std::fs::remove_dir_all(&work);
            (
                [
                    (header::CONTENT_TYPE, "audio/wav".to_string()),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": error.to_string()}))).into_response()
        }
    }
}
