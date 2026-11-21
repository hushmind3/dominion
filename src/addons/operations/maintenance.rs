//! Scheduled maintenance: bounded database backups.
use crate::state::AppState;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Cleans up plaintext SQLite and staged ciphertext on *every* early return.
/// A failed backup must not leave an unencrypted database in the backup directory.
struct TemporarySnapshot(PathBuf);

impl Drop for TemporarySnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Publish a fully flushed ciphertext file only after all bytes are written.
/// The staging file is on the same filesystem so rename is atomic.
fn write_sealed_snapshot(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let staged = destination.with_file_name(format!(
        ".dominion-sealed-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    let _cleanup = TemporarySnapshot(staged.clone());
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    std::fs::rename(&staged, destination).map_err(|error| error.to_string())
}

/// An offsite copy must not become visible to restore/rotation while incomplete.
fn copy_sealed_snapshot(source: &Path, destination: &Path) -> Result<(), String> {
    let staged = destination.with_file_name(format!(
        ".dominion-copy-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    let _cleanup = TemporarySnapshot(staged.clone());
    std::fs::copy(source, &staged).map_err(|error| error.to_string())?;
    // A read-only handle cannot flush on Windows: FlushFileBuffers needs write
    // access. Unix accepts the read-only open, Windows fails the offsite copy.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&staged)
        .and_then(|file| file.sync_all())
        .map_err(|error| error.to_string())?;
    std::fs::rename(&staged, destination).map_err(|error| error.to_string())
}

fn interval_seconds() -> u64 {
    std::env::var("DOMINION_BACKUP_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3_600)
}

fn keep_count() -> usize {
    std::env::var("DOMINION_BACKUP_KEEP")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1)
}

/// Removes the oldest snapshots beyond `keep`. Returns the removed paths.
pub fn prune_backups(directory: &Path, keep: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("sqlite" | "enc")
            ) && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with("dominion-"))
        })
        .collect::<Vec<_>>();
    files.sort();
    if files.len() <= keep {
        return Vec::new();
    }
    let remove_count = files.len() - keep;
    files
        .into_iter()
        .take(remove_count)
        .filter(|path| std::fs::remove_file(path).is_ok())
        .collect()
}

