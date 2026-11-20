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
const MAX_PRETURN_EVENTS:usize=128;
const MAX_PRETURN_BYTES:usize=2*MAX_LINE;
fn ensure(ctx:&JobContext,started:Instant)->Result<(),String>{
    ctx.ensure_running()?;
    if started.elapsed()>MAX_TIME {Err("External Agent Bridge timed out".into())}
    else {Ok(())}
}
/// Bounded streaming JSONL read. Never call read_line() on an untrusted
/// subprocess: an endless line would allocate memory before the size check.
async fn read(ctx:&JobContext,reader:&mut BufReader<ChildStdout>,
    started:Instant)->Result<Value,String>{
    loop {
        let mut line=Vec::new();
        loop {
            ensure(ctx,started)?;
            let (count,terminated)={
                let chunk=match tokio::time::timeout(Duration::from_millis(300),
                    reader.fill_buf()).await {
                    Ok(Ok(chunk))=>chunk,
                    Ok(Err(error))=>return Err(error.to_string()),
                    Err(_)=>continue,
                };
                if chunk.is_empty() {
                    return Err("External agent stdout closed before complete JSONL event".into());
                }
                let end=chunk.iter().position(|byte|*byte==b'\n')
                    .map(|idx|idx+1).unwrap_or(chunk.len());
                if line.len().saturating_add(end)>MAX_LINE {
                    return Err("External JSONL event over size cap".into());
                }
                line.extend_from_slice(&chunk[..end]);
                (end,chunk[end-1]==b'\n')
            };
            reader.consume(count);
            if terminated {break;}
        }
        if line.iter().all(|b|b.is_ascii_whitespace()){continue;}
        return serde_json::from_slice(&line).map_err(|e|
            format!("External agent sent invalid JSONL: {e}"));
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
/// Notifications can legitimately precede the JSON-RPC response for
/// turn/start. Retain these frames rather than swallowing a finished turn
/// and then waiting forever for a second completion notification.
async fn rpc(ctx:&JobContext,child:&mut Child,reader:&mut BufReader<ChildStdout>,
    started:Instant,id:u64,method:&str,params:Value)->Result<(Value,Vec<Value>),String>{
    send(child,json!({"id":id,"method":method,"params":params})).await?;
    let mut notifications=Vec::new();
    let mut buffered_bytes=0usize;
    for _ in 0..512{
        let event=read(ctx,reader,started).await?;
        if needs_host_approval(&event){
            return Err("External harness requested unmediated approval".into());
        }
        if event["id"]==id {
            if !event["error"].is_null(){return Err(format!("{method}: {}",event["error"]));}
            return Ok((event["result"].clone(),notifications));
        }
        if event["method"].is_string() {
            buffered_bytes=buffered_bytes.saturating_add(event.to_string().len());
            if notifications.len()>=MAX_PRETURN_EVENTS||buffered_bytes>MAX_PRETURN_BYTES {
                return Err("External RPC pre-response event budget exhausted".into());
            }
            notifications.push(event);
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
fn codex_finish(thread_id:&str,completed:bool,answer:&str)->Result<Value,String>{
    if !completed||answer.trim().is_empty(){
        return Err("Codex turn not successfully completed".into());
    }
    Ok(json!({"status":"completed","runtime":"codex",
        "threadId":thread_id,"answer":answer,
        "evidenceGrade":"external_unverified"}))
}
async fn run_codex(ctx:&JobContext,child:&mut Child,started:Instant,
    prompt:&str,model:Option<&str>,workspace:&std::path::Path)->Result<Value,String>{
    let mut reader=BufReader::new(child.stdout.take().ok_or("Codex stdout missing")?);
    rpc(ctx,child,&mut reader,started,1,"initialize",
        json!({"clientInfo":{"name":"dominion","title":"DOMINION","version":"0.3.0"}})).await?;
    send(child,json!({"method":"initialized","params":{}})).await?;
    let mut params=json!({"cwd":workspace.to_string_lossy(),
        "sandbox":"read-only","approvalPolicy":"never"});
    if let Some(model)=model{params["model"]=json!(model);}
    let (thread,_)=rpc(ctx,child,&mut reader,started,2,"thread/start",params).await?;
    let thread_id=thread["thread"]["id"].as_str().filter(|s|!s.is_empty())
        .ok_or("Codex thread ID missing")?.to_string();
    ctx.checkpoint(json!({"bridgeState":{"runtime":"codex",
        "inFlight":true,"phase":"turn","threadId":thread_id}}))?;
    let (_,pre_response)=rpc(ctx,child,&mut reader,started,3,"turn/start",
        json!({"threadId":thread_id,"input":[{"type":"text","text":prompt}]})).await?;
    let mut answer=String::new();
    // A very short turn may finish before the turn/start RPC response.
    for event in pre_response {
        if let Some(completed)=codex_event(&event,&mut answer)? {
            return codex_finish(&thread_id,completed,&answer);
        }
    }
    for _ in 0..4096 {
        let event=read(ctx,&mut reader,started).await?;
        if let Some(completed)=codex_event(&event,&mut answer)? {
            return codex_finish(&thread_id,completed,&answer);
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
    let workspace=crate::features::collaboration::worktree::for_job(&ctx.state,job)?;
    let mut command=Command::new(binary);
    command.current_dir(&workspace)
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
        run_codex(ctx,&mut child,started,prompt,job["input"]["model"].as_str(),&workspace).await
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
    #[test] fn early_turn_completion_is_not_lost_before_rpc_ack(){
        // turn/start can produce an agent message and terminal notification
        // before its own RPC acknowledgement; the buffered order is preserved.
        let buffered=vec![
            json!({"method":"item/agentMessage/delta","params":{"delta":"proof"}}),
            json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}),
        ];
        let mut answer=String::new();
        let mut final_status=None;
        for frame in buffered {
            if let Some(status)=codex_event(&frame,&mut answer).unwrap(){
                final_status=Some(status);
                break;
            }
        }
        let result=codex_finish("thread1",final_status.unwrap(),&answer).unwrap();
        assert_eq!(result["answer"],"proof");
        assert_eq!(result["evidenceGrade"],"external_unverified");
    }
    #[test] fn incomplete_codex_turn_must_not_unlock_team_dependency(){
        assert!(codex_finish("thread1",true," ").is_err());
        assert!(codex_finish("thread1",false,"false success").is_err());
    }
    #[tokio::test]
    async fn real_jsonl_pipe_preserves_completion_before_turn_start_rpc_response(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "dominion-codex-fake-server-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"fake-app-server-job",
            "kind":"external_agent","status":"running","attempts":1})).unwrap();
        let context=JobContext::new(state.clone(),"fake-app-server-job".into());

        // A real subprocess running the app-server's JSONL protocol, not a
        // mocked Rust callback. The terminal event deliberately precedes the
        // turn/start RPC result to cover a formerly lost-completion race.
        let mock=r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"id":1,"result":{}}'
      ;;
    *'"method":"thread/start"'*)
      printf '%s\n' '{"method":"thread/started","params":{}}'
      printf '%s\n' '{"id":2,"result":{"thread":{"id":"fixture-thread"}}}'
      ;;
    *'"method":"turn/start"'*)
      printf '%s\n' '{"method":"item/agentMessage/delta","params":{"delta":"verified JSONL exchange"}}'
      printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
      printf '%s\n' '{"id":3,"result":{"turn":{"id":"fixture-turn"}}}'
      ;;
  esac
