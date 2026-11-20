//! Optional single-turn external harness adapter.
//!
//! NO bundled Codex/Claude/Hermes runtime, SDK, dependency, scheduler, model
//! loop or extra SQLite state. Only binaries explicitly configured by the
//! operator are available. The existing native tool dispatcher owns approval,
//! execution receipts, idempotency checks, cancellations, and tool budget.
//! The external process is a delegated worker, NEVER a second DOMINION core.
use super::*;
use std::time::{Duration,Instant};
use sha2::{Digest,Sha256};

const ENV_KEY:&str="DOMINION_EXTERNAL_HARNESSES";
const MAX_ADAPTERS:usize=8;
const MAX_ARGS:usize=20;
const MAX_PROMPT:usize=24_000;

#[derive(Clone,Debug)]
struct Adapter {
    id:String,
    command:String,
    argv:Vec<String>,
    resume_argv:Option<Vec<String>>,
}

fn safe_id(id:&str)->bool{
    !id.is_empty() && id.len()<=40 && id.bytes().all(|b|
        b.is_ascii_lowercase() || b.is_ascii_digit() || b==b'-' || b==b'_')
}
fn parse_adapters(raw:&str)->Vec<Adapter>{
    if raw.len()>16_384 {return Vec::new();}
    let Ok(parsed)=serde_json::from_str::<Value>(raw) else{return Vec::new()};
    let Some(entries)=parsed.as_object() else{return Vec::new()};
    if entries.len()>MAX_ADAPTERS{return Vec::new();}
    let mut accepted=Vec::new();
    for (id,record) in entries{
        if !safe_id(id) || record["enabled"]!=true {continue;}
        let Some(command)=record["command"].as_str() else{continue};
        let command=command.trim();
        // This is supplied only by the server operator's environment.
        // Never evaluate a shell command or an argv template from the model.
        if command.is_empty() || command.len()>320
            || command.contains('\0') || command.contains('\n')
            || command.contains('\r') {continue;}
        let Some(argv)=record["args"].as_array() else{continue};
        if argv.len()>MAX_ARGS {continue;}
        let mut args=Vec::new();
        let mut valid=true;
        for arg in argv{
            match arg.as_str(){
                Some(arg) if arg.len()<=2048 && !arg.contains('\0')=>
                    args.push(arg.to_string()),
                _=>{valid=false;break;}
            }
        }
        let resume_argv=record["resumeArgs"].as_array().and_then(|raw|{
            if raw.len()>MAX_ARGS{return None;}
            let args=raw.iter().map(|v|v.as_str().filter(|s|
                s.len()<=2048 && !s.contains('\0') && !s.contains('\n')
                && !s.contains('\r')).map(str::to_string))
                .collect::<Option<Vec<_>>>()?;
            (args.iter().filter(|a|a.contains("{session_id}")).count()==1)
                .then_some(args)
        });
        if record.get("resumeArgs").is_some() && resume_argv.is_none(){continue;}
        if valid {accepted.push(Adapter{id:id.clone(),command:command.to_string(),
            argv:args,resume_argv});}
    }
    accepted
}
fn adapters()->Vec<Adapter>{
    std::env::var(ENV_KEY).map(|raw|parse_adapters(&raw)).unwrap_or_default()
}
pub(super) fn configured_ids()->Vec<String>{
    adapters().into_iter().map(|entry|entry.id).collect()
}

