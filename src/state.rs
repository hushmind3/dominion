use crate::config::Config;
use crate::db::Store;
use crate::events::Bus;
use crate::vault::Vault;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::oneshot;

type FileMutations = Arc<Mutex<HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>>>;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub store: Store,
    pub bus: Bus,
    /// Shared HTTP transport pools, keyed by request timeout. API credentials
    /// remain request-local and are never stored in this cache.
    provider_http_clients: Arc<Mutex<HashMap<u64, reqwest::Client>>>,
    pub vault: Arc<Vault>,
    /// Pending tool approvals: approval id -> waiter.
    pub approvals: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Sessions the user asked to stop. The engine checks between rounds/tools.
    pub cancelled: Arc<Mutex<HashSet<String>>>,
    /// Sessions currently executing. Prevents two turns racing on one history.
    pub active: Arc<Mutex<HashSet<String>>>,
    /// Automatic context compaction, keyed by session. The value is the
    /// admission time: a running or recently failed attempt blocks a second
    /// model call, while a successful one clears the entry.
    pub auto_compact: Arc<Mutex<HashMap<String, std::time::Instant>>>,
    /// Coordinates checked and unchecked writes to the same canonical path.
    pub file_mutations: FileMutations,
    pub principle_learning: Arc<Mutex<()>>,
    pub policy_settlement: Arc<Mutex<()>>,
    /// Identity of this engine process, used to own durable run leases.
    pub instance_id: String,
    /// Explicit maintenance flag. While true, non-GET writes are rejected.
    pub maintenance: Arc<std::sync::atomic::AtomicBool>,
    /// Serializes backup restore transactions even if multiple HTTP requests race.
    pub restore_guard: Arc<tokio::sync::Mutex<()>>,
    pub managed_apps: Arc<tokio::sync::Mutex<HashMap<String,tokio::process::Child>>>,
}

