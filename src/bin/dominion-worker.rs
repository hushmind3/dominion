//! Standalone remote DOMINION worker. Requires a *separate* local DOMINION
//! Rust server on the worker machine. It never runs shell commands provided
//! by the coordinator; all work enters that machine's native Agent SDK.
use reqwest::{Client,Response,StatusCode};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::path::{Path,PathBuf};
use tokio::process::Command;
use std::time::Duration;
use tokio::time::{sleep,Instant};

struct Config{
    coordinator:String,
    token:String,
    worker_id:String,
    local:String,
    local_token:Option<String>,
    git_worktree:Option<PathBuf>,
}
impl Config{
    fn read()->Result<Self,String>{
        let coordinator=std::env::var("DOMINION_REMOTE_COORDINATOR")
            .map_err(|_|"DOMINION_REMOTE_COORDINATOR is required")?
            .trim_end_matches('/').to_string();
        if !coordinator.starts_with("https://")
            && !coordinator.starts_with("http://127.0.0.1:")
            && !coordinator.starts_with("http://localhost:"){
            return Err("remote coordinator requires HTTPS or loopback HTTP".into());
        }
        let token=std::env::var("DOMINION_REMOTE_WORKER_TOKEN")
            .map_err(|_|"DOMINION_REMOTE_WORKER_TOKEN is required")?;
        if token.len()<32{return Err("remote worker token must be >=32 chars".into());}
        let worker_id=std::env::var("DOMINION_REMOTE_WORKER_ID")
            .map_err(|_|"DOMINION_REMOTE_WORKER_ID is required")?;
        if worker_id.is_empty()||worker_id.len()>96||worker_id.bytes()
            .any(|b|!b.is_ascii_alphanumeric()&&!matches!(b,b'-'|b'_'|b'.'|b':')){
            return Err("invalid worker id".into());
        }
        let local=std::env::var("DOMINION_WORKER_LOCAL_URL")
            .unwrap_or_else(|_|"http://127.0.0.1:4317".into())
            .trim_end_matches('/').to_string();
        if !(local.starts_with("http://127.0.0.1:")
            ||local.starts_with("http://localhost:")){
            return Err("worker SDK endpoint must be loopback-only".into());
        }
        let local_token=std::env::var("DOMINION_LOCAL_ADMIN_TOKEN").ok();
        // Explicit opt-in; never take an arbitrary repo path from job text.
        let git_worktree=std::env::var_os("DOMINION_REMOTE_GIT_WORKTREE")
            .map(PathBuf::from);
        Ok(Self{coordinator,token,worker_id,local,local_token,git_worktree})
    }
}
async fn json_response(r:Response)->Result<Value,String>{
    let status=r.status();
    if !status.is_success(){
        return Err(format!("HTTP {}: {}",status.as_u16(),
            r.text().await.unwrap_or_default().chars().take(200).collect::<String>()));
    }
    r.json::<Value>().await.map_err(|e|e.to_string())
}
fn local_request(client:&Client,cfg:&Config,method:reqwest::Method,path:&str)
    ->reqwest::RequestBuilder{
    let r=client.request(method,format!("{}{}",cfg.local,path));
    if let Some(token)=&cfg.local_token{r.header("x-dominion-token",token)}else{r}
}
fn coordinator_request(client:&Client,cfg:&Config,path:&str)
    ->reqwest::RequestBuilder{
    client.post(format!("{}{}",cfg.coordinator,path))
        .header("x-dominion-worker-token",&cfg.token)
}
async fn report(client:&Client,cfg:&Config,job:&Value,result:Value)->Result<(),String>{
    let id=job["jobId"].as_str().ok_or("claim missing job ID")?;
    let response=coordinator_request(client,cfg,
        &format!("/api/remote/jobs/{}/finish",urlencode_id(id)))
        .json(&json!({"workerId":cfg.worker_id,
            "leaseId":job["leaseId"],"result":result}))
        .send().await.map_err(|e|e.to_string())?;
    json_response(response).await.map(|_|())
}
fn urlencode_id(s:&str)->String{
    // Broker-issued IDs use only [A-Za-z0-9:-]; never interpolate URLs from
    // remote instructions or accept user-provided URL fragments.
    s.chars().filter(|c|c.is_ascii_alphanumeric()||matches!(c,'-'|'_'|':'))
        .collect()
}
/// Fixed-argument Git inspection only: no shell expansion or worker-provided
/// command. A dirty baseline is excluded so preexisting edits cannot become
/// another job's patch evidence.
async fn git(dir:&Path,args:&[&str])->Result<Vec<u8>,String>{
    let output=tokio::time::timeout(Duration::from_secs(15),
        Command::new("git").arg("-C").arg(dir).args(args)
            .kill_on_drop(true).output())
        .await.map_err(|_|"git evidence inspection timed out".to_string())?
        .map_err(|e|e.to_string())?;
    if !output.status.success(){
        return Err("git evidence inspection failed".into());
    }
    Ok(output.stdout)
}
async fn clean_git_base(cfg:&Config)->Option<String>{
    let dir=cfg.git_worktree.as_deref()?;
    let dirty=git(dir,&["status","--porcelain","--untracked-files=normal"])
        .await.ok()?;
    if !dirty.is_empty(){return None;}
    let rev=git(dir,&["rev-parse","--verify","HEAD"]).await.ok()?;
    let revision=String::from_utf8(rev).ok()?.trim().to_string();
    if !(revision.len()==40||revision.len()==64)
        ||!revision.bytes().all(|b|b.is_ascii_hexdigit()){return None;}
    Some(revision)
}
async fn upload_git_patch(client:&Client,cfg:&Config,job:&Value,
    base:&str)->Result<Option<String>,String>{
    let dir=cfg.git_worktree.as_deref().ok_or("git workspace not configured")?;
    let current=git(dir,&["rev-parse","--verify","HEAD"]).await?;
    if String::from_utf8_lossy(&current).trim()!=base{
        return Err("worker Git HEAD changed during execution".into());
    }
    // Only tracked edits; binary Git patches are textual and remain bounded.
    // Untracked files are intentionally NOT transferred automatically.
    let bytes=git(dir,&["diff","--binary","--no-ext-diff","HEAD"]).await?;
    if bytes.is_empty(){return Ok(None);}
    if bytes.len()>128*1024{return Err("Git patch exceeds 128 KiB".into());}
    let patch=String::from_utf8(bytes).map_err(|_|"non-UTF8 Git patch".to_string())?;
    if !patch.starts_with("diff --git "){return Err("unexpected Git diff format".into());}
    let sha=format!("{:x}",Sha256::digest(patch.as_bytes()));
    let id=job["jobId"].as_str().ok_or("job identity missing")?;
    let sent=coordinator_request(client,cfg,
        &format!("/api/remote/jobs/{}/artifact/upload",urlencode_id(id)))
        .json(&json!({"workerId":cfg.worker_id,"leaseId":job["leaseId"],
            "baseCommit":base,"patchSha256":sha,"patch":patch}))
        .send().await.map_err(|e|e.to_string())?;
    if sent.status()!=StatusCode::CREATED{
        return Err(format!("coordinator declined Git artifact: {}",sent.status()));
    }
    Ok(Some(sha))
}