fn safe_remote_id(id:&str)->bool{
    !id.is_empty() && id.len()<=160 && id.bytes().all(|b|
        b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b':'|b'.'))
}
fn remote_session_id(stdout:&str)->Option<String>{
    let candidate=|value:Value|->Option<String>{
        let id=value["thread_id"].as_str()
            .or_else(||value["session_id"].as_str())
            .or_else(||value["sessionId"].as_str())?;
        safe_remote_id(id).then(||id.to_string())
    };
    serde_json::from_str::<Value>(stdout).ok().and_then(&candidate)
        .or_else(||stdout.lines().find_map(|line|
            serde_json::from_str::<Value>(line).ok().and_then(&candidate)))
}
fn adapter_version(spec:&Adapter)->String{
    format!("{:x}",Sha256::digest(
        json!([spec.id,spec.command,spec.argv,spec.resume_argv])
            .to_string().as_bytes()))
}
fn same_owner(row:&Value,ctx:&ToolContext<'_>,spec:&Adapter)->bool{
    row["harnessId"]==spec.id
        && row["adapterVersion"]==adapter_version(spec)
        && row["sessionId"]==ctx.session_id
        && row["agentId"].as_str()==ctx.agent_id
        && row["workspace"]==ctx.workspace.to_string_lossy().as_ref()
}
fn has_failed_turn(stdout:&str)->bool{
    stdout.lines().any(|line|serde_json::from_str::<Value>(line)
        .ok().is_some_and(|value|value["type"]=="turn.failed"))
}
fn agent_answer(stdout:&str)->Option<String>{
    if let Ok(value)=serde_json::from_str::<Value>(stdout){
        if let Some(result)=value["result"].as_str(){return Some(result.to_string());}
        if let Some(answer)=value["answer"].as_str(){return Some(answer.to_string());}
    }
    // Codex exec --json emits JSONL. Do not mistake a command_execution
    // event for the final answer, or a turn.failed for a success message.
    let mut last=None;
    for line in stdout.lines(){
        if let Ok(value)=serde_json::from_str::<Value>(line){
            if value["type"]=="item.completed"
                && value["item"]["type"]=="agent_message" {
                if let Some(text)=value["item"]["text"].as_str(){
                    last=Some(text.to_string());
                }
            }
            if value["type"]=="turn.failed" {return None;}
        }
    }
    last
}
async fn execute_adapter(
    ctx:&ToolContext<'_>,
    spec:&Adapter,
    prompt:&str,
    timeout_seconds:u64,
    resume_remote:Option<&str>,
)->Result<Value,String>{
    if !ctx.tools_enabled || !ctx.tools_execution_enabled {
        return Err("external harness delegation is disabled by the owner".into());
    }
    if ctx.state.is_cancelled(ctx.session_id){
        return Err("delegation cancelled before process start".into());
    }
    let started=Instant::now();
    let mut cmd=tokio::process::Command::new(&spec.command);
    if let Some(remote)=resume_remote{
        if !safe_remote_id(remote){return Err("unsafe remote session ID".into());}
        let args=spec.resume_argv.as_ref()
            .ok_or("owner has not configured external session resume")?;
        cmd.args(args.iter().map(|item|item.replace("{session_id}",remote)));
    }else{cmd.args(&spec.argv);}
    cmd.current_dir(ctx.workspace);
    // One native process supervisor serves terminal_exec, shell_exec and
    // this optional adapter. No child lifecycle, pipe reader or timeout
    // implementation is duplicated in the harness integration.
    let pending=super::run_command_with_input(
        &mut cmd,Duration::from_secs(timeout_seconds.clamp(5,600)),
        prompt.as_bytes(),
    );
    tokio::pin!(pending);
    let outcome=loop{
        if ctx.state.is_cancelled(ctx.session_id){
            return Ok(json!({"error":"external harness was cancelled",
                "outcomeUnknown":true,"harnessId":spec.id,
                "requiresInspection":true}));
        }
        tokio::select!{
            finished=&mut pending =>break finished,
            _=ctx.state.cancel_notify.notified()=>{},
            // Covers a cancellation signal racing with Notify registration
            // and a cancellation initiated from another engine process.
            _=tokio::time::sleep(Duration::from_millis(500))=>{},
        }
    };
    let (output,output_truncated,errors_truncated)=match outcome{
        Ok(value)=>value,
        Err(error)=>return Ok(json!({
            "harnessId":spec.id,"error":error,"outcomeUnknown":true,
            "requiresInspection":true,"elapsedMs":started.elapsed().as_millis() as u64,
        })),
    };
    let stdout=String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr=String::from_utf8_lossy(&output.stderr).into_owned();
    let answer=agent_answer(&stdout).unwrap_or_else(||
        stdout.chars().rev().take(12000).collect::<String>()
            .chars().rev().collect::<String>());
    let verified_exit=output.status.success()
        && !output_truncated && !has_failed_turn(&stdout)
        && !answer.trim().is_empty();
    Ok(json!({
        "harnessId":spec.id,
        "runtime":"external_process",
        "remoteSessionId":if verified_exit{remote_session_id(&stdout)}
            else{None},
        "exitCode":output.status.code(),
        "completed":verified_exit,
        "answer":answer,
        "stdoutTruncated":output_truncated,
        "stderrTruncated":errors_truncated,
        "stderr":stderr.chars().take(2500).collect::<String>(),
        "elapsedMs":started.elapsed().as_millis() as u64,
        "error":if verified_exit{Value::Null}else{
            json!("external harness failed or output was truncated; inspect external state before retry")
        },
        "outcomeUnknown":!verified_exit,
        "requiresInspection":!verified_exit,
        "externalPermissions":"governed by installed harness; DOMINION approval is an outer gate",
    }))
}
pub(super) async fn run(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let id=args["harnessId"].as_str()
        .filter(|id|safe_id(id)).ok_or("harnessId is required")?;
    let spec=adapters().into_iter().find(|entry|entry.id==id)
        .ok_or_else(||format!("external harness '{id}' is not enabled by the server owner"))?;
    run_with_spec(ctx,&spec,args).await
}
/// Testable host gateway without mutating the process-wide adapter registry.
async fn run_with_spec(
    ctx:&ToolContext<'_>,spec:&Adapter,args:&Value,
)->Result<Value,String>{
    let prompt=args["prompt"].as_str()
        .filter(|s|!s.trim().is_empty() && s.len()<=MAX_PROMPT)
        .ok_or("external harness task must be between 1 and 24000 bytes")?;
    let timeout=args["timeoutSeconds"].as_u64().unwrap_or(180).clamp(5,600);
    let requested=args["bridgeSessionId"].as_str();
    let resume=requested.filter(|id|uuid::Uuid::parse_str(id).is_ok());
    if requested.is_some() && resume.is_none(){
        return Err("bridgeSessionId must be a DOMINION-issued UUID".into());
    }
    if resume.is_some() && spec.resume_argv.is_none(){
        return Err("this external harness has no configured resume adapter".into());
    }
    // Reserve a session BEFORE launching any external process. A crash
    // leaves status=running, which cannot be replayed automatically.
    let bridge=if let Some(id)=resume{
        let old=ctx.store.get("external_agent_session",id)
            .ok_or("external session not found")?;
        if !same_owner(&old,ctx,&spec){
            return Err("external session has a different owner or adapter revision".into());
        }
        if old["status"]!="ready"{
            return Err("external session is unresolved or already executing".into());
        }
        ctx.store.patch_if("external_agent_session",id,"status",&json!("ready"),
            &json!({"status":"running"}))
            .ok_or("external session concurrently claimed")?
    }else if spec.resume_argv.is_some(){
        ctx.store.put_if_absent("external_agent_session",&json!({
            "id":uuid::Uuid::new_v4().to_string(),"harnessId":spec.id,
            "adapterVersion":adapter_version(&spec),
            "sessionId":ctx.session_id,"agentId":ctx.agent_id,
            "workspace":ctx.workspace.to_string_lossy(),
            "status":"running","remoteSessionId":null,
        })).ok_or("failed to record external session reservation")?
    }else{Value::Null};
    if !bridge.is_null(){ctx.state.bus.record("external_agent_session",&bridge);}
    let remote=bridge["remoteSessionId"].as_str().map(str::to_string);
    let outcome=execute_adapter(ctx,&spec,prompt,timeout,remote.as_deref()).await;
    let mut result=match outcome{
        Ok(value)=>value,
        Err(error)=>{
            if let Some(id)=bridge["id"].as_str(){
                if let Some(row)=ctx.store.patch_if("external_agent_session",
                    id,"status",&json!("running"),&json!({
                        "status":"needs_review","lastError":error,
                    })){
                    ctx.state.bus.record("external_agent_session",&row);
                }
            }
            return Err(error);
        }
    };
    if let Some(id)=bridge["id"].as_str(){
        let remote_id=result["remoteSessionId"].as_str()
            .filter(|id|safe_remote_id(id))
            .or_else(||remote.as_deref());
        let ready=result["completed"]==true && result["outcomeUnknown"]!=true
            && remote_id.is_some();
        let updated=ctx.store.patch_if("external_agent_session",
            id,"status",&json!("running"),&json!({
                "status":if ready{"ready"}else{"needs_review"},
                "remoteSessionId":remote_id,
                "lastCompleted":result["completed"],
                "lastOutcomeUnknown":result["outcomeUnknown"],
                "lastUsedAt":chrono::Utc::now().to_rfc3339(),
            })).ok_or("external session changed while executing; inspect before retry")?;
        ctx.state.bus.record("external_agent_session",&updated);
        result["bridgeSessionId"]=json!(id);
        result["sessionResumable"]=json!(ready);
        if !ready{result["requiresInspection"]=json!(true);}
    }
    // Never expose the raw remote thread id as a resume credential.
    if let Some(map)=result.as_object_mut(){map.remove("remoteSessionId");}
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_operator_provided_adapters_can_be_selected(){
        let list=parse_adapters(r#"{
            "codex":{"enabled":true,"command":"codex","args":["exec","--json","-"]},
            "claude":{"enabled":false,"command":"claude","args":["-p"]},
            "bad/name":{"enabled":true,"command":"sh","args":["-c","oops"]}
        }"#);
        assert_eq!(list.len(),1);
        assert_eq!(list[0].id,"codex");
        assert_eq!(list[0].argv,vec!["exec","--json","-"]);
        assert!(parse_adapters("not-json").is_empty());
        assert!(parse_adapters(r#"{"x":{"enabled":true,"command":"sh",
            "args":[null]}}"#).is_empty());
        assert!(parse_adapters(r#"{"x":{"enabled":true,"command":"bad\u0000",
            "args":[]}}"#).is_empty());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn real_bridge_session_resumes_once_and_rejects_other_owner_or_busy_session(){
        let dir=std::env::temp_dir().join(format!("dominion-agent-bridge-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"owner-session",agent_id:Some("a"),
            tools_enabled:true,tools_execution_enabled:true};
        let spec=Adapter{id:"echo".into(),command:"/bin/echo".into(),
            argv:vec![r#"{"result":"first","session_id":"remote-123"}"#.into()],
            resume_argv:Some(vec![
                r#"{"result":"second"}"#.into(),"{session_id}".into()]),
        };
        let first=run_with_spec(&ctx,&spec,&json!({"prompt":"initial"}))
            .await.unwrap();
        assert_eq!(first["answer"],"first");
        assert_eq!(first["sessionResumable"],true);
        assert!(first["remoteSessionId"].is_null());
        let uuid=first["bridgeSessionId"].as_str().unwrap();
        let stored=ctx.store.get("external_agent_session",uuid).unwrap();
        assert_eq!(stored["remoteSessionId"],"remote-123");
        assert_eq!(stored["status"],"ready");
        let second=run_with_spec(&ctx,&spec,&json!({
            "prompt":"follow-up","bridgeSessionId":uuid
        })).await.unwrap();
        assert_eq!(second["bridgeSessionId"],uuid);
        assert_eq!(second["sessionResumable"],true);
        assert_eq!(ctx.store.get("external_agent_session",uuid).unwrap()
            ["status"],"ready");
        let foreign=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"other-owner",agent_id:Some("a"),
            tools_enabled:true,tools_execution_enabled:true};
        assert!(run_with_spec(&foreign,&spec,&json!({
            "prompt":"take over","bridgeSessionId":uuid
        })).await.is_err());
        ctx.store.patch_if("external_agent_session",uuid,"status",
            &json!("ready"),&json!({"status":"running"})).unwrap();
        assert!(run_with_spec(&ctx,&spec,&json!({
            "prompt":"replay after crash","bridgeSessionId":uuid
        })).await.is_err());
        drop(ctx);drop(foreign);drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_owner_configured_resume_templates_can_issue_bridge_session_tokens(){
        let list=parse_adapters(r#"{
            "codex":{"enabled":true,"command":"codex","args":["exec","-"],
                "resumeArgs":["exec","resume","{session_id}","-"]},
            "bad":{"enabled":true,"command":"tool","args":[],
                "resumeArgs":["resume","missing"]}}"#);
        assert_eq!(list.len(),1);
        assert_eq!(list[0].resume_argv.as_ref().unwrap()[2],"{session_id}");
        assert_eq!(remote_session_id(
            "{\"type\":\"thread.started\",\"thread_id\":\"thread-123\"}\n"
        ).as_deref(),Some("thread-123"));
        assert_eq!(remote_session_id(
            r#"{"session_id":"claude-456","result":"ok"}"#
        ).as_deref(),Some("claude-456"));
        assert!(remote_session_id(r#"{"session_id":"bad;rm -rf /"}"#).is_none());
    }

    #[test]
    fn codex_jsonl_and_claude_json_are_reduced_to_answer_without_new_state(){
        let jsonl=concat!(
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"first\"}}\n",
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"final\"}}\n",
            "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":10}}\n");
        assert_eq!(agent_answer(jsonl).as_deref(),Some("final"));
        assert_eq!(agent_answer(r#"{"result":"ok"}"#).as_deref(),Some("ok"));
        assert!(agent_answer("{\"type\":\"turn.failed\"}\n").is_none());
        assert!(has_failed_turn("{\"type\":\"turn.failed\"}\n"));
        assert!(!has_failed_turn(jsonl));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn installed_binary_runs_lazily_and_returns_native_result(){
        let path=std::env::temp_dir().join(format!("harness-unit-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:path.join("db"),web_dir:path.join("web"),workspace:path.clone()};
        std::fs::create_dir_all(&path).unwrap();
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let ctx=ToolContext{workspace:&path,store:&state.store,state:&state,
            session_id:"adapter-test",agent_id:None,
            tools_enabled:true,tools_execution_enabled:true};
        let spec=Adapter{id:"echo".into(),command:"/bin/cat".into(),
            argv:Vec::new(),resume_argv:None};
        let result=execute_adapter(&ctx,&spec,"read task",15,None).await.unwrap();
        assert_eq!(result["completed"],true);
        assert_eq!(result["answer"],"read task");
        assert!(state.store.all("execution_operation").is_empty(),
            "child adapter does not create a second DOMINION execution loop");
        drop(ctx);drop(state);
        std::fs::remove_dir_all(path).unwrap();
    }
}
