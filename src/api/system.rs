use crate::engine;
use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use std::{collections::HashMap, io::{Read, Seek, SeekFrom}, path::PathBuf};
use serde_json::{json,Value};
use std::time::Duration;
use std::convert::Infallible;
use futures_util::Stream;

struct RemoveFileOnDrop(PathBuf);

impl Drop for RemoveFileOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = self.0.as_os_str().to_os_string();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(sidecar));
        }
    }
}

pub async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "dominion",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "engine": "rust",
        "executionKernel":"dominion-agent-sdk",
        "executionKernelOrigin":"dominion-sub3",
        "independentHarness":"dominion-harness",
        "instanceId":std::env::var("DOMINION_ENGINE_INSTANCE").ok(),
        // "commit" is retained for existing clients. A running binary's
        // embedded SHA is independent of checkout HEAD, which may advance
        // while the watch daemon is still building a replacement process.
        "commit": crate::build_identity::running().commit,
        "binaryBuildSha": crate::build_identity::running().commit,
        "branch": crate::build_identity::running().branch,
        "dirty": crate::build_identity::running().dirty,
        "identitySource": crate::build_identity::running().source,
    }))
}

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let running = state.store.active_job_count();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "engine": "rust",
        "executionKernel":"dominion-agent-sdk",
        "executionKernelOrigin":"dominion-sub3",
        "independentHarness":"dominion-harness",
        "providers": state.store.count("provider"),
        "agents": state.store.count("agent"),
        "sessions": state.store.count("session"),
        "rooms": state.store.count("room"),
        "mcp": state.store.count("mcp"),
        "skills": state.store.count("skill"),
        "memories": state.store.count("memory"),
        "jobs": running,
        "executions": state.store.count("execution"),
        "workspace": state.config.workspace.to_string_lossy(),
        "data": state.config.data_dir.to_string_lossy(),
    }))
}

pub async fn events(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Reconnect contract: `?after=<seq>` or the SSE `Last-Event-ID` header.
    let after = params
        .get("after")
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| {
            headers
                .get("last-event-id")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(0);
    let bus = state.bus.clone();
    // Subscribe before the database replay snapshot: events committed in the
    // gap are present on the receiver and then deduplicated by sequence.
    let mut receiver = bus.subscribe();
    let stream = async_stream::stream! {
        let mut last = after;
        // Drain all durable pages, not only the first 500. Otherwise a client
        // reconnecting after a busy offline period skips the middle pages.
        loop {
            let batch=bus.replay(last,500);
            if batch.is_empty(){break;}
            let n=batch.len();
            for value in batch {
                let seq=value["seq"].as_u64().unwrap_or(0);
                if seq<=last{continue;}
                last=seq;
                yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
            }
            if n<500{break;}
        }
        // A second product process can write to the SAME SQLite journal
        // without notifying this process's in-memory broadcast channel.
        // A lightweight durable replay tick makes those writes visible to an
        // already connected browser, rather than only after SSE reconnect.
        let mut durable_tick=tokio::time::interval(Duration::from_secs(2));
        durable_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _=durable_tick.tick()=>{
                    loop {
                        let batch=bus.replay(last,500);
                        if batch.is_empty(){break;}
                        let n=batch.len();
                        for value in batch {
                            let seq=value["seq"].as_u64().unwrap_or(0);
                            if seq<=last{continue;}
                            last=seq;
                            yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
                        }
                        if n<500{break;}
                    }
                },
                received=receiver.recv()=>{
                    match received {
                        Ok(value)=>{
                            let seq=value["seq"].as_u64().unwrap_or(0);
                            if seq<=last{continue;}
                            last=seq;
                            yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
                        },
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{
                            // Resume from SQLite on the next tick. Bounded
                            // broadcast loss never becomes transcript loss.
                            continue;
                        },
                        Err(_)=>break,
                    }
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn audit_log(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 500);
    Json(json!(state.store.recent_audit(limit)))
}

pub async fn model_usage(State(state): State<AppState>) -> Json<Value> {
    Json(state.store.usage_summary())
}

pub async fn run_wakes(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("run_wake")))
}

pub async fn policies(State(state): State<AppState>) -> Json<Value> {
    Json(json!(crate::engine::policy::list(&state)))
}

/// Creates a consistent snapshot inside the data directory for a restore drill.
pub async fn create_backup(State(state): State<AppState>) -> axum::response::Response {
    use axum::response::IntoResponse;
    match crate::engine::maintenance::backup_now_with_remote(&state).await {
        Ok((path, remote)) => {
            let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            Json(json!({
                "ok": true,
                "path": path.to_string_lossy(),
                "bytes": bytes,
                "encrypted": true,
                "remote": remote.map(|report| json!({
                    "attempted": true,
                    "uploaded": report.uploaded,
                    "verified": report.verified,
                    "url": report.url,
                    "sha256": report.sha256,
                    "attempts": report.attempts,
                    "error": report.error,
                })),
            }))
            .into_response()
        }
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": error})),
        )
            .into_response(),
    }
}