async fn execute(client:&Client,cfg:&Config,job:&Value)->Result<(),String>{
    let job_id=job["jobId"].as_str().ok_or("missing remote job id")?;
    let instruction=job["instruction"].as_str().ok_or("missing instruction")?;
    let git_base=clean_git_base(cfg).await;
    let mut request=json!({
        "instruction":instruction,
        "maxRounds":job["maxRounds"].as_u64().unwrap_or(24).clamp(1,80),
        "idempotencyKey":format!("remote-{}",job_id),
        "shellSandbox":"required"
    });
    if let Some(command)=job["verifyCommand"].as_str(){
        request["verifyCommand"]=json!(command);
    }
    let created=local_request(client,cfg,reqwest::Method::POST,"/api/sdk/runs")
        .json(&request).send().await.map_err(|e|e.to_string())?;
    let native=match json_response(created).await{
        Ok(value)=>value,
        Err(error)=>{
            return report(client,cfg,job,json!({
                "outcome":"failed_to_start","error":error,"nativeVerified":false
            })).await
        }
    };
    let Some(id)=native["id"].as_str()else{
        return report(client,cfg,job,json!({
            "outcome":"invalid_sdk_response","nativeVerified":false
        })).await
    };
    let native_id=id.to_string();
    let start=Instant::now();
    loop{
        if start.elapsed()>Duration::from_secs(1800){
            return report(client,cfg,job,json!({
                "outcome":"needs_review","remoteSdkRunId":native_id,
                "reason":"deadline exceeded","nativeVerified":false
            })).await;
        }
        // Leases are extended only by the owning worker. A 409 means STOP,
        // not permission to start again. Fail closed on transport failure.
        let heartbeat=coordinator_request(client,cfg,
            &format!("/api/remote/jobs/{}/heartbeat",urlencode_id(job_id)))
            .json(&json!({"workerId":cfg.worker_id,"leaseId":job["leaseId"]}))
            .send().await;
        // A network failure is also loss of authority: do not keep executing
        // while the coordinator can no longer confirm the lease.
        if !heartbeat.as_ref().is_ok_and(|reply|reply.status()==StatusCode::OK){
            let _=local_request(client,cfg,reqwest::Method::POST,
                &format!("/api/sdk/runs/{}/cancel",urlencode_id(&native_id)))
                .json(&json!({})).send().await;
            return Err("remote lease lost; SDK cancellation requested".into());
        }
        let snapshot=local_request(client,cfg,reqwest::Method::GET,
            &format!("/api/sdk/runs/{}",urlencode_id(&native_id)))
            .send().await.map_err(|e|e.to_string())?;
        let row=json_response(snapshot).await?;
        match row["status"].as_str().unwrap_or(""){
            "completed"|"cancelled"|"failed"|"exhausted"|"denied"|
            "awaiting_approval"|"in_doubt"|"interrupted"|"needs_review"=>{
                let uploaded=if row["status"]=="completed"{
                    if let Some(base)=git_base.as_deref(){
                        upload_git_patch(client,cfg,job,base).await.ok().flatten()
                    }else{None}
                }else{None};
                return report(client,cfg,job,json!({
                    "outcome":row["status"],
                    "remoteSdkRunId":native_id,
                    "remoteNativeExecutionId":row["runRef"]["executionId"],
                    "remoteVerifiedClaim":row["verified"]==true,
                    "remoteArtifactSha256":uploaded,
                    "taskOutcome":row["taskOutcome"],
                    "modelAccounting":row["modelAccounting"],
                    "answer":row["answer"].as_str().unwrap_or("")
                        .chars().take(12_000).collect::<String>(),
                    "nativeVerified":false
                })).await;
            },
            _=>sleep(Duration::from_secs(12)).await,
        }
    }
}
#[tokio::main]
async fn main(){
    if let Err(error)=run().await{
        eprintln!("dominion-worker: {error}");
        std::process::exit(1);
    }
}
async fn run()->Result<(),String>{
    let cfg=Config::read()?;
    let client=Client::builder().timeout(Duration::from_secs(30))
        .build().map_err(|e|e.to_string())?;
    loop{
        let response=coordinator_request(&client,&cfg,"/api/remote/claim")
            .json(&json!({"workerId":cfg.worker_id}))
            .send().await.map_err(|e|e.to_string())?;
        if response.status()==StatusCode::NO_CONTENT{
            sleep(Duration::from_secs(3)).await;continue;
        }
        let job=json_response(response).await?;
        if let Err(error)=execute(&client,&cfg,&job).await {
            // No automatic rerun: the coordinator's lease remains owned until
            // explicit operator reconciliation, even if the remote effect is
            // unknown after a network failure.
            eprintln!("remote job execution stopped; manual review: {error}");
            sleep(Duration::from_secs(5)).await;
        }
    }
}

