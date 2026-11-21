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
fn codex_denial(method:&str)->Option<Value>{
    match method{
        "item/commandExecution/requestApproval"|
        "item/fileChange/requestApproval"|
        "item/permissions/requestApproval" => Some(json!({"decision":"decline"})),
        _=>None
    }
}
/// Codex app-server's current JSON-RPC approval vocabulary is explicit:
/// decline supported write/shell/network requests. This keeps the native
/// Codex turn alive while preserving the owner's read-only grant. Never
/// convert a model's approval request into host tool authority.
async fn decline_codex_request(
    ctx:&JobContext,child:&mut Child,event:&Value
)->Result<bool,String>{
    let Some(method)=event["method"].as_str() else{return Ok(false)};
    let Some(result)=codex_denial(method) else{return Ok(false)};
    ctx.ensure_running()?;
    send(child,json!({"id":event["id"],"result":result})).await?;
    ctx.checkpoint(json!({
        "bridgeLastDeniedApproval":{"method":method,
            "at":chrono::Utc::now().to_rfc3339()},
        "bridgeAuthority":"read_only"
    }))?;
    Ok(true)
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
            if decline_codex_request(ctx,child,&event).await?{continue;}
            return Err("External harness requested unknown unmediated approval".into());
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
fn thread_request(resume:Option<&str>,workspace:&std::path::Path,
    model:Option<&str>)->(&'static str,Value){
    let mut params=json!({"cwd":workspace.to_string_lossy(),
        "sandbox":"read-only","approvalPolicy":"never"});
    if let Some(name)=model{params["model"]=json!(name);}
    if let Some(id)=resume{
        params["threadId"]=json!(id);
        ("thread/resume",params)
    }else{("thread/start",params)}
}
async fn run_codex(ctx:&JobContext,child:&mut Child,started:Instant,
    prompt:&str,model:Option<&str>,workspace:&std::path::Path,
    resume:Option<&str>)->Result<Value,String>{
    let mut reader=BufReader::new(child.stdout.take().ok_or("Codex stdout missing")?);
    rpc(ctx,child,&mut reader,started,1,"initialize",
        json!({"clientInfo":{"name":"dominion","title":"DOMINION","version":"0.3.0"}})).await?;
    send(child,json!({"method":"initialized","params":{}})).await?;
    let (method,params)=thread_request(resume,workspace,model);
    let (thread,_)=rpc(ctx,child,&mut reader,started,2,method,params).await?;
    let thread_id=thread["thread"]["id"].as_str().filter(|s|!s.is_empty())
        .ok_or("Codex thread ID missing")?.to_string();
    if resume.is_some_and(|id|id!=thread_id){
        return Err("Codex resumed a different thread; refuse to launch".into());
    }
    if resume.is_some()&&(thread["thread"]["status"]=="active"
        ||thread["thread"]["status"]["type"]=="active"){
        return Err("Codex thread is still active; owner review required".into());
    }
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
        if needs_host_approval(&event){
            if decline_codex_request(ctx,child,&event).await?{continue;}
            return Err("Codex requested an unsupported external authority".into());
        }
        if let Some(completed)=codex_event(&event,&mut answer)? {
            return codex_finish(&thread_id,completed,&answer);
        }
    }
    Err("Codex event budget exhausted".into())
}
/// Read stdout and process exit concurrently. A child producing more than
/// 1MB cannot block forever on a full pipe while the supervisor awaits exit.
/// Result bytes are capped before any JSON parsing or subsequent model cost.
async fn bounded_json_cli(ctx:&JobContext,child:&mut Child,
    started:Instant,label:&str)->Result<Value,String>{
    let stdout=child.stdout.take().ok_or_else(||format!("{label} stdout missing"))?;
    let mut reader=tokio::spawn(async move{
        let mut bytes=Vec::new();
        stdout.take((MAX_LINE+1) as u64).read_to_end(&mut bytes).await
            .map_err(|e|e.to_string())?;
        Ok::<Vec<u8>,String>(bytes)
    });
    let mut status=None;
    let mut output=None;
    loop {
        if let Err(reason)=ensure(ctx,started){
            reader.abort();
            return Err(reason);
        }
        tokio::select!{
            result=&mut reader, if output.is_none()=>{
                let bytes=result.map_err(|e|e.to_string())??;
                if bytes.len()>MAX_LINE {
                    return Err(format!("{label} stdout exceeded 1MB hard limit"));
                }
                output=Some(bytes);
            },
            result=child.wait(), if status.is_none()=>{
                status=Some(result.map_err(|e|e.to_string())?);
            },
            _=tokio::time::sleep(Duration::from_millis(300))=>{},
        }
        if let (Some(status),Some(bytes))=(status.as_ref(),output.as_ref()){
            if !status.success(){return Err(format!("{label} exited with {status}"));}
            return serde_json::from_slice(bytes)
                .map_err(|_|format!("{label} returned invalid JSON output"));
        }
    }
}
async fn run_claude(ctx:&JobContext,child:&mut Child,started:Instant)->Result<Value,String>{
    let output=bounded_json_cli(ctx,child,started,"Claude").await?;
    if output["is_error"]==true{return Err("Claude reported a run error".into());}
    let text=output["result"].as_str().filter(|s|!s.trim().is_empty())
        .ok_or("Claude returned no final result")?;
    Ok(json!({"status":"completed","runtime":"claude",
        "sessionId":output["session_id"],
        "answer":text.chars().take(MAX_ANSWER).collect::<String>(),
        "evidenceGrade":"external_unverified"}))
}

