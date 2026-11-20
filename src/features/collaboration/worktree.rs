//! Owner-selected Git worktree isolation for Task Team workers (2026).
//! Workers never supply arbitrary filesystem paths: the durable host-created
//! worktree record binds parent, task, source repository and base commit.
//! No implicit merge, forced cleanup, ref rewrite, or user branch checkout.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::process::Command;

const GIT_TIMEOUT:Duration=Duration::from_secs(20);
fn id_for(parent:&str,task:&str)->String{
    let digest=Sha256::digest(format!("v1:{parent}:{task}").as_bytes());
    format!("team-worktree-{:x}",digest)
}
async fn git(cwd:&Path,args:&[&str])->Result<String,String>{
    let output=tokio::time::timeout(GIT_TIMEOUT,
        Command::new("git").arg("-C").arg(cwd).args(args)
            .stdin(Stdio::null()).kill_on_drop(true).output())
        .await.map_err(|_|"Git worktree operation timed out".to_string())?
        .map_err(|e|format!("Git worktree unavailable: {e}"))?;
    if output.stdout.len()>16_384 || output.stderr.len()>16_384{
        return Err("Git worktree response exceeded size bound".into());
    }
    if !output.status.success(){
        return Err(format!("Git worktree operation failed: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(800)
                .collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
fn canonical_root(state:&AppState)->Result<PathBuf,String>{
    std::fs::canonicalize(&state.config.workspace)
        .map_err(|_|"Task Team source workspace does not exist".into())
}
fn base_dir(state:&AppState)->Result<PathBuf,String>{
    let base=state.config.data_dir.join("task-worktrees");
    std::fs::create_dir_all(&base).map_err(|e|e.to_string())?;
    std::fs::canonicalize(&base).map_err(|e|e.to_string())
}
fn record_valid(record:&Value,parent:&str,task:&str,id:&str,base:&Path)->bool{
    record["id"]==id && record["parentRunId"]==parent
        && record["taskId"]==task && record["schemaVersion"]==1
        && record["sourceRoot"].as_str()==base.to_str()
        && record["baseCommit"].as_str().is_some_and(|v|
            v.len()==40&&v.bytes().all(|c|c.is_ascii_hexdigit()))
}
fn valid_path(base:&Path,id:&str,record:&Value)->Result<PathBuf,String>{
    let target=base.join(id);
    if record["worktreePath"].as_str()!=target.to_str() {
        return Err("Worktree path changed after host pin".into());
    }
    if !target.exists(){
        return Err("Worktree is missing; owner review required".into());
    }
    let actual=std::fs::canonicalize(&target).map_err(|e|e.to_string())?;
    if actual!=target || !actual.starts_with(base){
        return Err("Worktree escaped pinned host-owned directory".into());
    }
    Ok(actual)
}
async fn validate_existing(path:&Path,record:&Value)->Result<(),String>{
    let git_root=git(path,&["rev-parse","--show-toplevel"]).await?;
    if std::fs::canonicalize(git_root).map_err(|e|e.to_string())?!=path{
        return Err("Pinned worktree is no longer a distinct Git checkout".into());
    }
    let base=record["baseCommit"].as_str().ok_or("Missing base commit")?;
    git(path,&["merge-base","--is-ancestor",base,"HEAD"]).await?;
    Ok(())
}
/// Idempotent preparation. Record immutable intent before any Git side
/// effect so a supervisor crash can reconcile without creating duplicates.
pub async fn prepare(state:&AppState,parent:&str,task:&str)->Result<String,String>{
    if parent.is_empty()||parent.len()>256||task.is_empty()||task.len()>48{
        return Err("Invalid Task Team worktree owner binding".into());
    }
    let source=canonical_root(state)?;
    let source_git=git(&source,&["rev-parse","--show-toplevel"]).await?;
    if std::fs::canonicalize(source_git).map_err(|e|e.to_string())?!=source{
        return Err("Worktree isolation requires the workspace be a Git repository root".into());
    }
    let commit=git(&source,&["rev-parse","HEAD"]).await?;
    if commit.len()!=40||!commit.bytes().all(|c|c.is_ascii_hexdigit()){
        return Err("Source Git HEAD is not a pinned SHA-1 commit".into());
    }
    let base=base_dir(state)?;
    let id=id_for(parent,task);
    let target=base.join(&id);
    let new_record=json!({"id":id,"schemaVersion":1,
        "parentRunId":parent,"taskId":task,
        "sourceRoot":source.to_string_lossy(),"worktreePath":target.to_string_lossy(),
        "baseCommit":commit,"status":"preparing",
        "createdAt":chrono::Utc::now().to_rfc3339()});
    let record=state.store.put_if_absent("team_worktree",&new_record)
        .or_else(||state.store.get("team_worktree",&id))
        .ok_or("Could not persist immutable worktree intent")?;
    if !record_valid(&record,parent,task,&id,&source){
        return Err("Worktree belongs to a different parent, task or source".into());
    }
    if record["status"]=="ready" {
        let verified=valid_path(&base,&id,&record)?;
        validate_existing(&verified,&record).await?;
        return Ok(id);
    }
    if record["status"]!="preparing"{
        return Err("Worktree requires manual review before reuse".into());
    }
    if target.exists(){
        let verified=valid_path(&base,&id,&record)?;
        validate_existing(&verified,&record).await?;
    }else{
        let commit=record["baseCommit"].as_str().ok_or("Missing pinned commit")?;
        let target_string=target.to_string_lossy().to_string();
        git(&source,&["worktree","add","--detach",&target_string,commit]).await?;
        let verified=valid_path(&base,&id,&record)?;
        validate_existing(&verified,&record).await?;
    }
    state.store.patch_if("team_worktree",&id,"status",&json!("preparing"),
        &json!({"status":"ready","readyAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("Could not persist prepared Git worktree state")?;
    Ok(id)
}
/// Resolve only a host-created record pinned to the current child job. This
/// is the single workspace selection used for model tools and verification.
pub fn for_job(state:&AppState,job:&Value)->Result<PathBuf,String>{
    let input=&job["input"];
    if input["teamIsolation"]!="worktree"{
        if input["teamWorktreeId"].is_string(){
            return Err("Unrequested worktree binding on shared worker".into());
        }
        return Ok(state.config.workspace.clone());
    }
    let parent=input["parentRunId"].as_str().ok_or("Worktree parent missing")?;
    let task=input["teamTaskId"].as_str().ok_or("Worktree task missing")?;
    let expected=id_for(parent,task);
    if input["teamWorktreeId"].as_str()!=Some(expected.as_str()){
        return Err("Worktree binding does not match parent and task".into());
    }
    let record=state.store.get("team_worktree",&expected)
        .ok_or("Pinned worktree record is missing")?;
    let source=canonical_root(state)?;
    if !record_valid(&record,parent,task,&expected,&source)||record["status"]!="ready"{
        return Err("Worktree record is not ready or owner-bound".into());
    }
    valid_path(&base_dir(state)?,&expected,&record)
}
pub fn view(state:&AppState,child:&Value)->Value{
    let id=child["input"]["teamWorktreeId"].as_str().unwrap_or("");
    if id.is_empty(){return Value::Null;}
    state.store.get("team_worktree",id).map(|record|json!({
        "id":record["id"],"status":record["status"],"baseCommit":record["baseCommit"],
        "path":record["worktreePath"],"mergeStatus":record["mergeStatus"],
        "isolated":true
    })).unwrap_or(Value::Null)
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->(AppState,PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-team-git-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("repository")).unwrap();
        let src=dir.join("repository");
        let status=std::process::Command::new("git").arg("init")
            .arg("-q").arg(&src).status().unwrap();
        assert!(status.success());
        std::fs::write(src.join("sample.txt"),"original").unwrap();
        for args in [&["add","sample.txt"][..],
            &["-c","user.name=DOMINION","-c","user.email=dev@example.test",
                "commit","-qm","baseline"][..]]{
            assert!(std::process::Command::new("git").arg("-C").arg(&src)
                .args(args).status().unwrap().success());
        }
        let cfg=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),
            workspace:src};
        std::fs::create_dir_all(&cfg.data_dir).unwrap();
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        (state,dir)
    }
    #[tokio::test]
    async fn isolated_worker_has_durable_independent_files_and_no_automatic_merge(){
        let (state,dir)=fixture();
        let id=prepare(&state,"team-a","write").await.unwrap();
        let again=prepare(&state,"team-a","write").await.unwrap();
        assert_eq!(id,again);
        let worker=json!({"id":"team-child:team-a:write","input":{
            "parentRunId":"team-a","teamTaskId":"write",
            "teamIsolation":"worktree","teamWorktreeId":id}});
        let target=for_job(&state,&worker).unwrap();
        std::fs::write(target.join("sample.txt"),"edited in isolation").unwrap();
        assert_eq!(std::fs::read_to_string(state.config.workspace.join("sample.txt"))
            .unwrap(),"original");
        assert_eq!(std::fs::read_to_string(target.join("sample.txt"))
            .unwrap(),"edited in isolation");
        assert!(for_job(&state,&json!({"input":{"teamIsolation":"worktree",
            "parentRunId":"team-b","teamTaskId":"write",
            "teamWorktreeId":id}})).is_err());
        assert_eq!(view(&state,&worker)["status"],"ready");
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn unreviewed_workspace_paths_are_not_accepted(){
        let (state,dir)=fixture();
        assert!(for_job(&state,&json!({"input":{"teamIsolation":"worktree",
            "parentRunId":"fake","teamTaskId":"foreign",
            "teamWorktreeId":"arbitrary"}})).is_err());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
}