impl AppState {
    pub fn new(config: Config, store: Store, vault: Vault) -> Self {
        let bus = Bus::new(store.clone());
        let instance_id = std::env::var("DOMINION_ENGINE_INSTANCE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self {
            config,
            store,
            bus,
            provider_http_clients: Arc::new(Mutex::new(HashMap::new())),
            vault: Arc::new(vault),
            approvals: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            active: Arc::new(Mutex::new(HashSet::new())),
            auto_compact: Arc::new(Mutex::new(HashMap::new())),
            file_mutations: Arc::new(Mutex::new(HashMap::new())),
            principle_learning: Arc::new(Mutex::new(())),
            policy_settlement: Arc::new(Mutex::new(())),
            instance_id,
            maintenance: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            restore_guard: Arc::new(tokio::sync::Mutex::new(())),
            managed_apps: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Reuse TLS connections across ordinary chats, rooms and SDK turns.
    /// The state lock is released before any await, and the cache contains no
    /// bearer token, provider URL or other user-specific credentials.
    pub fn provider_http_client(&self, timeout_secs: u64) -> Result<reqwest::Client, String> {
        if let Some(client) = self.provider_http_clients.lock().unwrap()
            .get(&timeout_secs).cloned() {
            return Ok(client);
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|error| error.to_string())?;
        let mut cache = self.provider_http_clients.lock().unwrap();
        Ok(cache.entry(timeout_secs).or_insert(client).clone())
    }

    pub fn set_maintenance(&self, enabled: bool) {
        self.maintenance
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
        self.store.audit(
            "system.maintenance",
            serde_json::json!({"enabled": enabled}),
        );
    }

    pub fn maintenance_mode(&self) -> bool {
        self.maintenance.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn save_secret(&self, name: &str, plaintext: &str) {
        let sealed = self.vault.seal(plaintext);
        self.store.set_secret(name, &sealed);
    }

    pub fn read_secret(&self, name: &str) -> Option<String> {
        let sealed = self.store.get_secret(name)?;
        self.vault.open_secret(&sealed)
    }

    pub fn cancel_session(&self, session_id: &str) {
        let newly_cancelled=!self.is_cancelled(session_id);
        self.cancelled
            .lock()
            .unwrap()
            .insert(session_id.to_string());
        if self.store.cancel_session_epoch(session_id).is_none() {
            self.store.audit(
                "session.cancel_persistence_failed",
                serde_json::json!({"sessionId":session_id}),
            );
        }
        // Cancellation survives a restart. Inspect only live jobs through the
        // status index, not years of completed jobs and historical payloads.
        let active_jobs = match self.store.active_jobs() {
            Ok(jobs) => jobs,
            Err(error) => {
                self.store.audit(
                    "session.cancel_active_jobs_read_failed",
                    serde_json::json!({"sessionId":session_id,"error":error}),
                );
                Vec::new()
            }
        };
        for job in active_jobs {
            let matches = [
                "/sessionId",
                "/input/sessionId",
                "/goalState/sessionId",
                "/roomState/sessionId",
                "/workflowState/sessionId",
            ]
            .iter()
            .any(|path| job.pointer(path).and_then(serde_json::Value::as_str) == Some(session_id));
            if matches && matches!(job["status"].as_str(), Some("queued" | "running")) {
                if let Some(id) = job["id"].as_str() {
                    if let Some(saved) = self.store.patch_if("job",id,"status",&job["status"],&serde_json::json!({
                        "status":"cancelled","cancelRequested":true,"finishedAt":chrono::Utc::now().to_rfc3339(),"error":"사용자 요청으로 실행이 중단되었습니다."
                    })) { self.bus.record("job",&saved); }
                }
            }
        }
        if newly_cancelled {
            let children = match self.store.delegated_child_sessions(session_id) {
                Ok(children) => children,
                Err(error) => {
                    self.store.audit(
                        "session.cancel_children_read_failed",
                        serde_json::json!({"sessionId":session_id,"error":error}),
                    );
                    Vec::new()
                }
            };
            for child in children {
                if let Some(id) = child["id"].as_str() {
                    self.cancel_session(id);
                    crate::engine::execution_budget::close_unstarted_child(self, id);
                }
            }
        }
    }

    pub fn clear_cancel(&self, session_id: &str) {
        self.resume_session_if_generation(session_id, self.cancellation_generation(session_id));
    }

    pub fn cancellation_generation(&self, session_id: &str) -> u64 {
        self.store
            .get("session_cancel", session_id)
            .and_then(|record| record["generation"].as_u64())
            .unwrap_or(0)
    }

    pub fn resume_session_if_generation(&self, session_id: &str, generation: u64) -> bool {
        if !self.store.resume_session_epoch(session_id, generation) {
            return false;
        }
        self.cancelled.lock().unwrap().remove(session_id);
        true
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.cancelled.lock().unwrap().contains(session_id)
            || self
                .store
                .get("session_cancel", session_id)
                .is_some_and(|record| record["cancelled"].as_bool().unwrap_or(true))
    }

    /// Returns false when the session already has a running turn.
    pub fn begin_session(&self, session_id: &str) -> bool {
        self.active.lock().unwrap().insert(session_id.to_string())
    }

    pub fn end_session(&self, session_id: &str) {
        self.active.lock().unwrap().remove(session_id);
    }

    pub fn session_guard(&self, session_id: &str) -> SessionGuard {
        SessionGuard {
            state: self.clone(),
            id: session_id.to_string(),
        }
    }

    /// Admits one automatic compaction per session. A previous attempt still
    /// inside `cooldown` (running, or failed) keeps the slot taken.
    pub fn begin_auto_compact(&self, session_id: &str, cooldown: std::time::Duration) -> bool {
        let mut guard = self.auto_compact.lock().unwrap();
        if guard
            .get(session_id)
            .is_some_and(|started| started.elapsed() < cooldown)
        {
            return false;
        }
        guard.insert(session_id.to_string(), std::time::Instant::now());
        true
    }

    /// Success frees the slot; a failure keeps the timestamp so the provider
    /// is not retried on every following turn.
    pub fn finish_auto_compact(&self, session_id: &str, succeeded: bool) {
        if succeeded {
            self.auto_compact.lock().unwrap().remove(session_id);
        }
    }
}

/// Releases the session lock even when the turn returns early via `?`.
pub struct SessionGuard {
    state: AppState,
    id: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.state.end_session(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&cfg.db_path()).unwrap();
        let vault = Vault::open(&cfg.key_path());
        (AppState::new(cfg, store, vault), dir)
    }

    #[test]
    fn cancel_session_targets_only_active_matching_jobs_and_delegated_children() {
        let (state, dir) = temp_state();
        state.store.put("session", &serde_json::json!({
            "id":"child","delegated":true,"parentSessionId":"root"
        }));
        state.store.put("session", &serde_json::json!({
            "id":"unrelated","delegated":true,"parentSessionId":"other"
        }));
        for (id, session_id, status) in [
            ("active-root", "root", "queued"),
            ("completed-root", "root", "completed"),
            ("active-child", "child", "running"),
            ("active-unrelated", "other", "queued"),
        ] {
            state.store.put("job", &serde_json::json!({
                "id":id,"sessionId":session_id,"status":status,
                "queuedAt":"2026-11-18T00:00:00Z"
            }));
        }
        state.cancel_session("root");
        assert_eq!(state.store.get("job","active-root").unwrap()["status"], "cancelled");
        assert_eq!(state.store.get("job","active-child").unwrap()["status"], "cancelled");
        assert_eq!(state.store.get("job","completed-root").unwrap()["status"], "completed");
        assert_eq!(state.store.get("job","active-unrelated").unwrap()["status"], "queued");
        assert!(state.is_cancelled("root"));
        assert!(state.is_cancelled("child"));
        assert!(!state.is_cancelled("other"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn provider_transport_pool_is_shared_by_clones_and_split_by_timeout() {
        let (state, dir) = temp_state();
        let cloned = state.clone();
        assert!(state.provider_http_client(60).is_ok());
        assert!(cloned.provider_http_client(60).is_ok());
        assert_eq!(state.provider_http_clients.lock().unwrap().len(), 1);
        assert!(state.provider_http_client(300).is_ok());
        assert_eq!(cloned.provider_http_clients.lock().unwrap().len(), 2);
        assert!(Arc::ptr_eq(&state.provider_http_clients, &cloned.provider_http_clients));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn session_lock_rejects_a_second_turn() {
        let (state, dir) = temp_state();
        assert!(state.begin_session("s1"));
        assert!(!state.begin_session("s1"));
        state.end_session("s1");
        assert!(state.begin_session("s1"));
        {
            let _guard = state.session_guard("s1");
            assert!(!state.begin_session("s1"));
        }
        assert!(state.begin_session("s1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancellation_survives_restart_and_old_jobs_stay_cancelled() {
        let (state, dir) = temp_state();
        state.store.put("job",&serde_json::json!({"id":"old","kind":"prompt","status":"queued","input":{"sessionId":"s"}}));
        state.store.put(
            "job",
            &serde_json::json!({"id":"running","kind":"room","status":"running","sessionId":"s"}),
        );
        state.cancel_session("s");
        let restarted = AppState::new(
            state.config.clone(),
            state.store.clone(),
            Vault::open(&state.config.key_path()),
        );
        assert!(restarted.is_cancelled("s"));
        restarted.clear_cancel("s");
        assert!(!restarted.is_cancelled("s"));
        assert_eq!(
            restarted.store.get("job", "old").unwrap()["status"],
            "cancelled"
        );
        assert!(
            crate::engine::jobs::JobContext::new(restarted.clone(), "running".into())
                .checkpoint(serde_json::json!({"status":"completed"}))
                .is_err()
        );
        assert_eq!(
            restarted.store.get("job", "running").unwrap()["status"],
            "cancelled"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resuming_a_new_epoch_cannot_revive_an_old_worker() {
        let (state, dir) = temp_state();
        let initial = state.cancellation_generation("room:r");
        assert!(state.resume_session_if_generation("room:r", initial));
        state.cancel_session("room:r");
        let stopped = state.cancellation_generation("room:r");
        assert!(stopped > initial);
        assert!(!state.resume_session_if_generation("room:r", initial));
        assert!(state.is_cancelled("room:r"));
        assert!(state.resume_session_if_generation("room:r", stopped));
        assert!(!state.is_cancelled("room:r"));
        assert_ne!(state.cancellation_generation("room:r"), initial);
        let _ = std::fs::remove_dir_all(dir);
    }
}
