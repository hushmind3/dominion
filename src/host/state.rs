use crate::config::Config;
use crate::db::Store;
use crate::events::Bus;
use crate::vault::Vault;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{oneshot,Notify};

type FileMutations = Arc<Mutex<HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>>>;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub store: Store,
    pub bus: Bus,
    /// Advisory wakeup for job workers. SQLite remains the durable authority.
    pub job_wakeup: Arc<Notify>,
    /// Wakes active turns after same-process cancellation or epoch changes.
    pub cancel_wakeup: Arc<Notify>,
    /// Reusable provider HTTP transports keyed by per-request timeout in seconds.
    /// Shared by all speakers and jobs in this process to retain connection pooling.
    provider_http_clients: Arc<Mutex<HashMap<u64, reqwest::Client>>>,
    /// Account APIs carry owner data and credentials; never follow redirects
    /// that could forward a mutation body to another host.
    account_http_clients: Arc<Mutex<HashMap<u64, reqwest::Client>>>,
    /// Optional adapter transport state is separate from the agent runtime.
    pub integrations: crate::integrations::IntegrationState,
    pub vault: Arc<Vault>,
    /// Pending tool approvals: approval id -> waiter.
    pub approvals: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Sessions the user asked to stop. The engine checks between rounds/tools.
    pub cancelled: Arc<Mutex<HashSet<String>>>,
    /// Sessions currently executing. Prevents two turns racing on one history.
    pub active: Arc<Mutex<HashSet<String>>>,
    /// Coordinates checked and unchecked writes to the same canonical path.
    pub file_mutations: FileMutations,
    pub principle_learning: Arc<Mutex<()>>,
    pub policy_settlement: Arc<Mutex<()>>,
    /// Identity of this engine process, used to own durable run leases.
    pub instance_id: String,
    /// Explicit maintenance flag. While true, non-GET writes are rejected.
    pub maintenance: Arc<std::sync::atomic::AtomicBool>,
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
            job_wakeup: Arc::new(Notify::new()),
            cancel_wakeup: Arc::new(Notify::new()),
            provider_http_clients: Arc::new(Mutex::new(HashMap::new())),
            account_http_clients: Arc::new(Mutex::new(HashMap::new())),
            integrations: crate::integrations::IntegrationState::default(),
            vault: Arc::new(vault),
            approvals: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            active: Arc::new(Mutex::new(HashSet::new())),
            file_mutations: Arc::new(Mutex::new(HashMap::new())),
            principle_learning: Arc::new(Mutex::new(())),
            policy_settlement: Arc::new(Mutex::new(())),
            instance_id,
            maintenance: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Build a provider client once per timeout policy. Clients carry pooled
    /// sockets and TLS configuration; cloning a handle is cheap. Never hold
    /// the mutex while awaiting a network operation. Credentials belong to
    /// individual requests, not these process-wide transport handles.
    pub fn provider_http_client(&self, timeout_secs: u64) -> Result<reqwest::Client, String> {
        if let Some(client) = self.provider_http_clients.lock().unwrap()
            .get(&timeout_secs).cloned() {
            return Ok(client);
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build().map_err(|error|error.to_string())?;
        let mut cache = self.provider_http_clients.lock().unwrap();
        Ok(cache.entry(timeout_secs).or_insert(client).clone())
    }

    /// Reusable account connector transport with redirects disabled. OAuth
    /// credentials and owner-authored writes must remain on their fixed API
    /// origin even when a remote server responds with 307/308.
    pub fn account_http_client(&self, timeout_secs: u64) -> Result<reqwest::Client, String> {
        if let Some(client) = self.account_http_clients.lock().unwrap()
            .get(&timeout_secs).cloned() {
            return Ok(client);
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build().map_err(|error|error.to_string())?;
        let mut cache = self.account_http_clients.lock().unwrap();
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
        // Wake active turns once the cancellation epoch has been persisted.
        // A bounded poll remains for changes written by another process.
        self.cancel_wakeup.notify_waiters();
        // Cancellation survives a restart and cannot be cleared for old queued
        // work merely because the user starts a fresh foreground turn.
        for job in self.store.all("job") {
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
        for mut goal in self.store.all("goal") {
            if goal["sessionId"].as_str() == Some(session_id)
                && matches!(goal["status"].as_str(), Some("queued" | "running"))
            {
                goal["status"] = serde_json::json!("cancelled");
                goal["phase"] = serde_json::json!("cancelled");
                goal["finishedAt"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
                if let Some(saved) = self.store.put("goal", &goal) {
                    self.bus.record("goal", &saved);
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
        self.cancel_wakeup.notify_waiters();
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
    fn provider_clients_are_reused_by_timeout_and_across_state_clones() {
        let (state, dir) = temp_state();
        assert!(state.provider_http_clients.lock().unwrap().is_empty());
        state.provider_http_client(60).unwrap();
        state.clone().provider_http_client(60).unwrap();
        assert_eq!(state.provider_http_clients.lock().unwrap().len(), 1);
        state.provider_http_client(300).unwrap();
        assert_eq!(state.provider_http_clients.lock().unwrap().len(), 2);
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
