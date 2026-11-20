//! Opt-in independent Agent Bridge. Codex App Server and Claude CLI are NOT
//! ModelProviders, and external success is NOT a native tool receipt.
//! Both adapters are read-only until an external approval bridge exists.
use crate::engine::jobs::JobContext;
use serde_json::{json,Value};
use std::{process::Stdio,time::{Duration,Instant}};
use tokio::{io::{AsyncBufReadExt,AsyncReadExt,AsyncWriteExt,BufReader},
    process::{Child,ChildStdout,Command}};
const MAX_TIME:Duration=Duration::from_secs(3600);
const MAX_LINE:usize=1_048_576;
const MAX_ANSWER:usize=24_000;
fn ensure(ctx:&JobContext,started:Instant)->Result<(),String>{
    ctx.ensure_running()?;
    if started.elapsed()>MAX_TIME {Err("External Agent Bridge timed out".into())}
    else {Ok(())}
}
async fn read(ctx:&JobContext,reader:&mut BufReader<ChildStdout>,
    started:Instant)->Result<Value,String>{
    loop{
        ensure(ctx,started)?;
        let mut line=String::new();
        match tokio::time::timeout(Duration::from_millis(300),
            reader.read_line(&mut line)).await{
            Ok(Ok(0))=>return Err("External agent exited before completion".into()),
            Ok(Ok(_))=>{
                if line.len()>MAX_LINE {return Err("External event over size cap".into());}
                if line.trim().is_empty(){continue;}
                return serde_json::from_str(&line).map_err(|e|e.to_string());
            },
            Ok(Err(e))=>return Err(e.to_string()),
            Err(_)=>{}
        }
    }
}
async fn send(child:&mut Child,value:Value)->Result<(),String>{
    let mut line=value.to_string().into_bytes();
    line.push(b'\n');
    let stdin=child.stdin.as_mut().ok_or("Missing external stdin")?;
    tokio::time::timeout(Duration::from_secs(10),async{
        stdin.write_all(&line).await.map_err(|e|e.to_string())?;
        stdin.flush().await.map_err(|e|e.to_string())
    }).await.map_err(|_|"External stdin timeout".to_string())?
}
fn needs_host_approval(event:&Value)->bool{
    event.get("id").is_some()&&event.get("method").is_some()
}
async fn rpc(ctx:&JobContext,child:&mut Child,reader:&mut BufReader<ChildStdout>,
    started:Instant,id:u64,method:&str,params:Value)->Result<Value,String>{
    send(child,json!({"id":id,"method":method,"params":params})).await?;
    for _ in 0..512{
        let event=read(ctx,reader,started).await?;
        if needs_host_approval(&event){
            return Err("External harness requested unmediated approval".into());
        }
        if event["id"]==id {
            if !event["error"].is_null(){return Err(format!("{method}: {}",event["error"]));}
            return Ok(event["result"].clone());
        }
    }
    Err("External RPC response budget exhausted".into())
}
fn codex_event(frame:&Value,answer:&mut String)->Result<Option<bool>,String>{
    if needs_host_approval(frame){
        return Err("External approval request denied".into());
    }
    match frame["method"].as_str(){
        Some("item/agentMessage/delta")=>{
            if let Some(delta)=frame["params"]["delta"].as_str(){
                if answer.chars().count()+delta.chars().count()>MAX_ANSWER{
                    return Err("External answer over size cap".into());
                }
                answer.push_str(delta);
            }
        },
        Some("item/completed") if frame["params"]["item"]["type"]=="agentMessage"=>{
            if answer.is_empty(){
                if let Some(text)=frame["params"]["item"]["text"].as_str(){
                    answer.extend(text.chars().take(MAX_ANSWER));
                }
            }
        },
        Some("turn/completed")=>{
            return Ok(Some(frame["params"]["turn"]["status"]=="completed"));
        },
        _=>{}
    }
    Ok(None)
}
async fn run_codex(ctx:&JobContext,child:&mut Child,started:Instant,
    prompt:&str,model:Option<&str>)->Result<Value,String>{
    let mut reader=BufReader::new(child.stdout.take().ok_or("Codex stdout missing")?);
    rpc(ctx,child,&mut reader,started,1,"initialize",
        json!({"clientInfo":{"name":"dominion","title":"DOMINION","version":"0.3.0"}})).await?;
    send(child,json!({"method":"initialized","params":{}})).await?;
    let mut params=json!({"cwd":ctx.state.config.workspace.to_string_lossy(),
        "sandbox":"read-only","approvalPolicy":"never"});
    if let Some(model)=model{params["model"]=json!(model);}
    let thread=rpc(ctx,child,&mut reader,started,2,"thread/start",params).await?;
    let thread_id=thread["thread"]["id"].as_str().filter(|s|!s.is_empty())
        .ok_or("Codex thread ID missing")?.to_string();
    ctx.checkpoint(json!({"bridgeState":{"runtime":"codex",
        "inFlight":true,"phase":"turn","threadId":thread_id}}))?;
    rpc(ctx,child,&mut reader,started,3,"turn/start",
        json!({"threadId":thread_id,"input":[{"type":"text","text":prompt}]})).await?;
    let mut answer=String::new();
    for _ in 0..4096 {
        let event=read(ctx,&mut reader,started).await?;
        if let Some(completed)=codex_event(&event,&mut answer)? {
            if !completed || answer.trim().is_empty(){
                return Err("Codex turn not successfully completed".into());
            }
            return Ok(json!({"status":"completed","runtime":"codex",
                "threadId":thread_id,"answer":answer,
                "evidenceGrade":"external_unverified"}));
        }
    }
    Err("Codex event budget exhausted".into())
}
async fn run_claude(ctx:&JobContext,child:&mut Child,started:Instant)->Result<Value,String>{
    let stdout=child.stdout.take().ok_or("Claude stdout missing")?;
    let read=tokio::spawn(async move{
        let mut bytes=Vec::new();
        stdout.take((MAX_LINE+1) as u64).read_to_end(&mut bytes).await
            .map_err(|e|e.to_string())?;
        Ok::<_,String>(bytes)
    });
    let status=loop{
        ensure(ctx,started)?;
        match tokio::time::timeout(Duration::from_millis(300),child.wait()).await{
            Ok(Ok(status))=>break status,
            Ok(Err(error))=>return Err(error.to_string()),
            Err(_)=>{}
        }
    };
    let bytes=read.await.map_err(|e|e.to_string())??;
    if bytes.len()>MAX_LINE{return Err("Claude output over size cap".into());}
    if !status.success(){return Err(format!("Claude exited with {status}"));}
    let output:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
    if output["is_error"]==true{return Err("Claude reported a run error".into());}
    let text=output["result"].as_str().filter(|s|!s.trim().is_empty())
        .ok_or("Claude returned no final result")?;
    Ok(json!({"status":"completed","runtime":"claude",
        "sessionId":output["session_id"],
        "answer":text.chars().take(MAX_ANSWER).collect::<String>(),
        "evidenceGrade":"external_unverified"}))
}
pub async fn run(ctx:&JobContext,job:&Value)->Result<Value,String>{
    if std::env::var("DOMINION_EXTERNAL_AGENTS").ok().as_deref()!=Some("1"){
        return Err("External Agent Bridge requires DOMINION_EXTERNAL_AGENTS=1".into());
    }
    let runtime=job["input"]["bridgeRuntime"].as_str()
        .filter(|s|matches!(*s,"codex"|"claude"))
        .ok_or("Invalid external agent runtime")?;
    if job["input"]["teamMode"]!="read"{
        return Err("External write requires a separate approval bridge".into());
    }
    let prompt=job["input"]["prompt"].as_str().filter(|s|
        !s.trim().is_empty()&&s.len()<=32_000).ok_or("External prompt missing")?;
    let env_name=if runtime=="codex"{"DOMINION_CODEX_BINARY"}
        else{"DOMINION_CLAUDE_BINARY"};
    let binary=std::env::var(env_name).ok().filter(|s|!s.is_empty())
        .unwrap_or_else(||runtime.into());
    let mut command=Command::new(binary);
    command.current_dir(&ctx.state.config.workspace)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true);
    if runtime=="codex"{command.arg("app-server");}
    else {
        command.args(["--print","--output-format","json",
            "--permission-mode","plan","--tools","Read,Glob,Grep"]);
        if let Some(model)=job["input"]["model"].as_str(){
            command.arg("--model").arg(model);
        }
        command.arg("--").arg(prompt);
    }
    ctx.ensure_running()?;
    ctx.checkpoint(json!({"bridgeState":{"inFlight":true,
        "runtime":runtime,"phase":"launching","evidenceGrade":"external_unverified"}}))?;
    let mut child=match command.spawn(){
        Ok(child)=>child,
        Err(e)=>{
            ctx.checkpoint(json!({"bridgeState":{"inFlight":false,
                "runtime":runtime,"phase":"spawn_failed"}}))?;
            return Err(format!("External agent spawn failed: {e}"));
        }
    };
    let started=Instant::now();
    let outcome=if runtime=="codex"{
        run_codex(ctx,&mut child,started,prompt,job["input"]["model"].as_str()).await
    }else{run_claude(ctx,&mut child,started).await};
    let _=child.kill().await;
    match outcome{
        Ok(value)=>{
            ctx.checkpoint(json!({"bridgeState":{"inFlight":false,
                "runtime":runtime,"phase":"completed",
                "threadId":value["threadId"],"sessionId":value["sessionId"],
                "evidenceGrade":"external_unverified"}}))?;
            Ok(value)
        },
        Err(reason)=>{
            let _=ctx.checkpoint(json!({"bridgeState":{"inFlight":false,
                "runtime":runtime,"phase":"needs_review",
                "outcomeUnknown":true,"reason":reason}}));
            Ok(json!({"status":"needs_review","reason":reason,
                "outcomeUnknown":true,"evidenceGrade":"external_unverified"}))
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn codex_notifications_do_not_promote_failed_turns(){
        let mut text=String::new();
        assert_eq!(codex_event(&json!({"method":"item/agentMessage/delta",
            "params":{"delta":"hello"}}),&mut text).unwrap(),None);
        assert_eq!(text,"hello");
        assert_eq!(codex_event(&json!({"method":"turn/completed",
            "params":{"turn":{"status":"failed"}}}),&mut text).unwrap(),Some(false));
        assert_eq!(codex_event(&json!({"method":"turn/completed",
            "params":{"turn":{"status":"completed"}}}),&mut text).unwrap(),Some(true));
    }
    #[test] fn unmediated_approval_request_is_denied(){
        let mut text=String::new();
        assert!(codex_event(&json!({"id":3,
            "method":"item/commandExecution/requestApproval"}),&mut text).is_err());
        assert!(text.is_empty());
    }
}
