use crate::engine;
use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use std::collections::HashMap;
use serde_json::{json,Value};
use std::time::Duration;
use std::convert::Infallible;
use futures_util::Stream;

pub async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "dominion",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "engine": "rust",
        "instanceId":std::env::var("DOMINION_ENGINE_INSTANCE").ok(),
        "commit": crate::build_identity::running().commit,
        "branch": crate::build_identity::running().branch,
        "dirty": crate::build_identity::running().dirty,
        "identitySource": crate::build_identity::running().source,
    }))
}

/// Read-only identity for the isolated benchmark runner. It never exposes
/// secrets and is only authoritative when the server was started with
/// DOMINION_BENCHMARK=1.
pub async fn benchmark_identity(State(state): State<AppState>) -> Json<Value> {
    let canonical = |path: &std::path::Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string()
    };
    Json(json!({
        "instanceId": std::env::var("DOMINION_ENGINE_INSTANCE").ok(),
        "benchmark": std::env::var("DOMINION_BENCHMARK").as_deref() == Ok("1"),
        "dataRoot": canonical(&state.config.data_dir),
        "workspace": canonical(&state.config.workspace),
        "build": env!("CARGO_PKG_VERSION"),
        "commit": crate::build_identity::running().commit,
        "branch": crate::build_identity::running().branch,
        "dirty": crate::build_identity::running().dirty,
        "identitySource": crate::build_identity::running().source,
        "pid": std::process::id(),
    }))
}

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let jobs = state.store.all("job");
    let running = jobs
        .iter()
        .filter(|job| {
            matches!(
                job.get("status").and_then(|value| value.as_str()),
                Some("queued") | Some("running")
            )
        })
        .count();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "engine": "rust",
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

/// Drain the bounded SQLite event log in pages. A one-page replay silently
/// skipped older records when a reconnect had >500 pending events.
fn durable_replay(bus: &crate::events::Bus, after: u64) -> Vec<Value> {
    const PAGE: usize = 500;
    const MAX_BATCH: usize = 3_000; // > durable 2000 (+ pruning interval)
    let mut cursor = after;
    let mut result = Vec::new();
    while result.len() < MAX_BATCH {
        let page = bus.replay(cursor, PAGE);
        let count = page.len();
        if count == 0 {
            break;
        }
        for item in page {
            let seq = item["seq"].as_u64().unwrap_or(0);
            if seq <= cursor {
                continue;
            }
            if seq > cursor.saturating_add(1) {
                // The durable log is intentionally bounded. Never imply that
                // an expired cursor received a complete event history.
                result.push(gap_event(cursor, seq));
            }
            cursor = seq;
            result.push(item);
        }
        if count < PAGE {
            break;
        }
    }
    result
}

fn gap_event(after: u64, first_available: u64) -> Value {
    json!({
        "type": "event_gap",
        "seq": first_available.saturating_sub(1),
        "resumedAfter": after,
        "firstAvailable": first_available,
        "missed": first_available.saturating_sub(after.saturating_add(1)),
        "resyncRequired": true
    })
}

/// EventSource reconnects keep their original URL, but supply the newest
/// event ID in Last-Event-ID. The reconnect watermark must win over an
/// initial `?after=` query or every disconnect replays old events again.
fn event_cursor(params: &HashMap<String, String>, headers: &HeaderMap) -> u64 {
    headers.get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| params.get("after").and_then(|value| value.parse::<u64>().ok()))
        .unwrap_or(0)
}

