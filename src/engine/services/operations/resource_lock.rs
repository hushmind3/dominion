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
        "shell_exec"|"terminal_exec"
            if dominion_agent_kernel::scheduling::inert_shell_command(args)=>None,
        "workspace_write"|"workspace_replace"|"workspace_replace_range"|
        "workspace_apply_patch"|"team_remote_apply"|"shell_exec"|"terminal_exec"|"external_harness_run" =>
            Some(format!("workspace:{prefix}")),
        "file_write"=>{
            // Absolute file_write is an alias for workspace_write whenever
            // its destination resolves inside this workspace. Previously it
            // used an unrelated parent-directory lock, permitting a shell
            // command and file_write to modify the SAME source concurrently.
            let Some(target)=args["path"].as_str() else {
                return Some("global:unresolved-file-write".to_string());
            };
            let path=Path::new(target);
            let parent=path.parent().unwrap_or(Path::new("."));
            match parent.canonicalize(){
                Ok(root) if root.starts_with(&canonical)=>Some(format!("workspace:{prefix}")),
                Ok(root)=>Some(format!("external:{}",root.to_string_lossy())),
                // Missing or unresolvable parent directories must not fall
                // through without a writer lock.
                Err(_)=>Some("global:unresolved-file-write".to_string()),
            }
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
    use serde_json::json;
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
    async fn inert_parallel_shells_skip_writer_lease_without_exposing_shell_mutations(){
        let dir=std::env::temp_dir().join(format!("readonly-process-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let writer=acquire(&dir,"workspace_write",&Value::Null).await.unwrap();
        for command in ["sleep 2.0","sleep 0.1","date","pwd","whoami","uname"]{
            let args=json!({"command":command});
            assert!(key(&dir,"shell_exec",&args).is_none(),"{command}");
            assert!(acquire(&dir,"shell_exec",&args).await.is_none());
            assert!(acquire_durable(&state,&dir,"shell_exec",&args,"reader")
                .await.unwrap().is_none());
        }
        for command in ["sleep 1; touch bad","sleep 1 && touch bad",
            "sleep $(touch bad)","sleep 9999","date > bad",
            "cargo test","git status","bash -c 'sleep 1'"]{
            assert_eq!(key(&dir,"shell_exec",&json!({"command":command})),
                key(&dir,"workspace_write",&Value::Null),
                "non-literal or workspace-affecting shell must remain locked: {command}");
        }
        drop(writer);drop(state);
        let _=std::fs::remove_dir_all(dir);
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
    #[tokio::test]
    async fn absolute_file_write_inside_workspace_shares_writer_lease_with_shell(){
        let dir=std::env::temp_dir().join(format!("writer-alias-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        let file=json!({"path":dir.join("nested").join("work.rs").to_string_lossy()});
        assert_eq!(key(&dir,"file_write",&file),key(&dir,"shell_exec",&Value::Null));
        assert_eq!(key(&dir,"file_write",&file),key(&dir,"workspace_apply_patch",&Value::Null));
        assert!(key(&dir,"file_write",&Value::Null).is_some());
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
            acquire_durable(&two,&dir,"file_write",&file,"b")).await.is_err());
        drop(first);
        let next=acquire_durable(&two,&dir,"file_write",&file,"b")
            .await.unwrap().unwrap();
        drop(next);
        drop(one);drop(two);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn outside_file_write_uses_independent_external_scope(){
        let root=std::env::temp_dir().join(format!("writer-root-{}",uuid::Uuid::new_v4()));
        let external=std::env::temp_dir().join(format!("writer-other-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&external).unwrap();
        let foreign=json!({"path":external.join("different.txt").to_string_lossy()});
        assert_ne!(key(&root,"file_write",&foreign),key(&root,"workspace_write",&Value::Null));
        assert!(key(&root,"file_write",&foreign).unwrap().starts_with("external:"));
        let _=std::fs::remove_dir_all(root);
        let _=std::fs::remove_dir_all(external);
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