/// Qwen Code headless JSON is an ARRAY of events (system, assistant, result),
/// not the Claude Code result OBJECT. Only a terminal successful result
/// is accepted; assistant text alone cannot certify completion.
fn qwen_result(value:&Value)->Result<Value,String>{
    let events=value.as_array().ok_or("Qwen headless output is not a JSON event array")?;
    if events.len()>4096{return Err("Qwen event count exceeded limit".into());}
    let last=events.iter().rev().find(|event|event["type"]=="result")
        .ok_or("Qwen returned no terminal result event")?;
    if last["subtype"]!="success"||last["is_error"]==true{
        return Err("Qwen reported an unsuccessful headless run".into());
    }
    let text=last["result"].as_str().filter(|v|!v.trim().is_empty())
        .ok_or("Qwen returned an empty final result")?;
    if text.chars().count()>MAX_ANSWER{
        return Err("Qwen final answer exceeded size cap".into());
    }
    let session=last["session_id"].as_str().filter(|v|!v.trim().is_empty());
    Ok(json!({"status":"completed","runtime":"qwen",
        "sessionId":session,"answer":text,
        "evidenceGrade":"external_unverified"}))
}
async fn run_qwen(ctx:&JobContext,child:&mut Child,started:Instant)->Result<Value,String>{
    let output=bounded_json_cli(ctx,child,started,"Qwen").await?;
    qwen_result(&output)
}
fn qwen_arguments(prompt:&str,resume:Option<&str>,model:Option<&str>)->Vec<String>{
    // Qwen's plan mode is documented as analysis-only (no write or shell).
    // safe-mode disables user hooks/extensions/MCP that may otherwise execute
    // independently of the agent's tool approval policy.
    let mut args=vec!["--prompt".into(),prompt.into(),
        "--output-format".into(),"json".into(),
        "--approval-mode".into(),"plan".into(),
        "--safe-mode".into(),
        "--max-session-turns".into(),"8".into(),
        "--max-wall-time".into(),"120".into()];
    if let Some(model)=model{args.extend(["--model".into(),model.into()]);}
    if let Some(id)=resume{args.extend(["--resume".into(),id.into()]);}
    args
}