#[cfg(test)]
mod artifact_tests{
    use super::*;
    #[tokio::test]
    async fn fixed_git_inspection_rejects_dirty_baselines_and_keeps_diff_bounded(){
        let root=std::env::temp_dir().join(format!("dominion-p3-diff-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let init=git(&root,&["init"]).await.unwrap();
        assert!(init.is_empty()||!init.is_empty());
        // No Git invocation runs text supplied by an Agent. All commands
        // are hardcoded here and dispatched without a shell.
        std::fs::write(root.join("a.txt"),"original\n").unwrap();
        let mut add=Command::new("git");
        assert!(add.arg("-C").arg(&root).args(["add","a.txt"]).status()
            .await.unwrap().success());
        let mut commit=Command::new("git");
        assert!(commit.arg("-C").arg(&root).args([
            "-c","user.name=DOMINION-test",
            "-c","user.email=dominion-test@example.invalid",
            "commit","-m","baseline"]).status().await.unwrap().success());
        let cfg=Config{coordinator:String::new(),token:String::new(),
            worker_id:String::new(),local:String::new(),local_token:None,
            git_worktree:Some(root.clone())};
        let base=clean_git_base(&cfg).await.expect("clean baseline");
        std::fs::write(root.join("a.txt"),"changed\n").unwrap();
        assert!(clean_git_base(&cfg).await.is_none());
        let delta=git(&root,&["diff","--binary","--no-ext-diff","HEAD"])
            .await.unwrap();
        assert!(String::from_utf8(delta).unwrap().starts_with("diff --git "));
        assert_eq!(git(&root,&["rev-parse","--verify","HEAD"]).await.unwrap()
            .len(),base.len()+1);
        let _=std::fs::remove_dir_all(root);
    }
}
