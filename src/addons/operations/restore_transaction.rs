//! Crash-recoverable backup restore. An intent marker is durable before either
//! the database or the vault key changes. Startup restores the previous pair
//! if a process dies at any point before successful migration/commit.
use crate::state::AppState;
use std::{fs, io::{self, Write}, path::{Path,PathBuf}};

const MARKER: &str = ".restore-in-progress";
const OLD_DB: &str = ".restore-rollback.sqlite";
const OLD_KEY: &str = ".restore-rollback.key";

fn sync_file(path:&Path)->Result<(),String>{
    fs::File::open(path).and_then(|file|file.sync_all()).map_err(|e|e.to_string())
}
fn sync_parent(path:&Path)->Result<(),String>{
    #[cfg(unix)]
    {
        fs::File::open(path).and_then(|dir|dir.sync_all()).map_err(|e|e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn private(path:&Path)->Result<(),String>{
    #[cfg(unix)]{
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path,fs::Permissions::from_mode(0o600)).map_err(|e|e.to_string())?;
    }
    #[cfg(not(unix))]
    let _=path;
    Ok(())
}
pub fn pending(dir:&Path)->bool{dir.join(MARKER).exists()}
fn paths(dir:&Path)->(PathBuf,PathBuf,PathBuf){
    (dir.join(MARKER),dir.join(OLD_DB),dir.join(OLD_KEY))
}
fn cleanup(dir:&Path){
    let (_,database,key)=paths(dir);
    let _=fs::remove_file(database);
    let _=fs::remove_file(key);
}
fn mark_committed(dir:&Path)->Result<(),String>{
    let (marker,_,_)=paths(dir);
    fs::remove_file(marker).map_err(|error|format!("restore commit marker: {error}"))?;
    // If a power failure replays the marker, recovery still chooses the old,
    // consistent pair. Both outcomes are valid.
    let _=sync_parent(dir);
    cleanup(dir);
    Ok(())
}
fn stage_copy(from:&Path,to:&Path)->Result<(),String>{
    fs::copy(from,to).map_err(|error|error.to_string())?;
    private(to)?;
    sync_file(to)?;
    Ok(())
}

/// Run before Store::open/Vault::open/migrations. A present marker means the
/// old database/key pair must win, even if the new database was already moved.
pub fn recover_before_open(db_path:&Path,key_path:&Path)->Result<bool,String>{
    let dir=db_path.parent().ok_or("restore database has no parent")?;
    let (marker,old_db,old_key)=paths(dir);
    if !marker.exists(){ return Ok(false); }
    if !old_db.is_file() || !old_key.is_file(){
        return Err("Incomplete restore rollback files: startup halted to protect secrets".into());
    }
    let stage_db=dir.join(".restore-recovery.sqlite.tmp");
    stage_copy(&old_db,&stage_db)?;
    // A crashed WAL belongs to the abandoned snapshot, never to the old pair.
    for suffix in ["-wal","-shm"] {
        let auxiliary=PathBuf::from(format!("{}{}",db_path.display(),suffix));
        if auxiliary.exists(){ fs::remove_file(&auxiliary).map_err(|e|e.to_string())?; }
    }
    fs::rename(&stage_db,db_path).map_err(|error|format!("restore database recovery: {error}"))?;
    let stage_key=dir.join(".restore-recovery.key.tmp");
    stage_copy(&old_key,&stage_key)?;
    fs::rename(&stage_key,key_path).map_err(|error|format!("restore key recovery: {error}"))?;
    sync_parent(dir)?;
    mark_committed(dir)?;
    Ok(true)
}

pub struct Transaction {
    directory: PathBuf,
    old_key: [u8;32],
}

impl Transaction {
    fn begin(state:&AppState)->Result<Self,String>{
        let directory=state.config.data_dir.clone();
        let (marker,old_db,old_key)=paths(&directory);
        if marker.exists(){
            return Err("Restore recovery is pending. Restart before another restore".into());
        }
        // Snapshot includes WAL content. Hold no secret in the record store;
        // backup key and DB are chmod 0600 and removed after settlement.
        let _=fs::remove_file(&old_db);
        let _=fs::remove_file(&old_key);
        state.store.backup_to(&old_db)?;
        private(&old_db)?;
        sync_file(&old_db)?;
        state.vault.write_key_file(&old_key).map_err(|e|e.to_string())?;
        private(&old_key)?;
        sync_file(&old_key)?;
        let mut file=fs::OpenOptions::new().write(true).create_new(true)
            .open(&marker).map_err(|e|format!("restore intent: {e}"))?;
        file.write_all(b"DOMINION_RESTORE_ROLLBACK_V1\n").map_err(|e|e.to_string())?;
        file.sync_all().map_err(|e|e.to_string())?;
        sync_parent(&directory)?;
        Ok(Self{directory,old_key:state.vault.primary_key()})
    }
    fn rollback(self,state:&AppState)->Result<(),String>{
        let (_,old_db,_)=paths(&self.directory);
        let bytes=fs::read(&old_db).map_err(|e|format!("restore rollback database read: {e}"))?;
        // This path must not depend on the potentially blocked restore.tmp
        // path, otherwise the injected failure prevents rollback as well.
        state.store.replace_database_rollback(&bytes)
            .map_err(|e|format!("restore rollback database: {e}"))?;
        state.vault.restore_primary(self.old_key)
            .map_err(|e|format!("restore rollback key: {e}"))?;
        mark_committed(&self.directory)
    }
    fn commit(self)->Result<(),String>{mark_committed(&self.directory)}
}

/// Untrusted backup is verified before the first live mutation. On a failed
/// database/key swap or migration, restore the original *pair*, not one half.
pub fn apply<F>(state:&AppState, bytes:&[u8], target_key:[u8;32], migrate:F)->Result<(),String>
where F:FnOnce(&AppState)->Result<(),String>{
    crate::engine::vault_rotation::verify_snapshot_secrets(
        &target_key,bytes,&state.config.data_dir
    )?;
    let transaction=Transaction::begin(state)?;
    let attempt=(||{
        state.store.replace_database(bytes)?;
        state.vault.restore_primary(target_key).map_err(|e|e.to_string())?;
        migrate(state)?;
        Ok::<(),String>(())
    })();
    match attempt {
        Ok(())=>transaction.commit(),
        Err(error)=>match transaction.rollback(state){
            Ok(())=>Err(error),
            Err(rollback)=>Err(format!(
                "{error}; rollback incomplete: {rollback}; maintenance and startup recovery required"
            )),
        },
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    fn fixture()->(AppState,Vec<u8>,[u8;32],PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-restore-atomic-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.save_secret("fixture-token","old-secret");
        state.store.put("fixture",&serde_json::json!({"id":"test","state":"old"}));
        let new_key=[0x42u8;32];
        let snapshot=dir.join("fixture.sqlite");
        let alternate=dir.join("alternate.key");
        let vault=Vault::open(&alternate);
        vault.restore_primary(new_key).unwrap();
        let restore_store=Store::open(&dir.join("fixture-source.sqlite")).unwrap();
        restore_store.set_secret("fixture-token",&vault.seal("new-secret"));
        restore_store.put("fixture",&serde_json::json!({"id":"test","state":"new"}));
        restore_store.backup_to(&snapshot).unwrap();
        drop(restore_store);
        (state,fs::read(snapshot).unwrap(),new_key,dir)
    }
    fn old_still_readable(state:&AppState){
        assert_eq!(state.read_secret("fixture-token").as_deref(),Some("old-secret"));
        assert_eq!(state.store.get("fixture","test").unwrap()["state"],"old");
        let vault=Vault::open(&state.config.key_path());
        assert_eq!(vault.open_secret(&state.store.get_secret("fixture-token").unwrap()).as_deref(),
            Some("old-secret"));
    }
    #[test]
    fn blocked_restore_staging_does_not_change_key_db_or_restart_state(){
        let (state,bytes,key,dir)=fixture();
        fs::create_dir_all(dir.join("dominion.restore.tmp")).unwrap();
        assert!(apply(&state,&bytes,key,|_|Ok(())).is_err());
        old_still_readable(&state);
        drop(state);
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        assert!(!recover_before_open(&config.db_path(),&config.key_path()).unwrap());
        let reopened=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        old_still_readable(&reopened);
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn migration_error_rolls_back_the_complete_key_db_pair(){
        let (state,bytes,key,dir)=fixture();
        assert!(apply(&state,&bytes,key,|_|Err("migration fixture failure".into())).is_err());
        old_still_readable(&state);
        assert!(!dir.join(MARKER).exists());
        drop(state);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn crash_after_database_swap_recovers_old_database_and_key_at_startup(){
        let (state,bytes,_key,dir)=fixture();
        let transaction=Transaction::begin(&state).unwrap();
        state.store.replace_database(&bytes).unwrap();
        // Simulate process death before the vault key changes. No rollback
        // is invoked; the durable marker must drive restart recovery.
        drop(transaction);
        drop(state);
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        assert!(recover_before_open(&config.db_path(),&config.key_path()).unwrap());
        let reopened=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        old_still_readable(&reopened);
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn successful_restore_restarts_with_matching_key_and_database(){
        let (state,bytes,key,dir)=fixture();
        apply(&state,&bytes,key,|_|Ok(())).unwrap();
        assert_eq!(state.read_secret("fixture-token").as_deref(),Some("new-secret"));
        assert_eq!(state.store.get("fixture","test").unwrap()["state"],"new");
        drop(state);
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        assert!(!recover_before_open(&config.db_path(),&config.key_path()).unwrap());
        let reopened=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        assert_eq!(reopened.read_secret("fixture-token").as_deref(),Some("new-secret"));
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }
}
