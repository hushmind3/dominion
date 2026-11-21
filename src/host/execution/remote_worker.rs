//! Opt-in remote text inference worker. Runs on the REMOTE machine and polls
//! an owner-controlled private endpoint through TLS or a localhost tunnel.
use reqwest::{Client,Url};
use serde_json::{json,Value};
use std::time::Duration;
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
    response.json().await.map_err(|e|e.to_string())
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
    let output:Value=response.json().await.map_err(|e|e.to_string())?;
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
    // Preflight local inference first: never claim work that cannot run.
    if std::env::var("DOMINION_WORKER_MODEL_ENDPOINT").is_err()
        || std::env::var("DOMINION_WORKER_MODEL").is_err(){
        return Err("Worker inference endpoint and model must be configured".into());
    }
    eprintln!("DOMINION remote worker {} connected (text-only, no tools)",worker_id);
    loop{
        let response=gateway_post(&client,&base,&token,"v1/worker/claim",
            &json!({"workerId":worker_id})).await?;
        if let Some(task)=response["task"].as_object(){
            let id=task.get("taskId").and_then(Value::as_str).ok_or("Missing task identity")?;
            let lease=task.get("leaseId").and_then(Value::as_str).ok_or("Missing lease")?;
            let prompt=task.get("prompt").and_then(Value::as_str).ok_or("Missing prompt")?;
            let max_tokens=task.get("maxTokens").and_then(Value::as_u64).unwrap_or(2048);
            let outcome=model_infer(&client,prompt,max_tokens).await;
            let (status,answer,usage)=match outcome{
                Ok((answer,usage))=>("completed",answer,usage),
                Err(error)=>("needs_review",error,Value::Null)
            };
            // No replay on network ambiguity: the lease is single-use and
            // the coordinator will place an expired unknown result in review.
            gateway_post(&client,&base,&token,"v1/worker/complete",
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
