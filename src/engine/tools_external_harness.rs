//! Optional single-turn external harness adapter.
//!
//! NO bundled Codex/Claude/Hermes runtime, SDK, dependency, scheduler, model
//! loop or extra SQLite state. Only binaries explicitly configured by the
//! operator are available. The existing native tool dispatcher owns approval,
//! execution receipts, idempotency checks, cancellations, and tool budget.
//! The external process is a delegated worker, NEVER a second DOMINION core.
use super::*;
use std::time::{Duration,Instant};

const ENV_KEY:&str="DOMINION_EXTERNAL_HARNESSES";
const MAX_ADAPTERS:usize=8;
const MAX_ARGS:usize=20;
const MAX_PROMPT:usize=24_000;

#[derive(Clone,Debug)]
struct Adapter {
    id:String,
    command:String,
    argv:Vec<String>,
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
        if valid {accepted.push(Adapter{id:id.clone(),command:command.to_string(),argv:args});}
    }
    accepted
}
fn adapters()->Vec<Adapter>{
    std::env::var(ENV_KEY).map(|raw|parse_adapters(&raw)).unwrap_or_default()
}
pub(super) fn configured_ids()->Vec<String>{
    adapters().into_iter().map(|entry|entry.id).collect()
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
)->Result<Value,String>{
    if !ctx.tools_enabled || !ctx.tools_execution_enabled {
        return Err("external harness delegation is disabled by the owner".into());
    }
    if ctx.state.is_cancelled(ctx.session_id){
        return Err("delegation cancelled before process start".into());
    }
    let started=Instant::now();
    let mut cmd=tokio::process::Command::new(&spec.command);
    cmd.args(&spec.argv).current_dir(ctx.workspace);
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
    let prompt=args["prompt"].as_str()
        .filter(|s|!s.trim().is_empty() && s.len()<=MAX_PROMPT)
        .ok_or("external harness task must be between 1 and 24000 bytes")?;
    let timeout=args["timeoutSeconds"].as_u64().unwrap_or(180).clamp(5,600);
    execute_adapter(ctx,&spec,prompt,timeout).await
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
        let spec=Adapter{id:"echo".into(),command:"/bin/cat".into(),argv:Vec::new()};
        let result=execute_adapter(&ctx,&spec,"read task",15).await.unwrap();
        assert_eq!(result["completed"],true);
        assert_eq!(result["answer"],"read task");
        assert!(state.store.all("execution_operation").is_empty(),
            "child adapter does not create a second DOMINION execution loop");
        drop(ctx);drop(state);
        std::fs::remove_dir_all(path).unwrap();
    }
}
