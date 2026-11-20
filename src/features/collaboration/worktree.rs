//! Owner-selected Git worktree isolation for Task Team workers (2026).
//! Workers never supply arbitrary filesystem paths: the durable host-created
//! worktree record binds parent, task, source repository and base commit.
//! No implicit merge, forced cleanup, ref rewrite, or user branch checkout.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::io::{AsyncReadExt,AsyncWriteExt};
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
        "baseCommit":commit,"status":"preparing","mergeStatus":"pending",
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

/// Read the exact changes against the pinned source commit; no untracked
/// files, external diff drivers or textconv are allowed into a promotion.
/// The patch never enters persistent records, prompt context or HTTP output.
const MAX_PATCH:usize=2*1024*1024;
async fn patch(workspace:&Path,base:&str)->Result<Vec<u8>,String>{
    let mut child=Command::new("git").arg("-C").arg(workspace)
        .args(["diff","--binary","--no-ext-diff","--no-textconv",base,"--"])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true).spawn().map_err(|e|e.to_string())?;
    let mut data=Vec::new();
    let mut stdout=child.stdout.take().ok_or("Git diff stdout unavailable")?;
    let read=tokio::time::timeout(GIT_TIMEOUT,
        stdout.take((MAX_PATCH+1) as u64).read_to_end(&mut data)).await;
    match read{
        Ok(Ok(_)) if data.len()<=MAX_PATCH=>{},
        Ok(Ok(_))=>{let _=child.kill().await;return Err("Worktree patch is over the 2MB review limit".into());},
        Ok(Err(e))=>{let _=child.kill().await;return Err(e.to_string());},
        Err(_)=>{let _=child.kill().await;return Err("Worktree diff timed out".into());},
    }
    let exit=tokio::time::timeout(GIT_TIMEOUT,child.wait()).await
        .map_err(|_|"Git diff wait timed out".to_string())?
        .map_err(|e|e.to_string())?;
    if !exit.success(){return Err("Git diff failed".into());}
    Ok(data)
}
async fn apply(source:&Path,data:&[u8],check:bool)->Result<(),String>{
    let mut command=Command::new("git");
    command.arg("-C").arg(source).arg("apply")
        .args(["--index","--whitespace=nowarn"]);
    if check {command.arg("--check");}
    let mut child=command.stdin(Stdio::piped()).stdout(Stdio::null())
        .stderr(Stdio::null()).kill_on_drop(true).spawn()
        .map_err(|e|format!("Git apply unavailable: {e}"))?;
    let mut pipe=child.stdin.take().ok_or("Git apply input missing")?;
    let run=tokio::time::timeout(GIT_TIMEOUT,async{
        pipe.write_all(data).await.map_err(|e|e.to_string())?;
        drop(pipe);
        child.wait().await.map_err(|e|e.to_string())
    }).await.map_err(|_|"Git apply timed out".to_string())??;
    if !run.success(){
        return Err(if check{"Worktree patch conflicts with target index/workspace".into()}
            else{"Git apply failed; target requires owner review".into()});
    }
    Ok(())
}
async fn review_data(state:&AppState,parent:&str,task_id:&str)
    ->Result<(Value,PathBuf,PathBuf,Vec<u8>,Value),String>{
    use super::task_team::{self,Isolation,Mode,Team};
    let lead=state.store.get("job",parent)
        .filter(|v|v["kind"]=="team"&&v["status"]=="completed")
        .ok_or("Task Team has not completed successfully")?;
    let team=Team::parse(&lead["input"])?;
    let task=team.tasks.iter().find(|node|node.id==task_id
        && node.mode==Mode::Write && node.isolation==Isolation::Worktree)
        .ok_or("Only owner-selected isolated write tasks may be promoted")?;
    let child=state.store.get("job",&task_team::child_id(parent,task_id))
        .ok_or("Writer has no durable job")?;
    if !task_team::belongs_to_team(parent,task,&child)
        ||!task_team::complete(task,&child){
        return Err("Worktree writer has no fully verified host receipts".into());
    }
    let src=canonical_root(state)?;
    let isolated=for_job(state,&child)?;
    let id=id_for(parent,task_id);
    let record=state.store.get("team_worktree",&id)
        .ok_or("Host-pinned worktree disappeared")?;
    if record["mergeStatus"]!="pending"{
        return Err("Worktree merge is no longer pending; manual review required".into());
    }
    validate_existing(&isolated,&record).await?;
    let base=record["baseCommit"].as_str().ok_or("Base commit missing")?;
    if git(&src,&["rev-parse","HEAD"]).await?!=base{
        return Err("Source branch moved after worktree creation; manual rebase required".into());
    }
    // The target index and tracked files must both be pristine; untracked
    // target files are still protected by git apply --check --index.
    if !git(&src,&["status","--porcelain","--untracked-files=no"]).await?.is_empty(){
        return Err("Source has uncommitted tracked changes; cannot auto-promote".into());
    }
    if !git(&isolated,&["ls-files","--others","--exclude-standard"]).await?.is_empty(){
        return Err("Worktree contains untracked files; add/commit them before promotion".into());
    }
    let data=patch(&isolated,base).await?;
    if data.is_empty(){return Err("Worktree produced no tracked source changes".into());}
    let hash=format!("{:x}",Sha256::digest(&data));
    let names=git(&isolated,&["diff","--name-only","--no-ext-diff",base,"--"]).await?
        .lines().take(80).map(str::to_owned).collect::<Vec<_>>();
    Ok((record,src,isolated,data,json!({
        "teamId":parent,"taskId":task_id,
        "worktreeId":id,"baseCommit":base,
        "diffSha256":hash,"patchBytes":data.len(),
        "fileCount":names.len(),"files":names,
        "requiresExplicitPromotion":true
    })))
}
/// Preview has zero effects. The approval token is the SHA-256 of the exact
/// bounded binary patch that promotion will re-read and re-check.
pub async fn preview(state:&AppState,parent:&str,task:&str)->Result<Value,String>{
    let (_,_,_,_,view)=review_data(state,parent,task).await?;
    Ok(view)
}
/// Only an explicitly confirmed owner request can apply changes to the
/// source. Source HEAD/cleanliness and patch digest are revalidated under
/// an exclusive workspace lock; no branch switch, commit or push occurs.
pub async fn promote(state:&AppState,parent:&str,task:&str,
    expected_digest:&str)->Result<Value,String>{
    if expected_digest.len()!=64||!expected_digest.bytes()
        .all(|c|c.is_ascii_hexdigit()){
        return Err("A verified 64-character preview digest is required".into());
    }
    let source=canonical_root(state)?;
    let _guard=crate::host::execution::lease::LeaseGuard::acquire_workspace_for(
        state,&source,"write")
        .ok_or("Another Task Team is using this workspace")?;
    let (record,source,_,data,view)=review_data(state,parent,task).await?;
    if view["diffSha256"].as_str()!=Some(expected_digest){
        return Err("Reviewed worktree diff changed; preview again".into());
    }
    apply(&source,&data,true).await?;
    let id=record["id"].as_str().ok_or("Invalid merge record")?;
    let fenced=state.store.patch_if("team_worktree",id,"mergeStatus",
        &json!("pending"),&json!({"mergeStatus":"applying",
            "reviewedDiffSha256":expected_digest,
            "mergeStartedAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("Worktree promotion is already owned by another request")?;
    let committed=apply(&source,&data,false).await;
    if let Err(reason)=committed {
        state.store.patch_if("team_worktree",id,"mergeStatus",&json!("applying"),
            &json!({"mergeStatus":"needs_review","mergeError":reason}));
        return Err(reason);
    }
    let status=state.store.patch_if("team_worktree",id,"mergeStatus",
        &json!("applying"),&json!({"mergeStatus":"applied",
            "appliedAt":chrono::Utc::now().to_rfc3339()}));
    if status.is_none(){
        return Err("Patch was applied but durable receipt could not be recorded; review before retry".into());
    }
    state.store.audit("team.worktree.promoted",json!({
        "worktreeId":id,"parentRunId":parent,"taskId":task,
        "diffSha256":expected_digest,"sourceRoot":fenced["sourceRoot"],
        "mergeStatus":"applied","committed":false
    }));
    Ok(json!({"status":"applied","requiresOwnerCommit":true,
        "diffSha256":expected_digest,"staged":true,"worktreeId":id,
        "files":view["files"]}))
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

    #[tokio::test]
    async fn owner_reviews_exact_patch_and_promotes_only_after_confirmation(){
        let (state,dir)=fixture();
        let id=prepare(&state,"lead-promote","writer").await.unwrap();
        let worker=json!({"id":"team-child:lead-promote:writer","kind":"prompt",
            "status":"completed","input":{"parentRunId":"lead-promote",
                "teamTaskId":"writer","teamMode":"write",
                "bridgeRuntime":"native","teamIsolation":"worktree",
                "teamWorktreeId":id},
            "result":{"status":"completed",
                "receipts":[{"tool":"workspace_write","id":"write-1","success":true}],
                "checks":[{"satisfied":true}]},
            "nativeEvidenceBaseline":{"complete":true,
                "workspaceRootDigest":"workspace-digest",
                "criteriaDigest":"criteria-digest",
                "entries":{"sample.txt":{"state":"file","sha256":"old"}}},
            "nativeEvidenceFinal":{"complete":true,
                "workspaceRootDigest":"workspace-digest",
                "criteriaDigest":"criteria-digest",
                "entries":{"sample.txt":{"state":"file","sha256":"new"}}}
        });
        state.store.put("job",&json!({"id":"lead-promote","kind":"team",
            "status":"completed","input":{"goal":"Update sample","maxParallel":1,
                "tasks":[{"id":"writer","prompt":"Update source",
                    "runtime":"native","mode":"write","isolation":"worktree",
                    "checks":[{"type":"file_contains","path":"sample.txt",
                        "contains":"edited"}]}]}})).unwrap();
        state.store.put("job",&worker).unwrap();
        let target=for_job(&state,&worker).unwrap();
        std::fs::write(target.join("sample.txt"),"edited").unwrap();
        let snapshot=preview(&state,"lead-promote","writer").await.unwrap();
        let original=state.config.workspace.join("sample.txt");
        assert_eq!(std::fs::read_to_string(&original).unwrap(),"original");
        assert!(promote(&state,"lead-promote","writer",
            &"0".repeat(64)).await.is_err());
        assert_eq!(std::fs::read_to_string(&original).unwrap(),"original");
        let reviewed=snapshot["diffSha256"].as_str().unwrap();
        let outcome=promote(&state,"lead-promote","writer",reviewed).await.unwrap();
        assert_eq!(outcome["status"],"applied");
        assert_eq!(outcome["requiresOwnerCommit"],true);
        assert_eq!(std::fs::read_to_string(&original).unwrap(),"edited");
        assert_eq!(state.store.get("team_worktree",&id).unwrap()["mergeStatus"],
            "applied");
        assert!(promote(&state,"lead-promote","writer",reviewed).await.is_err());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn worktree_rejects_source_divergence_before_owner_promotion(){
        let (state,dir)=fixture();
        let id=prepare(&state,"lead-stale","writer").await.unwrap();
        let record=state.store.get("team_worktree",&id).unwrap();
        let baseline=record["baseCommit"].as_str().unwrap().to_string();
        assert_eq!(git(&state.config.workspace,&["rev-parse","HEAD"]).await.unwrap(),
            baseline);
        // A separate commit on the user's branch must never be overwritten.
        std::fs::write(state.config.workspace.join("sample.txt"),"owner edited").unwrap();
        assert!(std::process::Command::new("git").arg("-C").arg(&state.config.workspace)
            .args(["add","sample.txt"]).status().unwrap().success());
        assert!(std::process::Command::new("git").arg("-C").arg(&state.config.workspace)
            .args(["-c","user.name=DOMINION","-c","user.email=dev@example.test",
                "commit","-qm","owner moved source"]).status().unwrap().success());
        assert_ne!(git(&state.config.workspace,&["rev-parse","HEAD"]).await.unwrap(),
            baseline);
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