fn offsite_directory() -> Option<PathBuf> {
    std::env::var("DOMINION_BACKUP_OFFSITE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Creates an encrypted snapshot. The plaintext SQLite file exists only as a
/// short-lived temp during VACUUM INTO and is removed before returning.
pub fn create_snapshot(state: &AppState) -> Result<PathBuf, String> {
    let directory = state.config.data_dir.join("backups");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%6f");
    let temp = directory.join(format!(".backup-{}.tmp", uuid::Uuid::new_v4()));
    let plaintext_cleanup = TemporarySnapshot(temp.clone());
    state.store.backup_to(&temp)?;
    let bytes = std::fs::read(&temp).map_err(|error| error.to_string())?;
    // The plaintext is no longer needed on disk, including if key loading fails.
    drop(plaintext_cleanup);
    let ring = crate::engine::backup_keys::KeyRing::load_or_create(&state.config.data_dir)?;
    // The snapshot carries the vault key that its provider secrets were sealed
    // with, so restoring an older snapshot remains decryptable after rotation.
    let mut container = b"DOMVAULT1".to_vec();
    container.extend_from_slice(&state.vault.primary_key());
    container.extend_from_slice(&bytes);
    let sealed = ring.encrypt(&container)?;
    // A UUID prevents concurrent/manual backups from replacing one another.
    let path = directory.join(format!(
        "dominion-{stamp}-{}.sqlite.enc",
        uuid::Uuid::new_v4()
    ));
    write_sealed_snapshot(&path, &sealed)?;
    if let Some(offsite) = offsite_directory() {
        std::fs::create_dir_all(&offsite).map_err(|error| error.to_string())?;
        copy_sealed_snapshot(&path, &offsite.join(path.file_name().unwrap_or_default()))?;
        prune_backups(&offsite, keep_count());
    }
    prune_backups(&directory, keep_count());
    state.store.audit(
        "backup.encrypted",
        serde_json::json!({
            "path": path.to_string_lossy(),
            "bytes": sealed.len(),
            "offsite": offsite_directory().map(|dir| dir.to_string_lossy().to_string()),
        }),
    );
    Ok(path)
}

/// Scheduled path: create locally, then best-effort remote transfer. A remote
/// failure never deletes the local snapshot.
async fn create_snapshot_off_thread(state: &AppState) -> Result<PathBuf, String> {
    // SQLite VACUUM and filesystem encryption are blocking work. Do not stall
    // the same Tokio workers serving chat, approvals, cancellation and SDK APIs.
    let snapshot_state = state.clone();
    tokio::task::spawn_blocking(move || create_snapshot(&snapshot_state))
        .await
        .map_err(|error| format!("backup worker failed: {error}"))?
}

pub async fn backup_now(state: &AppState) -> Result<PathBuf, String> {
    let path = create_snapshot_off_thread(state).await?;
    let _ = crate::engine::remote_backup::upload_if_configured(state, &path).await;
    Ok(path)
}

/// API path: report the remote transfer outcome to the caller.
pub async fn backup_now_with_remote(
    state: &AppState,
) -> Result<(PathBuf, Option<crate::engine::remote_backup::RemoteReport>), String> {
    let path = create_snapshot_off_thread(state).await?;
    let report = crate::engine::remote_backup::upload_if_configured(state, &path).await;
    Ok((path, report))
}

/// Starts the background backup loop. `DOMINION_BACKUP_INTERVAL_SECS=0`
/// disables it; the default is hourly with five retained snapshots.
pub fn start(state: AppState) {
    let seconds = interval_seconds();
    if seconds == 0 {
        return;
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(seconds));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if state.maintenance_mode() {
                continue;
            }
            match backup_now(&state).await {
                Ok(path) => state.store.audit(
                    "backup.created",
                    serde_json::json!({"path": path.to_string_lossy()}),
                ),
                Err(error) => state.store.audit(
                    "backup.failed",
                    serde_json::json!({"error": error}),
                ),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-backup-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn encrypted_backup_round_trips_and_reopens_as_sqlite() {
        let (state, dir) = state();
        state.store.put(
            "provider",
            &serde_json::json!({"id":"p1","name":"encrypted backup","baseUrl":"http://x/v1"}),
        );
        let path = create_snapshot(&state).unwrap();
        assert_eq!(path.extension().and_then(|value| value.to_str()), Some("enc"));
        let sealed = std::fs::read(&path).unwrap();
        assert!(!sealed.starts_with(b"SQLite format 3"));
        let plain =
            crate::engine::backup_keys::decrypt_backup(&state, &sealed).expect("decrypt");
        assert!(plain.starts_with(b"DOMVAULT1"));
        let database = &plain[41..];
        assert!(database.starts_with(b"SQLite format 3"));
        let restored_path = dir.join("restored.sqlite");
        std::fs::write(&restored_path, database).unwrap();
        let restored = Store::open(&restored_path).unwrap();
        assert_eq!(
            restored.get("provider", "p1").unwrap()["name"],
            "encrypted backup"
        );
        // The plaintext temp used for VACUUM INTO is removed.
        let leftovers = std::fs::read_dir(dir.join("backups"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    == Some("tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn consecutive_snapshots_never_overwrite_one_another() {
        let (state, dir) = state();
        let first = create_snapshot(&state).unwrap();
        let second = create_snapshot(&state).unwrap();
        assert_ne!(first, second);
        assert!(first.is_file() && second.is_file());
        for path in [&first, &second] {
            let sealed = std::fs::read(path).unwrap();
            assert!(crate::engine::backup_keys::decrypt_backup(&state, &sealed).is_some());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plaintext_is_removed_when_key_initialization_fails() {
        let (state, dir) = state();
        // A regular file where the key directory belongs forces a failure
        // after SQLite has written its plaintext VACUUM snapshot.
        std::fs::write(dir.join("backup-keys"), b"block key creation").unwrap();
        assert!(create_snapshot(&state).is_err());
        let leftovers = std::fs::read_dir(dir.join("backups"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("tmp"))
            .count();
        assert_eq!(leftovers, 0, "plaintext backup leaked after an error");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn retention_keeps_the_newest_snapshots_only() {
        let dir = std::env::temp_dir().join(format!("dominion-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for index in 1..=5 {
            std::fs::write(dir.join(format!("dominion-2026110{index}-000000.sqlite")), b"x")
                .unwrap();
        }
        std::fs::write(dir.join("other.sqlite"), b"x").unwrap();
        let removed = prune_backups(&dir, 2);
        assert_eq!(removed.len(), 3);
        let remaining = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.starts_with("dominion-"))
            })
            .count();
        assert_eq!(remaining, 2);
        assert!(dir.join("other.sqlite").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
