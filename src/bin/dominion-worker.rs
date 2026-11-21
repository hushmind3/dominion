//! Standalone remote DOMINION worker. Requires a *separate* local DOMINION
//! Rust server on the worker machine. It never runs shell commands provided
//! by the coordinator; all work enters that machine's native Agent SDK.
use reqwest::{Client,Response,StatusCode};
use serde_json::{json,Value};
use std::time::Duration;
use tokio::time::{sleep,Instant};

struct Config{
    coordinator:String,
    token:String,
    worker_id:String,
    local:String,
    local_token:Option<String>,
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
        Ok(Self{coordinator,token,worker_id,local,local_token})
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
async fn execute(client:&Client,cfg:&Config,job:&Value)->Result<(),String>{
    let job_id=job["jobId"].as_str().ok_or("missing remote job id")?;
    let instruction=job["instruction"].as_str().ok_or("missing instruction")?;
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
            .send().await.map_err(|e|e.to_string())?;
        if heartbeat.status()!=StatusCode::OK{
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
                return report(client,cfg,job,json!({
                    "outcome":row["status"],
                    "remoteSdkRunId":native_id,
                    "remoteNativeExecutionId":row["runRef"]["executionId"],
                    "remoteVerifiedClaim":row["verified"]==true,
                    "taskOutcome":row["taskOutcome"],
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