pub async fn bridge_preflight()->Value{
    // No authentication or paid call is attempted by this capability probe.
    let enabled=std::env::var("DOMINION_EXTERNAL_AGENTS")
        .ok().as_deref()==Some("1");
    let mut bridges=serde_json::Map::new();
    for runtime in ["codex","claude","qwen"]{
        let key=match runtime{
            "codex"=>"DOMINION_CODEX_BINARY",
            "claude"=>"DOMINION_CLAUDE_BINARY",
            _=>"DOMINION_QWEN_BINARY",
        };
        let binary=std::env::var(key).ok().filter(|v|!v.is_empty())
            .unwrap_or_else(||runtime.into());
        let output=if enabled{
            tokio::time::timeout(Duration::from_secs(3), async {
                let mut command=Command::new(&binary);
                command.arg("--version").kill_on_drop(true);
                crate::integrations::local_tools::run_command(&mut command,Duration::from_secs(3)).await
            })
                .await.ok().and_then(Result::ok)
        }else{None};
        let available=output.as_ref().is_some_and(|o|o.status.success());
        let version=output.as_ref().filter(|o|o.status.success())
            .map(|o|String::from_utf8_lossy(&o.stdout).chars().take(120)
                .collect::<String>());
        bridges.insert(runtime.into(),json!({"runtime":runtime,
            "kind":"independent_agent","enabled":enabled,
            "binaryAvailable":available,"version":version,
            "authentication":"unknown","executionPermission":"read_only"}));
    }
    json!({"version":1,"bridges":bridges,"paidRunPerformed":false})
}
pub async fn run(ctx:&JobContext,job:&Value)->Result<Value,String>{
    if std::env::var("DOMINION_EXTERNAL_AGENTS").ok().as_deref()!=Some("1"){
        return Err("External Agent Bridge requires DOMINION_EXTERNAL_AGENTS=1".into());
    }
    let runtime=job["input"]["bridgeRuntime"].as_str()
        .filter(|s|matches!(*s,"codex"|"claude"|"qwen"))
        .ok_or("Invalid external agent runtime")?;
    if job["input"]["teamMode"]!="read"{
        return Err("External write requires a separate approval bridge".into());
    }
    let prompt=job["input"]["prompt"].as_str().filter(|s|
        !s.trim().is_empty()&&s.len()<=32_000).ok_or("External prompt missing")?;
    let env_name=match runtime{
        "codex"=>"DOMINION_CODEX_BINARY",
        "claude"=>"DOMINION_CLAUDE_BINARY",
        _=>"DOMINION_QWEN_BINARY",
    };
    let binary=std::env::var(env_name).ok().filter(|s|!s.is_empty())
        .unwrap_or_else(||runtime.into());
    let workspace=crate::features::collaboration::worktree::for_job(&ctx.state,job)?;
    let mut command=Command::new(binary);
    command.current_dir(&workspace)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true);
    match runtime {
        "codex"=>{command.arg("app-server");},
        "qwen"=>{command.args(qwen_arguments(prompt,
            job["input"]["resumeId"].as_str(),
            job["input"]["model"].as_str()));},
        _=>{
            command.args(["--print","--output-format","json",
                "--permission-mode","plan","--tools","Read,Glob,Grep"]);
            if let Some(model)=job["input"]["model"].as_str(){
                command.arg("--model").arg(model);
            }
            if let Some(session)=job["input"]["resumeId"].as_str(){
                command.arg("--resume").arg(session);
            }
            command.arg("--").arg(prompt);
        }
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
    let outcome=match runtime {
        "codex"=>run_codex(ctx,&mut child,started,prompt,
            job["input"]["model"].as_str(),&workspace,
            job["input"]["resumeId"].as_str()).await,
        "qwen"=>run_qwen(ctx,&mut child,started).await,
        _=>run_claude(ctx,&mut child,started).await,
    };
    let _=child.kill().await;
    match outcome{
        Ok(value)=>{
            if runtime=="qwen" || runtime=="claude" {
                // A resumed CLI may silently create a fresh session if its
                // requested project-scoped session is missing or expired.
                // Never credit that different session to the original Task.
                if let Some(requested)=job["input"]["resumeId"].as_str(){
                    if value["sessionId"].as_str()!=Some(requested){
                        let reason=format!("{runtime} returned a different session; owner review required");
                        let _=ctx.checkpoint(json!({"bridgeState":{
                            "inFlight":false,"runtime":runtime,
                            "phase":"needs_review","error":reason}}));
                        return Err(reason);
                    }
                }
            }
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
    #[tokio::test]
    async fn bounded_external_stdout_does_not_deadlock_waiting_for_subprocess_exit(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "dominion-cli-stdout-limit-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),
            workspace:root.clone(),web_dir:root.join("web")};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"cli-over-limit","kind":"external_agent",
            "status":"running","attempts":1})).unwrap();
        let ctx=JobContext::new(state.clone(),"cli-over-limit".into());
        let mut child=Command::new("sh").arg("-c")
            .arg("yes X | head -c 2000000")
            .stdout(Stdio::piped()).stderr(Stdio::null())
            .kill_on_drop(true).spawn().unwrap();
        let failure=tokio::time::timeout(Duration::from_secs(8),
            run_qwen(&ctx,&mut child,Instant::now()))
            .await.expect("oversized output must be rejected without pipe deadlock")
            .unwrap_err();
        assert!(failure.contains("1MB hard limit"),"{failure}");
        let _=child.kill().await;
        crate::test_support::cleanup_dir(&root);
    }

    #[test]
    fn qwen_cli_requires_read_only_plan_mode_and_project_scoped_resume(){
        let args=qwen_arguments("Review project",Some("123e4567-e89b-12d3-a456-426614174000"),
            Some("qwen3-coder-plus"));
        assert!(args.windows(2).any(|pair|pair[0]=="--approval-mode"&&pair[1]=="plan"));
        assert!(args.contains(&"--safe-mode".to_string()));
        assert!(!args.contains(&"--yolo".to_string()));
        assert!(args.windows(2).any(|pair|pair[0]=="--resume"
            &&pair[1]=="123e4567-e89b-12d3-a456-426614174000"));
        assert!(args.windows(2).any(|pair|pair[0]=="--model"&&pair[1]=="qwen3-coder-plus"));
    }
    #[test]
    fn qwen_output_must_end_in_successful_result_not_plausible_chat_text(){
        let response=json!([
            {"type":"system","subtype":"session_start","session_id":"s"},
            {"type":"assistant","message":{"content":[{"type":"text","text":"partial"}]}},
            {"type":"result","subtype":"success","is_error":false,
                "result":"verified analysis","session_id":"s"}
        ]);
        let finished=qwen_result(&response).unwrap();
        assert_eq!(finished["status"],"completed");
        assert_eq!(finished["answer"],"verified analysis");
        assert_eq!(finished["sessionId"],"s");
        assert_eq!(finished["evidenceGrade"],"external_unverified");
        assert!(qwen_result(&json!([
            {"type":"assistant","message":{"content":"claimed complete"}}
        ])).is_err());
        assert!(qwen_result(&json!([
            {"type":"result","subtype":"error","is_error":true,"result":"no"}
        ])).is_err());
    }
    #[tokio::test]
    async fn qwen_headless_reads_a_real_bounded_json_output_from_a_child_process(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "dominion-qwen-jsonl-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),
            workspace:root.clone(),web_dir:root.join("web")};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"qwen-protocol-fixture",
            "kind":"external_agent","status":"running","attempts":1})).unwrap();
        let ctx=JobContext::new(state.clone(),"qwen-protocol-fixture".into());
        let mut child=Command::new("sh").arg("-c").arg(
            "printf '%s\\n' '[{\"type\":\"system\",\"session_id\":\"fixture\"},{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"session_id\":\"fixture\",\"result\":\"bounded Qwen completion\"}]'")
            .stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true)
            .spawn().unwrap();
        let result=tokio::time::timeout(Duration::from_secs(8),
            run_qwen(&ctx,&mut child,Instant::now())).await
            .expect("bounded Qwen JSON must complete").unwrap();
        assert_eq!(result["answer"],"bounded Qwen completion");
        assert_eq!(result["sessionId"],"fixture");
        assert_eq!(result["evidenceGrade"],"external_unverified");
        crate::test_support::cleanup_dir(&root);
    }
    #[test] fn codex_app_server_requests_are_explicitly_declined(){
        for method in ["item/commandExecution/requestApproval",
                       "item/fileChange/requestApproval",
                       "item/permissions/requestApproval"]{
            assert_eq!(codex_denial(method).unwrap()["decision"],"decline");
        }
        assert!(codex_denial("tool/requestUserInput").is_none());
        assert!(codex_denial("item/readOnlyMessage").is_none());
    }
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
    async fn codex_app_server_read_only_turn_declines_external_approval_and_completes(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "dominion-codex-deny-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"codex-denial",
            "kind":"external_agent","status":"running","attempts":1})).unwrap();
        let ctx=JobContext::new(state.clone(),"codex-denial".into());
        let mock=r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"id":1,"result":{}}'
      ;;
    *'"method":"thread/start"'*)
      printf '%s\n' '{"id":2,"result":{"thread":{"id":"denial-thread"}}}'
      ;;
    *'"method":"turn/start"'*)
      printf '%s\n' '{"id":77,"method":"item/commandExecution/requestApproval","params":{"command":"rm -rf ./workspace"}}'
      ;;
    *'"id":77'*'"decision":"decline"'*)
      printf '%s\n' '{"method":"item/agentMessage/delta","params":{"delta":"Permission denied. Read-only review finished."}}'
      printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
      printf '%s\n' '{"id":3,"result":{"turn":{"id":"completed"}}}'
      ;;
  esac
