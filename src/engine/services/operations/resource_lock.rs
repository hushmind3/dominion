//! Native tool resource isolation across sessions/agents in this process.
//! Independent workspaces can progress in parallel. All possible writes to
//! one workspace (including shell commands) share its lock, so a shell build
//! cannot race a simultaneous multi-file patch and produce mixed evidence.
//! This complements the more granular file mutation guards.
use serde_json::Value;
use std::{collections::HashMap,path::Path,sync::{Arc,Mutex,OnceLock,Weak}};
use tokio::sync::{Mutex as AsyncMutex,OwnedMutexGuard};
type Guard=OwnedMutexGuard<()>;
fn registry()->&'static Mutex<HashMap<String,Weak<AsyncMutex<()>>>>{
    static REGISTRY:OnceLock<Mutex<HashMap<String,Weak<AsyncMutex<()>>>>>=OnceLock::new();
    REGISTRY.get_or_init(||Mutex::new(HashMap::new()))
}
fn key(workspace:&Path,name:&str,args:&Value)->Option<String>{
    let canonical=workspace.canonicalize().unwrap_or_else(|_|workspace.to_path_buf());
    let prefix=canonical.to_string_lossy();
    match name{
        "workspace_write"|"workspace_replace"|"workspace_replace_range"|
        "workspace_apply_patch"|"shell_exec"|"terminal_exec"|"external_harness_run" =>
            Some(format!("workspace:{prefix}")),
        "file_write"=>{
            let target=args["path"].as_str()?;
            let path=Path::new(target);
            // Unknown/outside absolute paths serialize conservatively.
            let parent=path.parent().unwrap_or(Path::new("."));
            let root=parent.canonicalize().unwrap_or_else(|_|parent.to_path_buf());
            Some(format!("external:{}",root.to_string_lossy()))
        },
        "browser_control"=>Some("global:browser".to_string()),
        "github_api"=>Some("global:github-write-potential".to_string()),
        // Delegate tools spawn nested workers: do not retain a lock through
        // child execution, or recursive delegation would deadlock.
        _=>None,
    }
}
/// A SQLite lease fences a second engine process using the same DB.
/// Delegations retain no workspace lock, preventing parent/child deadlock.
pub async fn acquire_durable(
    state:&crate::state::AppState,workspace:&Path,name:&str,args:&Value,
    session_id:&str,
)->Result<Option<crate::engine::lease::LeaseGuard>,String>{
    let Some(resource)=key(workspace,name,args) else{return Ok(None);};
    let fingerprint=crate::engine::memory::stable_id(&[&resource]);
    let scope=format!("resource-writer:{fingerprint}");
    for _ in 0..100{
        if state.is_cancelled(session_id){
            return Err("resource writer was cancelled before acquisition".into());
        }
        if let Some(lease)=crate::engine::lease::LeaseGuard::acquire(state,&scope){
            return Ok(Some(lease));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("workspace resource is held by another writer; no tool was dispatched".into())
}

pub async fn acquire(workspace:&Path,name:&str,args:&Value)->Option<Guard>{
    let key=key(workspace,name,args)?;
    let lock={
        let mut map=registry().lock().ok()?;
        map.retain(|_,weak|weak.strong_count()!=0);
        if let Some(existing)=map.get(&key).and_then(Weak::upgrade){
            existing
        }else{
            let mutex=Arc::new(AsyncMutex::new(()));
            map.insert(key,Arc::downgrade(&mutex));
            mutex
        }
    };
    Some(lock.lock_owned().await)
}
#[cfg(test)]
mod tests{
    use super::*;
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    #[tokio::test]
    async fn same_workspace_mutations_serialize_but_other_workspaces_overlap(){
        let root=std::env::temp_dir().join(format!("lock-{}",uuid::Uuid::new_v4()));
        let a=root.join("a");let b=root.join("b");
        std::fs::create_dir_all(&a).unwrap();std::fs::create_dir_all(&b).unwrap();
        let first=acquire(&a,"workspace_apply_patch",&Value::Null).await.unwrap();
        let started=Arc::new(AtomicUsize::new(0));
        let count=started.clone();let a2=a.clone();
        let blocked=tokio::spawn(async move{
            count.fetch_add(1,Ordering::SeqCst);
            acquire(&a2,"shell_exec",&Value::Null).await.unwrap()
        });
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert_eq!(started.load(Ordering::SeqCst),1);
        assert!(!blocked.is_finished());
        let other=acquire(&b,"terminal_exec",&Value::Null).await.unwrap();
        drop(other);drop(first);
        let guard=tokio::time::timeout(std::time::Duration::from_secs(1),blocked).await
            .unwrap().unwrap();
        drop(guard);
        assert!(acquire(&a,"workspace_read",&Value::Null).await.is_none());
        assert!(acquire(&a,"delegate_task",&Value::Null).await.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn distinct_engine_connections_share_one_durable_workspace_writer_lease(){
        let dir=std::env::temp_dir().join(format!("lease-writer-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let one=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let two=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let first=acquire_durable(&one,&dir,"workspace_write",&Value::Null,"a")
            .await.unwrap().unwrap();
        assert!(tokio::time::timeout(std::time::Duration::from_millis(250),
            acquire_durable(&two,&dir,"shell_exec",&Value::Null,"b")
        ).await.is_err());
        drop(first);
        let second=acquire_durable(&two,&dir,"shell_exec",&Value::Null,"b")
            .await.unwrap().unwrap();
        drop(second);
        assert!(acquire_durable(&two,&dir,"workspace_read",&Value::Null,"b")
            .await.unwrap().is_none());
        drop(one);drop(two);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn resource_keys_match_workspace_scope_without_crossing_into_browser(){
        let ws=std::path::Path::new("/tmp/dominion");
        assert_eq!(key(ws,"workspace_write",&Value::Null),
            key(ws,"shell_exec",&Value::Null));
        assert_eq!(key(ws,"external_harness_run",&Value::Null),
            key(ws,"workspace_write",&Value::Null));
        assert_ne!(key(ws,"workspace_write",&Value::Null),
            key(ws,"browser_control",&Value::Null));
        assert!(key(ws,"web_search",&Value::Null).is_none());
    }
}