done
"#;
        let mut child=Command::new("sh").arg("-c").arg(mock)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .kill_on_drop(true).spawn().unwrap();
        let output=tokio::time::timeout(Duration::from_secs(8),
            run_codex(&context,&mut child,Instant::now(),"Inspect fixture",None,&context.state.config.workspace))
            .await.expect("The buffered finish must not hang").unwrap();
        assert_eq!(output["status"],"completed");
        assert_eq!(output["answer"],"verified JSONL exchange");
        assert_eq!(output["threadId"],"fixture-thread");
        assert_eq!(output["evidenceGrade"],"external_unverified");
        let _=child.kill().await;
        crate::test_support::cleanup_dir(&root);
    }

    #[tokio::test]
    async fn overlong_external_jsonl_is_rejected_before_a_full_line_allocates(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "dominion-codex-line-cap-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"bounded-jsonl-job",
            "kind":"external_agent","status":"running","attempts":1})).unwrap();
        let context=JobContext::new(state.clone(),"bounded-jsonl-job".into());
        let mut child=Command::new("sh").arg("-c")
            .arg("printf '%1048577s\n' x")
            .stdout(Stdio::piped()).stderr(Stdio::null())
            .kill_on_drop(true).spawn().unwrap();
        let mut reader=BufReader::new(child.stdout.take().unwrap());
        let failure=tokio::time::timeout(Duration::from_secs(8),
            read(&context,&mut reader,Instant::now()))
            .await.expect("Bounded parser must reject without waiting for EOF")
            .unwrap_err();
        assert!(failure.contains("size cap"),"{failure}");
        let _=child.kill().await;
        crate::test_support::cleanup_dir(&root);
    }
    #[test] fn unmediated_approval_request_is_denied(){
        let mut text=String::new();
        assert!(codex_event(&json!({"id":3,
            "method":"item/commandExecution/requestApproval"}),&mut text).is_err());
        assert!(text.is_empty());
    }
}