// ----------------------------------------------------------------- translate

pub async fn model_route(State(state): State<AppState>) -> Json<Value> {
    let providers = state.store.all("provider");
    let provider = providers.iter().find(|provider| {
        provider
            .get("enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(true)
    });
    match provider {
        Some(provider) => Json(json!({
            "providerId": provider.get("id"),
            "model": provider.get("model").cloned().unwrap_or(json!("")),
            "kind": provider.get("kind"),
        })),
        None => Json(json!({ "providerId": "", "model": "" })),
    }
}

pub async fn capabilities(State(state): State<AppState>) -> Json<Value> {
    let tools = engine::tools::definitions()
        .into_iter()
        .filter_map(|definition| {
            let function = definition.get("function")?;
            Some(json!({
                "id": function.get("name")?,
                "namespace": "builtin",
                "name": function.get("name")?,
                "description": function.get("description"),
                "available": true,
            }))
        })
        .chain(
            state
                .store
                .all("capability")
                .into_iter()
                .map(|record| record),
        )
        .collect::<Vec<_>>();
    Json(json!({ "items": tools, "updatedAt": chrono::Utc::now().to_rfc3339() }))
}

pub async fn commands(State(state): State<AppState>) -> Json<Value> {
    let candidates = [
        state.config.web_dir.join("shared/commands.json"),
        state.config.web_dir.join("../shared/commands.json"),
        state.config.workspace.join("shared/commands.json"),
    ];
    for path in candidates {
        if let Ok(text) = crate::storage::bounded_file::read_utf8(&path, 1024 * 1024) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                return Json(value);
            }
        }
    }
    Json(json!([]))
}

pub async fn list_backups(State(state): State<AppState>) -> Json<Value> {
    let directory = state.config.data_dir.join("backups");
    let mut items = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    matches!(
                        path.extension().and_then(|value| value.to_str()),
                        Some("enc" | "sqlite")
                    )
                })
                .filter_map(|path| {
                    let name = path.file_name()?.to_str()?.to_string();
                    let metadata = std::fs::metadata(&path).ok()?;
                    let bytes = metadata.len();
                    let modified = metadata
                        .modified()
                        .ok()
                        .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339());
                    Some(json!({
                        "name": name,
                        "path": path.to_string_lossy(),
                        "bytes": bytes,
                        "encrypted": path.extension().and_then(|value| value.to_str()) == Some("enc"),
                        "modifiedAt": modified,
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| {
        b["modifiedAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["modifiedAt"].as_str().unwrap_or(""))
    });
    Json(json!(items))
}

pub async fn backup_keys(State(state): State<AppState>) -> Json<Value> {
    Json(crate::engine::backup_keys::status(&state))
}

pub async fn rotate_backup_keys(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !state.maintenance_mode() {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"error":"키 회전은 유지보수 모드에서만 가능합니다."})),
        )
            .into_response();
    }
    let keep_old = body["keepOld"].as_bool().unwrap_or(false);
    match crate::engine::backup_keys::rotate(&state, keep_old) {
        Ok(report) => Json(json!({
            "ok": true,
            "activeVersion": report.active_version,
            "previousVersion": report.previous_version,
            "reencrypted": report.reencrypted,
            "retainedVersions": report.retained_versions,
        }))
        .into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        )
            .into_response(),
    }
}

pub async fn secret_key(State(state): State<AppState>) -> Json<Value> {
    Json(crate::engine::vault_rotation::status(&state))
}

pub async fn rotate_secret_key(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !state.maintenance_mode() {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"error":"secret key 회전은 유지보수 모드에서만 가능합니다."})),
        )
            .into_response();
    }
    let keep_old = body["keepOld"].as_bool().unwrap_or(false);
    match crate::engine::vault_rotation::rotate(&state, keep_old) {
        Ok(report) => Json(json!({
            "ok": true,
            "secretsReencrypted": report.secrets_reencrypted,
            "legacyBackupsMigrated": report.legacy_backups_migrated,
            "previousKeys": report.previous_keys,
            "oldKeyRetained": report.old_key_file.is_some(),
        }))
        .into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error, "oldKeyRetained": true})),
        )
            .into_response(),
    }
}

pub async fn maintenance_status(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"maintenance": state.maintenance_mode()}))
}

pub async fn maintenance(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let enabled = body["enabled"].as_bool().unwrap_or(false);
    state.set_maintenance(enabled);
    Json(json!({"maintenance": enabled}))
}

