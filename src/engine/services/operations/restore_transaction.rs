//! Crash-recoverable local backup restore.
//! A durable rollback marker is written before changing DB or vault key.
//! On restart the previous pair is recovered before Store/Vault are opened.
use crate::{config::Config,state::AppState};
use std::{fs,io::Write,path::Path,sync::{Mutex,OnceLock}};

const DB_ROLLBACK:&str=".restore-rollback.sqlite";
const KEY_ROLLBACK:&str=".restore-rollback.key";
const MARKER:&str=".restore-pending";

fn restore_lock()->&'static Mutex<()>{
    static LOCK:OnceLock<Mutex<()>>=OnceLock::new();
    LOCK.get_or_init(||Mutex::new(()))
}
fn snapshot_key(path:&Path)->Result<[u8;32],String>{
    let raw=fs::read(path).map_err(|e|format!("restore old key unreadable: {e}"))?;
    let key:[u8;32]=raw.try_into()
        .map_err(|_|"restore rollback key length invalid".to_string())?;
    Ok(key)
}
fn verify_rollback_database(path:&Path)->Result<(),String>{
    let conn=rusqlite::Connection::open_with_flags(path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e|e.to_string())?;
    let answer:String=conn.query_row("PRAGMA quick_check",[],|row|row.get(0))
        .map_err(|e|e.to_string())?;
    if answer!="ok"{return Err(format!("restore rollback integrity: {answer}"));}
    Ok(())
}
fn sync(path:&Path)->Result<(),String>{
    fs::File::open(path).and_then(|file|file.sync_all())
        .map_err(|e|format!("restore durability sync: {e}"))
}
fn finish(dir:&Path)->Result<(),String>{
    // Marker first, rollback snapshots second. A crash may leave orphans
    // but can never trigger rollback with missing recovery material.
    fs::remove_file(dir.join(MARKER)).map_err(|e|e.to_string())?;
    sync(dir)?;
    let _=fs::remove_file(dir.join(DB_ROLLBACK));
    let _=fs::remove_file(dir.join(KEY_ROLLBACK));
    Ok(())
}
/// Called at SDK/server startup before a live SQLite or key handle exists.
/// A partial restore is always rolled back, never accepted as success.
pub fn recover_pending(config:&Config)->Result<(),String>{
    let dir=&config.data_dir;
    if !dir.join(MARKER).exists(){return Ok(());}
    let old_db=dir.join(DB_ROLLBACK);
    let old_key=dir.join(KEY_ROLLBACK);
    verify_rollback_database(&old_db)?;
    let _=snapshot_key(&old_key)?;
    let staging=dir.join(".restore-recovery.sqlite");
    fs::copy(&old_db,&staging).map_err(|e|e.to_string())?;
    sync(&staging)?;
    fs::rename(&staging,config.db_path()).map_err(|e|e.to_string())?;
    for suffix in ["-wal","-shm"]{
        let _=fs::remove_file(format!("{}{}",config.db_path().display(),suffix));
    }
    let key_staging=dir.join(".restore-recovery.key");
    fs::copy(&old_key,&key_staging).map_err(|e|e.to_string())?;
    #[cfg(unix)]{
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_staging,fs::Permissions::from_mode(0o600))
            .map_err(|e|e.to_string())?;
    }
    sync(&key_staging)?;
    fs::rename(&key_staging,config.key_path()).map_err(|e|e.to_string())?;
    sync(dir)?;
    finish(dir)
}
/// Restore a verified backup. Old DB and key remain available until DB
/// swap, vault installation and migrations all succeed. Rollback is guarded
/// by the same marker used after power loss or process termination.
pub fn restore(state:&AppState,bytes:&[u8],key:Option<[u8;32]>)->Result<(),String>{
    let _guard=restore_lock().lock().map_err(|e|e.to_string())?;
    let dir=&state.config.data_dir;
    if dir.join(MARKER).exists(){
        return Err("interrupted restore needs recovery on restart".into());
    }
    let old_db=dir.join(DB_ROLLBACK);
    let old_key=dir.join(KEY_ROLLBACK);
    state.store.backup_to(&old_db)?;
    state.vault.write_key_file(&old_key)
        .map_err(|e|format!("restore rollback key backup: {e}"))?;
    sync(&old_db)?;
    sync(&old_key)?;
    let mut marker=fs::OpenOptions::new().write(true).create_new(true)
        .open(dir.join(MARKER)).map_err(|e|e.to_string())?;
    marker.write_all(b"DOMINION_RESTORE_PENDING_V1\n").map_err(|e|e.to_string())?;
    marker.sync_all().map_err(|e|e.to_string())?;
    sync(dir)?;
    let result=(||->Result<(),String>{
        // Stage SQLite before changing the key. Early target-file failures
        // leave the original key and the original DB intact.
        state.store.replace_database(bytes)?;
        if let Some(key)=key{
            state.vault.restore_primary(key).map_err(|e|e.to_string())?;
        }
        crate::migrations::run(state)?;
        Ok(())
    })();
    if let Err(error)=result{
        let rollback=(||->Result<(),String>{
            let original=fs::read(&old_db).map_err(|e|e.to_string())?;
            state.store.replace_database(&original)?;
            state.vault.restore_primary(snapshot_key(&old_key)?)
                .map_err(|e|e.to_string())?;
            finish(dir)
        })();
        return Err(match rollback{
            Ok(())=>format!("restore failed; original DB/key recovered: {error}"),
            Err(recovery)=>format!(
                "restore failed: {error}; rollback incomplete: {recovery}; \
                 restart the engine to recover before using this data directory"),
        });
    }
    finish(dir)
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{db::Store,vault::Vault};
    use serde_json::json;
    fn fixture()->(AppState,Config){
        let root=std::env::temp_dir().join(format!("dominion-restore-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        (AppState::new(config.clone(),store,vault),config)
    }
    #[test]
    fn blocked_db_stage_preserves_live_key_database_and_restart(){
        let (state,config)=fixture();
        let original_key=state.vault.primary_key();
        let secret=state.vault.seal("private");
        state.store.set_secret("provider:one",&secret);
        state.store.put("session",&json!({"id":"before","title":"old"}));
        let source=config.data_dir.join("original-snapshot.sqlite");
        state.store.backup_to(&source).unwrap();
        let bytes=fs::read(source).unwrap();
        let stage=config.db_path().with_extension("restore.tmp");
        fs::create_dir_all(&stage).unwrap();
        let err=restore(&state,&bytes,Some([41;32])).unwrap_err();
        assert!(err.contains("restore failed"),"{err}");
        assert_eq!(state.vault.primary_key(),original_key);
        assert_eq!(state.vault.open_secret(&secret).as_deref(),Some("private"));
        assert_eq!(state.store.get("session","before").unwrap()["title"],"old");
        drop(state);
        recover_pending(&config).unwrap();
        let restarted=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        assert_eq!(vault.primary_key(),original_key);
        assert_eq!(vault.open_secret(&restarted.get_secret("provider:one").unwrap())
            .as_deref(),Some("private"));
        let _=fs::remove_dir_all(config.data_dir);
    }
    #[test]
    fn startup_rolls_back_crash_with_old_db_and_new_vault_key(){
        let (state,config)=fixture();
        let old_key=state.vault.primary_key();
        let sealed=state.vault.seal("secret-old");
        state.store.set_secret("provider:one",&sealed);
        state.store.put("session",&json!({"id":"before","title":"old"}));
        let old_db=config.data_dir.join(DB_ROLLBACK);
        state.store.backup_to(&old_db).unwrap();
        state.vault.write_key_file(&config.data_dir.join(KEY_ROLLBACK)).unwrap();
        fs::write(config.data_dir.join(MARKER),b"DOMINION_RESTORE_PENDING_V1\n").unwrap();
        state.store.put("session",&json!({"id":"after","title":"new"}));
        state.vault.restore_primary([57;32]).unwrap();
        drop(state);
        recover_pending(&config).unwrap();
        let restarted=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        assert_eq!(vault.primary_key(),old_key);
        assert_eq!(vault.open_secret(&restarted.get_secret("provider:one").unwrap())
            .as_deref(),Some("secret-old"));
        assert!(restarted.get("session","after").is_none());
        assert_eq!(restarted.get("session","before").unwrap()["title"],"old");
        assert!(!config.data_dir.join(MARKER).exists());
        let _=fs::remove_dir_all(config.data_dir);
    }
}
