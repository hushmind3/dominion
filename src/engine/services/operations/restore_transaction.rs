//! Crash-recoverable local backup restore.
//! A durable rollback marker is written before changing DB or vault key.
//! On restart the previous pair is recovered before Store/Vault are opened.
use crate::{config::Config,state::AppState};
use std::{fs,io::Write,path::Path,sync::{Mutex,OnceLock}};
use sha2::{Digest, Sha256};

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
/// A DOMVAULT1 snapshot contains SQLite and the vault key, but not the
/// original attachment directory. Refuse a restore that would resurrect dead
/// attachment IDs and falsely report a usable transcript. Legacy records
/// without SHA256 still require exact byte length; new records verify SHA256.
/// The verification SQLite file is temporary and never mutates the live DB.
fn verify_snapshot_attachments(state:&AppState, db_bytes:&[u8]) -> Result<(),String> {
    if !db_bytes.starts_with(b"SQLite format 3\0") {
        return Err("복구 대상이 올바른 SQLite 데이터베이스가 아닙니다.".into());
    }
    let probe=state.config.data_dir.join(format!(
        ".verify-attachments-{}.sqlite",uuid::Uuid::new_v4()
    ));
    // The decrypted staging SQLite holds private chat transcripts. Create
    // it exclusive and owner-readable before writing any plaintext bytes.
    let written=(||->Result<(),String>{
        let mut file=fs::OpenOptions::new().write(true).create_new(true)
            .open(&probe).map_err(|e|format!("복구 검증 파일 생성 실패: {e}"))?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&probe,fs::Permissions::from_mode(0o600))
                .map_err(|e|format!("복구 검증 파일 접근 권한 오류: {e}"))?;
        }
        file.write_all(db_bytes).map_err(|e|format!("복구 검증 기록 오류: {e}"))?;
        Ok(())
    })();
    if let Err(error)=written {
        let _=fs::remove_file(&probe);
        return Err(error);
    }
    let result=(|| -> Result<(),String> {
        let conn=rusqlite::Connection::open_with_flags(&probe,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e|format!("복구 스냅샷 열기 실패: {e}"))?;
        let mut stmt=conn.prepare(
            "SELECT id,data FROM records WHERE kind='attachment'"
        ).map_err(|e|format!("복구 첨부 목록 조회 실패: {e}"))?;
        let mut rows=stmt.query([]).map_err(|e|e.to_string())?;
        let root=state.config.data_dir.canonicalize()
            .map_err(|_|"첨부 검증용 데이터 경로를 열 수 없습니다.")?;
        let mut folder:Option<std::path::PathBuf>=None;
        while let Some(row)=rows.next().map_err(|e|e.to_string())? {
            let id:String=row.get(0).map_err(|e|e.to_string())?;
            let metadata:String=row.get(1).map_err(|e|e.to_string())?;
            uuid::Uuid::parse_str(&id)
                .map_err(|_|"복구 대상에 잘못된 첨부 ID가 있습니다.".to_string())?;
            let record:serde_json::Value=serde_json::from_str(&metadata)
                .map_err(|_|"복구 첨부 기록이 손상되었습니다.".to_string())?;
            let size=record["size"].as_u64()
                .filter(|size|*size<=crate::engine::attachments::MAX_FILE as u64)
                .ok_or("복구 첨부 파일 크기가 잘못되었습니다.")?;
            if folder.is_none() {
                let path=root.join("attachments").canonicalize()
                    .map_err(|_|"복구 대상의 원본 첨부파일 저장 폴더가 없습니다.")?;
                if !path.starts_with(&root) {
                    return Err("첨부 저장 폴더가 복구 경계를 벗어났습니다.".into());
                }
                folder=Some(path);
            }
            let directory=folder.as_ref().ok_or("첨부 폴더가 없습니다.")?;
            let path=directory.join(format!("{id}.bin")).canonicalize()
                .map_err(|_|format!("복구 대상의 첨부 원본 {id}이 현재 저장소에 없습니다."))?;
            if path.parent()!=Some(directory.as_path()) {
                return Err("복구 첨부 원본이 저장 폴더를 벗어났습니다.".into());
            }
            let actual=fs::metadata(&path)
                .map_err(|_|format!("첨부 {id}의 크기를 확인할 수 없습니다."))?;
            if !actual.is_file() || actual.len()!=size {
                return Err(format!("복구 첨부 {id}의 크기가 원본 기록과 다릅니다."));
            }
            if let Some(expected)=record["sha256"].as_str() {
                let bytes=fs::read(&path)
                    .map_err(|_|format!("복구 첨부 {id}을 읽을 수 없습니다."))?;
                if format!("{:x}",Sha256::digest(&bytes))!=expected {
                    return Err(format!("복구 첨부 {id}의 SHA-256 검증에 실패했습니다."));
                }
            }
        }
        Ok(())
    })();
    let _=fs::remove_file(&probe);
    result
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
    // Validate file references before any rollback marker or database swap.
    // Existing encrypted backups never contained the original uploads.
    verify_snapshot_attachments(state,bytes)?;
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
    fn restore_refuses_missing_or_corrupt_files_without_touching_live_database() {
        use base64::{engine::general_purpose::STANDARD,Engine};
        let (state,config)=fixture();
        state.store.put("session",&json!({"id":"session-a","title":"uploaded proof"}));
        let file=crate::engine::attachments::upload(&state,&json!({
            "sessionId":"session-a","name":"evidence.txt",
            "data":STANDARD.encode("original bytes")
        })).unwrap();
        let id=file["id"].as_str().unwrap();
        let stored=config.data_dir.join("attachments").join(format!("{id}.bin"));
        let backup=config.data_dir.join("snapshot.sqlite");
        state.store.backup_to(&backup).unwrap();
        let data=fs::read(&backup).unwrap();
        state.store.put("session",&json!({"id":"live","title":"must survive"}));
        fs::remove_file(&stored).unwrap();
        let error=restore(&state,&data,None).unwrap_err();
        assert!(error.contains("첨부"),"{error}");
        assert!(state.store.get("session","live").is_some(),
            "a rejected backup must not replace the running SQLite database");
        assert!(!config.data_dir.join(MARKER).exists());
        fs::write(&stored,b"modified bytes").unwrap();
        let error=restore(&state,&data,None).unwrap_err();
        assert!(error.contains("SHA-256"),"{error}");
        assert!(state.store.get("session","live").is_some());
        fs::write(&stored,b"original bytes").unwrap();
        restore(&state,&data,None).unwrap();
        assert!(state.store.get("session","live").is_none(),
            "fully valid file-backed backup can restore its earlier DB state");
        assert_eq!(crate::engine::attachments::read(&state,id).unwrap().1,
            b"original bytes");
        assert!(!config.data_dir.join(MARKER).exists());
        let _=fs::remove_dir_all(config.data_dir);
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
