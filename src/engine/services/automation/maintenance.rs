//! Scheduled maintenance: bounded database backups.
use crate::state::AppState;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

pub fn offsite_directory() -> Option<PathBuf> {
    std::env::var("DOMINION_BACKUP_OFFSITE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Drop every plaintext VACUUM INTO result, including paths that fail
/// key-ring loading, encryption, offsite copy or final disk writes.
struct PlaintextSnapshot(std::path::PathBuf);
impl Drop for PlaintextSnapshot {
    fn drop(&mut self) {
        let _=std::fs::remove_file(&self.0);
    }
}

fn write_private_backup(path:&Path,bytes:&[u8])->Result<(),String>{
    let mut options=std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]{
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file=options.open(path).map_err(|error|error.to_string())?;
    if let Err(error)=file.write_all(bytes).and_then(|()|file.sync_all()){
        drop(file);
        let _=std::fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

/// Reclaim plaintext VACUUM outputs stranded by a power failure. Only files
/// with our exact generated prefix/suffix inside the private staging folder
/// and older than a full day are eligible. Active huge snapshots are safe.
fn prune_crashed_plaintext(directory:&Path)->usize {
    let Ok(entries)=std::fs::read_dir(directory) else{return 0};
    let mut recovered=0;
    for entry in entries.flatten(){
        let path=entry.path();
        let Some(name)=path.file_name().and_then(|name|name.to_str()) else{continue};
        let Some(id)=name.strip_prefix("snapshot-")
            .and_then(|value|value.strip_suffix(".sqlite")) else{continue};
        if uuid::Uuid::parse_str(id).is_err(){continue}
        let Ok(metadata)=std::fs::symlink_metadata(&path) else{continue};
        if !metadata.file_type().is_file() { continue }
        if !metadata.modified().ok().and_then(|time|time.elapsed().ok())
            .is_some_and(|age|age>Duration::from_secs(24*60*60)) { continue }
        if std::fs::remove_file(&path).is_ok(){recovered+=1}
    }
    recovered
}

/// Creates an encrypted snapshot. Plaintext SQLite is always staged inside a
/// private owner-only directory and removed on every success/error path.
pub fn create_snapshot(state: &AppState) -> Result<PathBuf, String> {
    let _keyring_lock=crate::engine::backup_keys::lock_keyring()?;
    let directory = state.config.data_dir.join("backups");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    // SQLite's VACUUM INTO demands a nonexistent destination, so enforce
    // owner-only access on its PARENT before creating any plaintext bytes.
    let private=directory.join(".plaintext-staging");
    std::fs::create_dir_all(&private).map_err(|e|e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&private,
            std::fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?;
    }
    let reclaimed=prune_crashed_plaintext(&private);
    if reclaimed>0 {
        state.store.audit("backup.plaintext_crash_cleanup",
            serde_json::json!({"count":reclaimed}));
    }
    let temp = private.join(format!("snapshot-{}.sqlite", uuid::Uuid::new_v4()));
    let cleanup=PlaintextSnapshot(temp.clone());
    state.store.backup_to(&temp)?;
    let bytes = std::fs::read(&temp).map_err(|error| error.to_string())?;
    // Plaintext is no longer needed once read; erase it before key lookup or
    // encryption can fail or a slow offsite transfer begins.
    drop(cleanup);
    let ring = crate::engine::backup_keys::KeyRing::load_or_create(&state.config.data_dir)?;
    // The snapshot carries the vault key that its provider secrets were sealed
    // with, so restoring an older snapshot remains decryptable after rotation.
    let mut container = b"DOMVAULT1".to_vec();
    container.extend_from_slice(&state.vault.primary_key());
    container.extend_from_slice(&bytes);
    let sealed = ring.encrypt(&container)?;
    let path = directory.join(format!(
        "dominion-{stamp}-{}.sqlite.enc",uuid::Uuid::new_v4().simple()
    ));
    write_private_backup(&path,&sealed)?;

    if let Some(offsite) = offsite_directory() {
        std::fs::create_dir_all(&offsite).map_err(|error| error.to_string())?;
        let name=path.file_name().and_then(|name|name.to_str()).unwrap_or("backup.enc");
        let destination=offsite.join(name);
        let temporary=offsite.join(format!(".{name}.tmp-{}",uuid::Uuid::new_v4()));
        write_private_backup(&temporary,&sealed)?;
        if let Err(error)=std::fs::rename(&temporary,&destination){
            let _=std::fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        std::fs::File::open(&offsite).and_then(|dir|dir.sync_all())
            .map_err(|error|error.to_string())?;
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
pub async fn backup_now(state: &AppState) -> Result<PathBuf, String> {
    let path = create_snapshot(state)?;
    let _ = crate::engine::remote_backup::upload_if_configured(state, &path).await;
    Ok(path)
}

/// API path: report the remote transfer outcome to the caller.
pub async fn backup_now_with_remote(
    state: &AppState,
) -> Result<(PathBuf, Option<crate::engine::remote_backup::RemoteReport>), String> {
    let path = create_snapshot(state)?;
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
            // Reuse the existing maintenance heartbeat to reclaim stale
            // unpublished Chat forks. No additional daemon or schedule.
            crate::api::sessions::recover_stale_pending_forks_at(
                &state, chrono::Utc::now());
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
        #[cfg(unix)]{
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode()&0o777,0o600);
            assert_eq!(std::fs::metadata(dir.join("backups/.plaintext-staging"))
                .unwrap().permissions().mode()&0o777,0o700);
        }
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
    fn failed_encryption_never_leaves_world_readable_plaintext_snapshots() {
        let (state,dir)=state();
        state.store.put("session",&serde_json::json!({
            "id":"private-chat","title":"do not expose plaintext"
        }));
        // A regular file deliberately blocks creation of the key-ring folder
        // after the SQLite VACUUM snapshot was already created.
        std::fs::write(dir.join("backup-keys"),b"blocked").unwrap();
        assert!(create_snapshot(&state).is_err());
        let staging=dir.join("backups").join(".plaintext-staging");
        let remaining=std::fs::read_dir(&staging).unwrap().count();
        assert_eq!(remaining,0,"failed encrypt must erase temporary SQLite");
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&staging).unwrap().permissions().mode()&0o777,
                0o700,"staging directory must be inaccessible to other users");
        }
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn crash_plaintext_cleanup_only_removes_aged_owned_snapshot_files() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-backup-stage-test-{}",uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let stale=dir.join(format!("snapshot-{}.sqlite",uuid::Uuid::new_v4()));
        let recent=dir.join(format!("snapshot-{}.sqlite",uuid::Uuid::new_v4()));
        let unrelated=dir.join("snapshot-user-owned.sqlite");
        for path in [&stale,&recent,&unrelated]{
            std::fs::write(path,b"plaintext").unwrap();
        }
        let old=std::time::SystemTime::now()-Duration::from_secs(48*60*60);
        for path in [&stale,&unrelated]{
            let file=std::fs::OpenOptions::new().write(true).open(path).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(old)).unwrap();
        }
        assert_eq!(prune_crashed_plaintext(&dir),1);
        assert!(!stale.exists());
        assert!(recent.exists(),"active snapshot is never deleted");
        assert!(unrelated.exists(),"foreign files are not treated as temp data");
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rapid_backup_requests_never_overwrite_a_previous_encrypted_snapshot() {
        let (state,dir)=state();
        let first=create_snapshot(&state).unwrap();
        let second=create_snapshot(&state).unwrap();
        assert_ne!(first,second,"repeated backups must use collision-resistant names");
        assert!(first.is_file()&&second.is_file());
        let private=dir.join("backups").join(".plaintext-staging");
        assert_eq!(std::fs::read_dir(private).unwrap().count(),0);
        let _=std::fs::remove_dir_all(dir);
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
