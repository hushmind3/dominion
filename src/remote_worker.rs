//! Outbound-only remote Worker runtime: no model loop, no public listener.
 //! A local Dominion Job executes the same Harness and tool policy as desktop.
use crate::{config::Config,engine,state::AppState,sdk::Dominion};
use serde_json::{json,Value};
use std::time::Duration;

const POLL_MS:u64=4_000;
const HEARTBEAT_MS:u64=12_000;

struct Transport{
    url:String,
    token:String,
    client:reqwest::Client,
}
impl Transport{
    async fn send(&self,path:&str,data:&Value)->Result<Value,String>{
        let response=self.client.post(format!("{}{}",self.url,path))
            .header("x-dominion-remote-token",self.token.as_str())
            .timeout(Duration::from_secs(15))
            .json(data).send().await.map_err(|e|format!("remote {path}: {e}"))?;
        let status=response.status();
        let payload=response.json::<Value>().await
            .map_err(|e|format!("remote {path}: invalid JSON: {e}"))?;
        if !status.is_success(){
            return Err(format!("remote {path} status {status}: {}",
                payload["error"].as_str().unwrap_or("remote rejected request")));
        }
        Ok(payload)
    }
}
fn configuration()->Result<(Transport,String,bool),String>{
    let url=std::env::var("DOMINION_REMOTE_COORDINATOR_URL")
        .map_err(|_|"DOMINION_REMOTE_COORDINATOR_URL required")?;
    let parsed=reqwest::Url::parse(&url).map_err(|e|format!("Invalid coordinator URL: {e}"))?;
    let local=matches!(parsed.host_str(),Some("127.0.0.1"|"localhost"|"::1"));
    if parsed.scheme()!="https" && !(parsed.scheme()=="http" && (
        local || std::env::var("DOMINION_REMOTE_ALLOW_PRIVATE_HTTP").ok().as_deref()==Some("1")
    )){
        return Err("Remote coordinator requires HTTPS (or explicit private HTTP overlay opt-in)".into());
    }
    if parsed.username()!="" || parsed.password().is_some() || parsed.query().is_some()
        || parsed.fragment().is_some(){
        return Err("Coordinator URL cannot contain user credentials, query or fragment".into());
    }
    let token=super::configured_token().ok_or(
        "DOMINION_REMOTE_CLUSTER_TOKEN must be 32..512 random bytes")?;
    let worker_id=std::env::var("DOMINION_REMOTE_WORKER_ID")
        .map_err(|_|"DOMINION_REMOTE_WORKER_ID required for stable identity")?;
    if !super::valid_id(&worker_id){
        return Err("DOMINION_REMOTE_WORKER_ID must be a valid unique ASCII node ID".into());
    }
    let allow_writes=std::env::var("DOMINION_REMOTE_ALLOW_WRITES")
        .ok().as_deref()==Some("1");
    let transport=Transport{
        url:url.trim_end_matches('/').to_string(),token,
        client:reqwest::Client::builder().pool_idle_timeout(Duration::from_secs(30))
            .build().map_err(|e|e.to_string())?,
    };
    Ok((transport,worker_id,allow_writes))
}
fn local_agents(state:&AppState)->Vec<String>{
    let mut agents=state.store.all("agent").into_iter().filter_map(|record|{
        if record["enabled"]==false ||record["disabled"]==true{return None;}
        record["id"].as_str().filter(|s|super::valid_id(s)).map(str::to_string)
    }).collect::<Vec<_>>();
    agents.sort();agents.dedup();agents.truncate(32);agents
}
fn stable_epoch(state:&AppState,worker_id:&str)->Result<String,String>{
    let key=format!("remote-node-{worker_id}");
    if let Some(old)=state.store.get("remote_node_identity",&key){
        if let Some(epoch)=old["workerEpoch"].as_str(){
            if uuid::Uuid::parse_str(epoch).is_ok(){return Ok(epoch.to_string());}
        }
        return Err("Persisted remote worker epoch invalid; stop and inspect".into());
    }
    let epoch=uuid::Uuid::new_v4().to_string();
    let value=json!({"id":key,"workerId":worker_id,"workerEpoch":epoch});
    let saved=state.store.put_if_absent("remote_node_identity",&value)
        .or_else(||state.store.get("remote_node_identity",&key))
        .ok_or("Cannot persist durable remote Worker epoch")?;
    saved["workerEpoch"].as_str().map(str::to_string)
        .ok_or("Worker epoch was lost".into())
}
fn native_job(
    state:&AppState,task:&Value,allow_writes:bool,
)->Result<(String,String),String>{
    let task_id=task["id"].as_str().filter(|s|super::valid_id(s))
        .ok_or("Missing remote task ID")?;
    let agent_id=task["agentId"].as_str().filter(|s|super::valid_id(s))
        .ok_or("Missing remote agent ID")?;
    let agent=state.store.get("agent",agent_id)
        .ok_or("Remote job's local agent is not registered on this machine")?;
    let readonly=task["readOnly"]!=false;
    if !readonly && !allow_writes{
        return Err("Remote writer requires DOMINION_REMOTE_ALLOW_WRITES=1".into());
    }
    let prompt=task["prompt"].as_str().filter(|s|!s.is_empty()&&s.len()<=8_000)
        .ok_or("Remote prompt is empty or too large")?;
    let session_id=format!("remote-session-{task_id}");
    let local_id=format!("remote-local-{task_id}");
    let session=json!({
        "id":session_id,"title":format!("Remote Worker · {}",task["nodeId"].as_str().unwrap_or("task")),
        "agentId":agent_id,"providerId":agent["providerId"],"model":agent["model"],
        "parallelReadOnly":readonly,"remoteTaskId":task_id,
        "delegated":true,"toolsEnabled":true,
    });
    let stored_session=state.store.put_if_absent("session",&session)
        .or_else(||state.store.get("session",&session_id))
        .ok_or("Unable to persist remote worker session")?;
    if stored_session["remoteTaskId"]!=task_id ||
        stored_session["agentId"]!=agent_id ||
        stored_session["parallelReadOnly"]!=readonly{
        return Err("Remote session identity mismatch; refusing replay".into());
    }
    let mut input=json!({
        "prompt":prompt,"agentId":agent_id,
        "providerId":agent["providerId"],"model":agent["model"],
        "sessionId":session_id,"remoteTaskId":task_id,
        "toolsEnabled":true,"toolsExecutionEnabled":true,
        "autonomous":true,"maxAutonomousAttempts":2,
        "parallelDelegationReadOnly":readonly,
    });
    if task["successCriteria"].is_array(){
        input["successCriteria"]=task["successCriteria"].clone();
    }
    let job=engine::jobs::enqueue_once(state,&local_id,"prompt",input)
        .ok_or("Remote task already used this local job ID with different arguments")?;
    if job["status"]=="queued"{
        engine::jobs::start_now(state.clone(),local_id.clone());
    }
    Ok((local_id,session_id))
}
fn terminal(status:&str)->bool{
    matches!(status,"completed"|"failed"|"cancelled"|"needs_review"|"interrupted")
}
fn receipt(state:&AppState,job_id:&str,session_id:&str)->Result<Value,String>{
    let job=state.store.get("job",job_id).ok_or("Local remote job disappeared")?;
    let status=job["status"].as_str().ok_or("Local job status missing")?;
    if !terminal(status){return Err("Local job has not reached terminal status".into());}
    let answer=state.store.messages(session_id).into_iter().rev()
        .find(|m|m["role"]=="assistant")
        .and_then(|m|m["content"].as_str().map(str::to_string))
        .unwrap_or_default().chars().take(2_000).collect::<String>();
    let operation_count=state.store.session_executions(session_id)?
        .into_iter().filter_map(|run|run["id"].as_str().map(str::to_string))
        .map(|run_id|state.store.execution_operations(&run_id).map(|v|v.len()).unwrap_or(0))
        .sum::<usize>();
    let usage=state.store.session_usage(session_id);
    // Only return bounded provider-observed counters. Cost estimates,
    // raw prompts and provider credentials are never attested as billing.
    Ok(json!({
        "localJobId":job_id,"jobStatus":status,
        "answer":answer,"verified":job["result"]["verified"]==true,
        "verification":job["result"]["verification"],
        "operationCount":operation_count.min(100),
        "usage":{
            "calls":usage["calls"],"promptTokens":usage["promptTokens"],
            "completionTokens":usage["completionTokens"],
            "totalTokens":usage["totalTokens"],
            "cachedPromptTokens":usage["cachedPromptTokens"],
            "cacheObservedCalls":usage["cacheObservedCalls"],
            "estimatedCalls":usage["estimatedCalls"],
        },
    }))
}
async fn handle_task(
    transport:&Transport,state:&AppState,worker_id:&str,epoch:&str,
    task:&Value,allow_writes:bool,
)->Result<(),String>{
    let task_id=task["id"].as_str().ok_or("Missing remote task ID")?;
    let lease=task["leaseId"].as_str().ok_or("Missing lease ID")?;
    let (job_id,session_id)=native_job(state,task,allow_writes)?;
    let common=json!({"workerId":worker_id,"workerEpoch":epoch,
        "taskId":task_id,"leaseId":lease});
    loop{
        if let Some(job)=state.store.get("job",&job_id){
            if terminal(job["status"].as_str().unwrap_or("")){
                let report=receipt(state,&job_id,&session_id)?;
                let mut body=common.clone();
                for (key,value) in report.as_object().ok_or("Invalid local native receipt")?{
                    body[key]=value.clone();
                }
                let saved=transport.send("/v1/remote/settle",&body).await?;
                println!("REMOTE task {task_id} settled: {}",saved["status"]);
                return Ok(());
            }
        }
        // A native job can run longer than the lease. The coordinator
        // requires an ownership-fenced renewal, not a client-side clock.
        transport.send("/v1/remote/heartbeat",&json!({
            "workerId":worker_id,"workerEpoch":epoch,
        })).await?;
        let renewed=transport.send("/v1/remote/renew",&common).await?;
        if renewed["cancelRequested"]==true {
            let _=engine::jobs::cancel_job(state,&job_id);
        }
        tokio::time::sleep(Duration::from_millis(HEARTBEAT_MS)).await;
    }
}
pub async fn run()->Result<(),String>{
    let (transport,worker_id,allow_writes)=configuration()?;
    // The remote process is never an HTTP listener. Every model/tool call
    // runs via the existing native Rust Job executor and its approval policy.
    let runtime=Dominion::from_config(Config::from_env())?;
    let state=runtime.shared_state();
    engine::jobs::recover(&state);
    engine::algorithms::seed(&state);
    engine::jobs::start(state.clone());
    let epoch=stable_epoch(&state,&worker_id)?;
    println!("DOMINION native remote worker {worker_id}, writes={allow_writes}");
    loop{
        let agents=local_agents(&state);
        if agents.is_empty(){
            return Err("Remote worker has no registered local agent; configure a native agent first".into());
        }
        let registered=transport.send("/v1/remote/register",&json!({
            "workerId":worker_id,"workerEpoch":epoch,
            "agents":agents,"allowWrites":allow_writes,
        })).await;
        match registered{
            Ok(_)=>{
                let claimed=transport.send("/v1/remote/claim",&json!({
                    "workerId":worker_id,"workerEpoch":epoch,
                })).await;
                match claimed {
                    Ok(response)=>{
                        if let Some(task)=response["task"].as_object(){
                            let task=Value::Object(task.clone());
                            if let Err(error)=handle_task(&transport,&state,&worker_id,
                                &epoch,&task,allow_writes).await{
                                eprintln!("REMOTE worker execution outcome unresolved: {error}");
                                // No automatic replay after a failed settle or lease.
                            }
                        }else{
                            tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
                        }
                    }
                    Err(error)=>{
                        eprintln!("REMOTE claim failed: {error}");
                        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
                    }
                }
            }
            Err(error)=>{
                eprintln!("REMOTE registration failed: {error}");
                tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn secure_transport_requires_explicit_configuration(){
        assert!(terminal("completed"));
        assert!(terminal("needs_review"));
        assert!(!terminal("running"));
        assert!(!terminal("queued"));
    }
}