pub async fn events(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let after = event_cursor(&params, &headers);
    let bus = state.bus.clone();
    // Subscribe before replay so writes during page reads enter the receiver.
    let mut receiver = bus.subscribe();
    let stream = async_stream::stream! {
        let mut last = after;
        for value in durable_replay(&bus, last) {
            let seq = value["seq"].as_u64().unwrap_or(0);
            if seq <= last { continue; }
            last = seq;
            yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
        }
        loop {
            match receiver.recv().await {
                Ok(value) => {
                    let seq = value["seq"].as_u64().unwrap_or(0);
                    if seq <= last { continue; }
                    if seq > last.saturating_add(1) {
                        // Recover the entire durable interval, not only the
                        // first 500 records. A late live value is deduplicated.
                        for previous in durable_replay(&bus, last) {
                            let previous_seq = previous["seq"].as_u64().unwrap_or(0);
                            if previous_seq <= last { continue; }
                            last = previous_seq;
                            yield Ok(Event::default().id(last.to_string()).data(previous.to_string()));
                        }
                    }
                    if seq > last.saturating_add(1) {
                        let gap = gap_event(last, seq);
                        last = seq - 1;
                        yield Ok(Event::default().id(last.to_string()).data(gap.to_string()));
                    }
                    if seq > last {
                        last = seq;
                        yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    for value in durable_replay(&bus, last) {
                        let seq = value["seq"].as_u64().unwrap_or(0);
                        if seq <= last { continue; }
                        last = seq;
                        yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
                    }
                }
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod event_replay_tests {
    use super::*;
    #[test]
    fn eventsource_reconnect_uses_latest_header_not_original_query() {
        let mut params = HashMap::new();
        params.insert("after".to_string(), "12".to_string());
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "45".parse().unwrap());
        assert_eq!(event_cursor(&params, &headers), 45);
        headers.insert("last-event-id", "0".parse().unwrap());
        assert_eq!(event_cursor(&params, &headers), 0);
    }

    #[test]
    fn first_connection_or_invalid_header_uses_query_cursor() {
        let mut params = HashMap::new();
        params.insert("after".to_string(), "12".to_string());
        let mut headers = HeaderMap::new();
        assert_eq!(event_cursor(&params, &headers), 12);
        headers.insert("last-event-id", "not-a-sequence".parse().unwrap());
        assert_eq!(event_cursor(&params, &headers), 12);
        params.clear();
        assert_eq!(event_cursor(&params, &headers), 0);
    }

    #[test]
    fn multi_page_reconnect_preserves_all_retained_records() {
        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let bus = crate::events::Bus::new(store);
        for n in 0..1100 { bus.emit(json!({"type":"tick","index":n})); }
        let events = durable_replay(&bus, 0);
        assert_eq!(events.len(), 1100);
        assert_eq!(events.first().unwrap()["seq"], 1);
        assert_eq!(events.last().unwrap()["seq"], 1100);
        assert!(events.iter().all(|event| event["type"] != "event_gap"));
    }
    #[test]
    fn expired_cursor_is_explicit_gap_not_silent_data_loss() {
        let store = crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let bus = crate::events::Bus::new(store);
        for n in 0..2600 { bus.emit(json!({"type":"tick","index":n})); }
        let events = durable_replay(&bus, 0);
        assert_eq!(events[0]["type"], "event_gap");
        assert_eq!(events[0]["resyncRequired"], true);
        assert_eq!(
            events[0]["seq"].as_u64().unwrap() + 1,
            events[1]["seq"].as_u64().unwrap()
        );
        assert_eq!(events.last().unwrap()["seq"], 2600);
    }
}

pub async fn audit_log(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.recent_audit(200)))
}

pub async fn model_usage(State(state): State<AppState>) -> Json<Value> {
    Json(state.store.usage_summary())
}

pub async fn evaluation_summary(State(state):State<AppState>)->Json<Value>{
    Json(crate::engine::evaluation::summary(&state))
}

pub async fn run_wakes(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("run_wake")))
}

pub async fn experiment_requests(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("experiment_request")))
}

pub async fn policies(State(state): State<AppState>) -> Json<Value> {
    Json(json!(crate::engine::policy::list(&state)))
}

pub async fn policy_searches(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("policy_search")))
}

pub async fn policy_patches(State(state): State<AppState>) -> Json<Value> {
    Json(json!(crate::engine::policy_patch::list(&state)))
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
        if let Ok(text) = std::fs::read_to_string(&path) {
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
    let sealed = match std::fs::read(&canonical) {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string()})),
            )
                .into_response()
        }
    };
    let Some(decoded) = crate::engine::backup_keys::decrypt_backup(&state, &sealed) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"이 키로 백업을 복호화할 수 없습니다."})),
        )
            .into_response();
    };
    let (bytes, embedded_key) = if decoded.len() > 41 && decoded.starts_with(b"DOMVAULT1") {
        let mut key = [0u8; 32];
        key.copy_from_slice(&decoded[9..41]);
        (decoded[41..].to_vec(), Some(key))
    } else {
        (decoded, None)
    };
    let target_key = embedded_key.unwrap_or_else(|| state.vault.primary_key());
    // Verify every sealed secret in the snapshot is decryptable with the key
    // we would install. Abort before touching the live database/key otherwise.
    if let Err(error) = crate::engine::vault_rotation::verify_snapshot_secrets(
        &target_key,
        &bytes,
        &state.config.data_dir,
    ) {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({
                "error": format!("복구 대상의 secret key를 확인할 수 없습니다: {error}. 현재 상태를 유지합니다.")
            })),
        )
            .into_response();
    }
    // DB and vault key form one crash-recoverable restore transaction.
    // A failure in DB staging, key installation or schema migration must
    // never advertise a successful restore of only one half of the pair.
    if let Err(error) = crate::engine::restore_transaction::restore(
        &state, &bytes, embedded_key,
    ) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error,"restored":false})),
        ).into_response();
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