done
"#;
        let mut child=Command::new("sh").arg("-c").arg(mock)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .kill_on_drop(true).spawn().unwrap();
        let result=tokio::time::timeout(Duration::from_secs(8),
            run_codex(&ctx,&mut child,Instant::now(),"Review only",None,
                &ctx.state.config.workspace,None)).await.unwrap().unwrap();
        assert_eq!(result["status"],"completed");
        assert!(result["answer"].as_str().unwrap().contains("Permission denied"));
        let job=state.store.get("job","codex-denial").unwrap();
        assert_eq!(job["bridgeLastDeniedApproval"]["method"],
            "item/commandExecution/requestApproval");
        let _=child.kill().await;
        crate::test_support::cleanup_dir(&root);
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
            run_codex(&context,&mut child,Instant::now(),"Inspect fixture",None,&context.state.config.workspace,None))
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
    #[test] fn explicit_resume_preserves_readonly_codex_thread_permissions(){
        let root=std::path::Path::new("/tmp/isolated-project");
        let (method,p)=thread_request(Some("thr_2026"),root,None);
        assert_eq!(method,"thread/resume");
        assert_eq!(p["threadId"],"thr_2026");
        assert_eq!(p["sandbox"],"read-only");
        assert_eq!(p["approvalPolicy"],"never");
        assert_eq!(p["cwd"],"/tmp/isolated-project");
        let (fresh,params)=thread_request(None,root,Some("test-model"));
        assert_eq!(fresh,"thread/start");
        assert!(params["threadId"].is_null());
        assert_eq!(params["model"],"test-model");
    }
    #[test] fn unmediated_approval_request_is_denied(){
        let mut text=String::new();
        assert!(codex_event(&json!({"id":3,
            "method":"item/commandExecution/requestApproval"}),&mut text).is_err());
        assert!(text.is_empty());
    }
}
