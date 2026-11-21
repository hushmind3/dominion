//! Opt-in remote text inference worker. Runs on the REMOTE machine and polls
//! an owner-controlled private endpoint through TLS or a localhost tunnel.
use reqwest::{Client,Url};
use serde_json::{json,Value};
use futures_util::StreamExt;
use std::time::Duration;
const MAX_GATEWAY_RESPONSE_BYTES:usize=64*1024;
const MAX_MODEL_RESPONSE_BYTES:usize=1024*1024;

async fn bounded_json(response:reqwest::Response,limit:usize,label:&str)
    ->Result<Value,String>{
    if response.content_length().is_some_and(|size|size>limit as u64){
        return Err(format!("{label} response exceeds size bound"));
    }
    let mut stream=response.bytes_stream();
    let mut body=Vec::new();
    while let Some(chunk)=stream.next().await{
        let chunk=chunk.map_err(|e|format!("{label} response interrupted: {e}"))?;
        if body.len().saturating_add(chunk.len())>limit{
            return Err(format!("{label} response exceeds size bound"));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|e|format!("Invalid {label} JSON: {e}"))
}

fn remote_url(raw:&str)->Result<Url,String>{
    let url=Url::parse(raw).map_err(|e|e.to_string())?;
    let allowed= url.scheme()=="https"
        || (url.scheme()=="http"
           &&matches!(url.host_str(),Some("127.0.0.1"|"localhost"|"::1")));
    if !allowed||url.username()!=""||url.password().is_some()||
       url.query().is_some()||url.fragment().is_some(){
        return Err("Remote gateway must use HTTPS or a local SSH tunnel".into());
    }
    Ok(url)
}
fn endpoint(base:&Url,path:&str)->Result<Url,String>{
    base.join(path).map_err(|e|e.to_string())
}
async fn gateway_post(client:&Client,base:&Url,token:&str,path:&str,
    body:&Value)->Result<Value,String>{
    let response=client.post(endpoint(base,path)?)
        .header("x-dominion-worker-token",token)
        .json(body).send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){
        return Err(format!("Worker gateway rejected {}: HTTP {}",path,response.status()));
    }
    bounded_json(response,MAX_GATEWAY_RESPONSE_BYTES,"Remote gateway").await
}
async fn model_infer(client:&Client,prompt:&str,max_tokens:u64)->Result<(String,Value),String>{
    let endpoint=std::env::var("DOMINION_WORKER_MODEL_ENDPOINT")
        .map_err(|_|"Set DOMINION_WORKER_MODEL_ENDPOINT to an OpenAI-compatible chat/completions URL")?;
    let url=remote_url(&endpoint)?;
    let model=std::env::var("DOMINION_WORKER_MODEL").map_err(|_|
        "Set DOMINION_WORKER_MODEL to the remote machine's local/API model ID")?;
    if model.is_empty()||model.len()>160{return Err("Invalid worker model ID".into());}
    let mut request=client.post(url)
        .json(&json!({"model":model,
            "messages":[{"role":"user","content":prompt}],
            "max_tokens":max_tokens.clamp(64,4096),"stream":false}));
    if let Ok(key)=std::env::var("DOMINION_WORKER_MODEL_KEY"){
        if !key.is_empty(){request=request.bearer_auth(key);}
    }
    let response=request.send().await.map_err(|e|format!("Remote inference request: {e}"))?;
    if !response.status().is_success(){
        return Err(format!("Model endpoint HTTP {}",response.status()));
    }
    let output=bounded_json(response,MAX_MODEL_RESPONSE_BYTES,"Worker model").await?;
    let answer=output["choices"][0]["message"]["content"]
        .as_str().ok_or("Missing text completion from worker model")?;
    if answer.trim().is_empty(){return Err("Worker model returned empty text".into());}
    Ok((answer.chars().take(16_384).collect(),output["usage"].clone()))
}
pub async fn run_cli(_args:&[String])->Result<(),String>{
    let base=remote_url(&std::env::var("DOMINION_REMOTE_GATEWAY")
        .map_err(|_|"Set DOMINION_REMOTE_GATEWAY to a private HTTPS/tunnel URL")?)?;
    let token=std::env::var("DOMINION_REMOTE_WORKER_TOKEN")
        .map_err(|_|"Set DOMINION_REMOTE_WORKER_TOKEN")?;
    if token.len()<32||token.len()>1024||!token.is_ascii()||
       token.chars().any(char::is_whitespace){
        return Err("Worker token must be 32-1024 non-whitespace ASCII bytes".into());
    }
    let worker_id=std::env::var("DOMINION_REMOTE_WORKER_ID")
        .unwrap_or_else(|_|format!("worker-{}",uuid::Uuid::new_v4()));
    if worker_id.len()>128||worker_id.is_empty()
        ||!worker_id.bytes().all(|b|b.is_ascii_alphanumeric()||
            matches!(b,b'-'|b'.'|b'_')){
        return Err("Invalid DOMINION_REMOTE_WORKER_ID".into());
    }
    let client=Client::builder().timeout(Duration::from_secs(120))
        .build().map_err(|e|e.to_string())?;
    // The gateway credential uses a custom header, which reqwest's default
    // cross-host redirect filtering does not recognize as sensitive. Never
    // forward it to a redirect target. Keep inference redirects independent.
    let gateway_client=Client::builder().timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e|e.to_string())?;
    // Preflight local inference first: never claim work that cannot run.
    if std::env::var("DOMINION_WORKER_MODEL_ENDPOINT").is_err()
        || std::env::var("DOMINION_WORKER_MODEL").is_err(){
        return Err("Worker inference endpoint and model must be configured".into());
    }
    let local_model=std::env::var("DOMINION_WORKER_MODEL").unwrap_or_default();
    gateway_post(&gateway_client,&base,&token,"v1/worker/hello",
        &json!({"workerId":worker_id,"model":local_model})).await?;
    eprintln!("DOMINION remote worker {} connected (text-only, no tools)",worker_id);
    loop{
        let response=gateway_post(&gateway_client,&base,&token,"v1/worker/claim",
            &json!({"workerId":worker_id,"model":local_model})).await?;
        if let Some(task)=response["task"].as_object(){
            let id=task.get("taskId").and_then(Value::as_str).ok_or("Missing task identity")?;
            let lease=task.get("leaseId").and_then(Value::as_str).ok_or("Missing lease")?;
            let prompt=task.get("prompt").and_then(Value::as_str).ok_or("Missing prompt")?;
            let max_tokens=task.get("maxTokens").and_then(Value::as_u64).unwrap_or(2048);
            // Keep the remote lease alive during a slow generation and
            // observe owner cancellation without waiting for the model's
            // entire HTTP timeout. Dropping the inference future aborts its
            // outbound HTTP operation; no speculative retry is launched.
            let inference=model_infer(&client,prompt,max_tokens);
            tokio::pin!(inference);
            let outcome=loop{
                tokio::select!{
                    result=&mut inference=>break result,
                    _=tokio::time::sleep(Duration::from_secs(12))=>{
                        gateway_post(&gateway_client,&base,&token,"v1/worker/heartbeat",
                            &json!({"taskId":id,"leaseId":lease,"workerId":worker_id}))
                            .await?;
                    }
                }
            };
            let (status,answer,usage)=match outcome{
                Ok((answer,usage))=>("completed",answer,usage),
                Err(error)=>("needs_review",error,Value::Null)
            };
            // No replay on network ambiguity: the lease is single-use and
            // the coordinator will place an expired unknown result in review.
            gateway_post(&gateway_client,&base,&token,"v1/worker/complete",
                &json!({"taskId":id,"leaseId":lease,"workerId":worker_id,
                    "status":status,"answer":answer,"usage":usage})).await?;
        }else{
            tokio::select!{
                _=tokio::signal::ctrl_c()=>return Ok(()),
                _=tokio::time::sleep(Duration::from_secs(3))=>{}
            }
        }
    }
}
/// Dispatch a verified text-only Team child through the same durable
/// remote-task ledger. A restart does not create another paid inference.
pub async fn run_team_read(ctx:&crate::engine::jobs::JobContext,job:&Value)
    ->Result<Value,String>{
    if std::env::var("DOMINION_REMOTE_WORKERS").ok().as_deref()!=Some("1"){
        return Err("needs_review: remote worker gateway is disabled".into());
    }
    if job["kind"]!="external_agent"
        ||job["input"]["bridgeRuntime"]!="remote"
        ||job["input"]["teamMode"]!="read"
        ||job["input"]["parentRunId"].as_str().is_none()
        ||job["input"]["teamTaskId"].as_str().is_none() {
        return Err("needs_review: invalid read-only remote Team binding".into());
    }
    let prompt=job["input"]["prompt"].as_str().filter(|s|
        !s.trim().is_empty()&&s.len()<=16_384)
        .ok_or("needs_review: remote prompt exceeds bound")?;
    let child_id=job["id"].as_str().ok_or("Missing Team child ID")?;
    let remote_id=format!("remote-{child_id}");
    if remote_id.len()>200{return Err("Remote Team task identity too long".into());}
    if let Some(existing)=ctx.state.store.get("remote_task",&remote_id){
        if existing["prompt"]!=prompt ||existing["teamChildJobId"]!=child_id ||
            existing["parentRunId"]!=job["input"]["parentRunId"] {
            return Err("needs_review: remote replay binding changed".into());
        }
    }else{
        ctx.ensure_running()?;
        let ticket=json!({
            "id":remote_id,"kind":"remote_inference","status":"queued","phase":"queued",
            "prompt":prompt,"maxTokens":2048,"attempts":0,
            "queuedAt":chrono::Utc::now().to_rfc3339(),
            "capability":"text_inference","evidenceGrade":"remote_unverified",
            "teamChildJobId":child_id,
            "parentRunId":job["input"]["parentRunId"],
            "teamTaskId":job["input"]["teamTaskId"]
        });
        ctx.state.store.put("remote_task",&ticket)
            .ok_or("Cannot persist remote Team dispatch")?;
        ctx.state.bus.record("remote_task",&ticket);
    }
    let started=std::time::Instant::now();
    loop{
        if ctx.ensure_running().is_err(){
            let _=ctx.state.store.cancel_remote_task(&remote_id);
            return Err("cancelled: remote Team parent no longer owns execution".into());
        }
        let Some(saved)=ctx.state.store.get("remote_task",&remote_id) else{
            return Err("needs_review: remote task disappeared".into());
        };
        match saved["status"].as_str(){
            Some("completed")=>{
                let answer=saved["result"]["answer"].as_str().unwrap_or("");
                if answer.is_empty(){return Err("needs_review: remote answer is empty".into());}
                return Ok(json!({"status":"completed","answer":answer,
                    "remoteTaskId":remote_id,"evidenceGrade":"external_unverified",
                    "verified":false,
                    "usage":saved["result"]["usage"]}));
            },
            Some("needs_review")=>return Ok(json!({
                "status":"needs_review","remoteTaskId":remote_id,
                "reason":saved["reviewReason"],
                "evidenceGrade":"external_unverified","verified":false
            })),
            Some("cancelled")=>return Err("cancelled: remote task stopped".into()),
            Some("queued"|"running")=>{},
            _=>return Err("needs_review: unexpected remote task status".into())
        }
        if started.elapsed()>Duration::from_secs(12*60){
            // A hung remote lease is a reviewable external outcome, not a
            // license to enqueue another model request.
            return Ok(json!({"status":"needs_review","remoteTaskId":remote_id,
                "reason":"Remote Team result pending beyond supervisor budget"}));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn private_endpoint_uses_https_or_local_tunnel(){
        assert!(remote_url("https://worker.example.com/v1/").is_ok());
        assert!(remote_url("http://127.0.0.1:7410/").is_ok());
        assert!(remote_url("http://localhost:7410/").is_ok());
        assert!(remote_url("http://worker.example.com/").is_err());
        assert!(remote_url("ftp://localhost/").is_err());
        assert!(remote_url("https://a:b@worker.example.com/").is_err());
    }
}