/// Restores an encrypted backup while maintenance mode is explicit. The
/// restored data replaces the live database under the store mutex.
pub async fn restore_backup(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !state.maintenance_mode() {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"error":"유지보수 모드에서만 복구할 수 있습니다."})),
        )
            .into_response();
    }
    if body["confirm"].as_bool() != Some(true) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"confirm=true 가 필요합니다."})),
        )
            .into_response();
    }
    let Some(requested) = body["path"].as_str().filter(|value| !value.is_empty()) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"path 가 필요합니다."})),
        )
            .into_response();
    };
    let directory = state.config.data_dir.join("backups");
    let candidate = {
        let path = std::path::Path::new(requested);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            directory.join(path)
        }
    };
    let canonical_directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.clone());
    let Ok(canonical) = candidate.canonicalize() else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"error":"백업 파일을 찾을 수 없습니다."})),
        )
            .into_response();
    };
    if !canonical.starts_with(&canonical_directory)
        || canonical.extension().and_then(|value| value.to_str()) != Some("enc")
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"data/backups 안의 .enc 백업만 복구할 수 있습니다."})),
        )
            .into_response();
    }
    let restore_id = uuid::Uuid::new_v4();
    let container_path = directory.join(format!(".restore-container-{restore_id}.tmp"));
    let _container_cleanup = RemoveFileOnDrop(container_path.clone());
    let container_len = match crate::engine::backup_keys::decrypt_backup_file_to(
        &state, &canonical, &container_path,
    ) {
        Ok(length) => length,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error":"이 키로 백업을 복호화할 수 없습니다."})),
            )
                .into_response()
        }
    };
    let mut container = match std::fs::File::open(&container_path) {
        Ok(file) => file,
        Err(error) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string()})),
            )
                .into_response()
        }
    };
    let mut embedded_key = None;
    let mut database_offset = 0u64;
    if container_len >= 41 {
        let mut envelope = [0u8; 41];
        if container.read_exact(&mut envelope).is_err() {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error":"백업 데이터 헤더가 잘렸습니다."})),
            )
                .into_response();
        }
        if &envelope[..9] == b"DOMVAULT1" {
            let mut key = [0u8; 32];
            key.copy_from_slice(&envelope[9..]);
            embedded_key = Some(key);
            database_offset = 41;
        }
    }
    let database_path = directory.join(format!(".restore-database-{restore_id}.tmp"));
    let _database_cleanup = RemoveFileOnDrop(database_path.clone());
    let mut database_options = std::fs::OpenOptions::new();
    database_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        database_options.mode(0o600);
    }
    let mut database = match database_options.open(&database_path) {
        Ok(file) => file,
        Err(error) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
            )
                .into_response()
        }
    };
    if let Err(error) = container.seek(SeekFrom::Start(database_offset))
        .and_then(|_| std::io::copy(&mut container, &mut database).map(|_| ()))
        .and_then(|_| database.sync_all())
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
            .into_response();
    }
    drop(database);
    drop(container);
    let mut database_header = [0u8; 16];
    if std::fs::File::open(&database_path)
        .and_then(|mut file| file.read_exact(&mut database_header))
        .is_err()
        || &database_header != b"SQLite format 3\0"
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"복구 파일이 SQLite 백업 형식이 아닙니다."})),
        )
            .into_response();
    }
    let target_key = embedded_key.unwrap_or_else(|| state.vault.primary_key());
    // Verify every sealed secret in the snapshot is decryptable with the key
    // we would install. Abort before touching the live database/key otherwise.
    if let Err(error) = crate::engine::vault_rotation::verify_snapshot_secrets_file(
        &target_key, &database_path,
    ) {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({
                "error": format!("복구 대상의 secret key를 확인할 수 없습니다: {error}. 현재 상태를 유지합니다.")
            })),
        )
            .into_response();
    }
    // A malformed snapshot must not rotate the live secret key first and
    // then fail while replacing the active SQLite database.
    if let Err(error) = crate::storage::sqlite::Store::validate_restore_snapshot_file(
        &database_path,
    ) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("SQLite 백업 데이터가 손상되었습니다: {error}. 현재 상태를 유지합니다.")})),
        )
            .into_response();
    }
    if let Some(key) = embedded_key {
        if let Err(error) = state.vault.restore_primary(key) {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("secret key 복원 실패: {error}")})),
            )
                .into_response();
        }
    }
    if let Err(error) = state.store.replace_database_from_file(&database_path) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        )
            .into_response();
    }
    if let Err(error) = crate::migrations::run(&state) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("복구 후 마이그레이션 실패: {error}")})),
        )
            .into_response();
    }
    state.store.audit(
        "system.restored",
        json!({"path": canonical.to_string_lossy()}),
    );
    Json(json!({
        "ok": true,
        "executions": state.store.count("execution"),
    }))
    .into_response()
}
