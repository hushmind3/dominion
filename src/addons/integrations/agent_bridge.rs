//! External *agent* worker bridge, not a ModelProvider.
//!
//! External executors run their OWN loops, approvals and tools, therefore this
//! route deliberately does not assert that DOMINION native tool receipts cover
//! their internal actions. Only an opt-in isolated inspection job is admitted.
//! No shell, arbitrary executable, inherited extra arguments or YOLO switches.
use crate::{addons::jobs::JobContext,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,time::Duration};
use tokio::{io::AsyncReadExt,process::Command};

const MAX_PROMPT:usize=16_000;
const MAX_STDOUT:u64=256*1024;
const MAX_STDERR:u64=12*1024;
const MAX_ANSWER:usize=12_000;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum ExternalAgent { Codex,Claude,Qwen }
impl ExternalAgent {
    fn parse(raw:&str)->Result<Self,String>{
        match raw{
            "codex"=>Ok(Self::Codex),"claude"=>Ok(Self::Claude),
            "qwen"=>Ok(Self::Qwen),
            _=>Err("지원하지 않는 외부 Agent Bridge 실행기입니다.".into())
        }
    }
    fn key(self)->&'static str{
        match self{Self::Codex=>"codex",Self::Claude=>"claude",Self::Qwen=>"qwen"}
    }
    fn command(self,prompt:&str)->(&'static str,Vec<String>){
        let argv=match self{
            Self::Codex=>vec!["exec","--sandbox","read-only","--json",
                "--skip-git-repo-check",prompt],
            Self::Claude=>vec!["-p",prompt,"--permission-mode","plan",
                "--max-turns","12","--output-format","json"],
            Self::Qwen=>vec!["-p",prompt,"--safe-mode","--approval-mode","plan",
                "--exclude-tools","shell,write,edit","--max-session-turns","12",
                "--max-wall-time","180","--output-format","json"]
        };
        (self.key(),argv.into_iter().map(str::to_string).collect())
    }
    /// Opt-in continuation of a completed inspector. Never use --last:
    /// that could attach an unrelated workspace's conversation.
    fn command_with_resume(self,prompt:&str,resume:Option<&str>)
        ->Result<(&'static str,Vec<String>),String>{
        let (binary,mut args)=self.command(prompt);
        if let Some(session)=resume {
            if uuid::Uuid::parse_str(session).is_err(){
                return Err("외부 Agent 재개 세션 ID가 유효하지 않습니다.".into());
            }
            match self{
                Self::Codex=>{
                    // The sandbox flag belongs BEFORE the resume subcommand.
                    // Passing it after 'resume' can be rejected by some CLIs.
                    args.pop();
                    args.extend(["resume".into(),session.into(),prompt.into()]);
                }
                Self::Claude|Self::Qwen=>{
                    args.extend(["--resume".into(),session.into()]);
                }
            }
        }
        Ok((binary,args))
    }
}

/// Only a successful terminal envelope may supply a resumable CLI session.
/// These IDs are external CLI state, never proof of native tool execution.
fn terminal_session_id(kind:ExternalAgent,raw:&str)->Option<String>{
    let session=match kind{
        ExternalAgent::Codex=>raw.lines().filter_map(|line|
            serde_json::from_str::<Value>(line).ok())
            .find(|event|event["type"]=="thread.started")
            .and_then(|event|event["thread_id"].as_str().map(str::to_string)),
        ExternalAgent::Claude=>serde_json::from_str::<Value>(raw).ok()
            .filter(|result|result["type"]=="result"
                &&result["is_error"]!=true
                &&result["subtype"].as_str().is_none_or(|v|v=="success"))
            .and_then(|result|result["session_id"].as_str().map(str::to_string)),
        ExternalAgent::Qwen=>serde_json::from_str::<Value>(raw).ok()
            .and_then(|value|value.as_array().cloned())
            .and_then(|rows|rows.into_iter().rev()
                .find(|row|row["type"]=="result"))
            .filter(|row|row["is_error"]!=true
                &&row["subtype"].as_str().is_none_or(|v|v=="success"))
            .and_then(|row|row["session_id"].as_str().map(str::to_string)),
    }?;
    uuid::Uuid::parse_str(&session).ok().map(|id|id.to_string())
}

/// Scope a resume token to a completed child of the SAME owner, agent,
/// external runtime and isolated Git checkout. Claim once in durable SQLite.
fn claim_completed_session(context:&JobContext,session:&Value,
    kind:ExternalAgent,previous_job_id:&str)->Result<String,String>{
    let store=&context.state.store;
    let old=store.get("job",previous_job_id)
        .ok_or("이전 외부 Agent 작업을 찾을 수 없습니다.")?;
    if old["status"]!="completed"
        ||old["input"]["delegationProfile"]!="inspection"
        ||old["input"]["externalRuntime"]!=kind.key()
        ||old["input"]["parentSessionId"]!=session["parentSessionId"]
        ||old["input"]["agentId"]!=session["agentId"]
        ||old["id"]==context.job_id {
        return Err("완료된 같은 소유자·에이전트·실행기의 조사 작업만 재개할 수 있습니다.".into());
    }
    let old_session=old["input"]["sessionId"].as_str()
        .and_then(|id|store.get("session",id))
        .ok_or("이전 외부 Agent 격리 세션이 없습니다.")?;
    if old_session["delegationWorkspace"]!=session["delegationWorkspace"]
        ||old_session["readOnly"]!=true ||old_session["delegationProfile"]!="inspection" {
        return Err("다른 격리 작업공간에서 외부 Agent 세션을 재사용할 수 없습니다.".into());
    }
    let run_id=old["result"]["externalRunId"].as_str()
        .ok_or("이전 외부 Agent 실행 증거가 없습니다.")?;
    let receipt=store.get("external_agent_run",run_id)
        .ok_or("이전 CLI 실행 기록이 없습니다.")?;
    if receipt["status"]!="completed"
        ||receipt["jobId"]!=old["id"]
        ||receipt["sessionId"]!=old["input"]["sessionId"]
        ||receipt["runtime"]!=kind.key()
        ||receipt["agentId"]!=session["agentId"]{
        return Err("이전 CLI 실행의 소유권 또는 완료 증거가 불일치합니다.".into());
    }
    let token=receipt["externalSessionId"].as_str()
        .filter(|id|uuid::Uuid::parse_str(id).is_ok())
        .ok_or("이전 CLI가 재개 가능한 세션 ID를 저장하지 않았습니다.")?.to_string();
    let claimed=store.mutate_record("external_agent_run",run_id,
        json!({"id":run_id}),|row|{
            if row["status"]!="completed"||row["resumedByJobId"].is_string(){
                return Err("이 CLI 세션은 이미 이어졌거나 완료되지 않았습니다.".into());
            }
            row["resumedByJobId"]=json!(context.job_id);
            row["resumeClaimedAt"]=json!(chrono::Utc::now().to_rfc3339());
            Ok(vec![])
        })?;
    context.state.bus.record("external_agent_run",&claimed);
    Ok(token)
}

fn final_answer(kind:ExternalAgent,raw:&str)->Result<String,String>{
    let result=match kind{
        ExternalAgent::Claude=>{
            let parsed:Value=serde_json::from_str(raw)
                .map_err(|_|"Claude CLI 출력 형식이 변경되었습니다.")?;
            // Headless JSON includes a terminal result envelope. Do not
            // accept intermediate or failed outputs as completed evidence.
            if parsed["type"]!="result"||parsed["is_error"]==true
                ||parsed["subtype"].as_str().is_some_and(|v|v!="success"){
                return Err("Claude Agent 실행이 정상 종료되지 않았습니다.".into());
            }
            parsed["result"].as_str().map(str::to_string)
        }
        ExternalAgent::Qwen=>{
            let parsed:Value=serde_json::from_str(raw)
                .map_err(|_|"Qwen CLI 출력 형식이 변경되었습니다.")?;
            // --output-format json returns a full event array. The LAST
            // terminal result is authoritative: searching backward for an
            // earlier success can misreport a later failed turn as complete.
            let terminal=parsed.as_array().and_then(|rows|rows.iter().rev()
                .find(|row|row["type"]=="result"));
            if terminal.is_some_and(|row|row["is_error"]==true){
                return Err("Qwen Agent 최종 실행 결과가 실패했습니다.".into());
            }
            terminal.and_then(|row|row["result"].as_str().map(str::to_string))
        }
        ExternalAgent::Codex=>{
            let mut output=None;
            let mut completed=false;
            for line in raw.lines(){
                let Ok(event)=serde_json::from_str::<Value>(line) else{continue;};
                if event["type"]=="turn.failed" {
                    return Err("Codex Agent 실행이 실패했습니다.".into());
                }
                if event["type"]=="turn.completed"{completed=true;}
                if event["type"]=="item.completed"
                    &&event["item"]["type"]=="agent_message" {
                    if let Some(text)=event["item"]["text"].as_str(){
                        output=Some(text.to_string());
                    }
                }
            }
            if completed{output}else{None}
        }
    }.ok_or("외부 Agent 실행 결과에 확정된 최종 응답이 없습니다.")?;
    let trimmed=result.trim();
    if trimmed.is_empty()||trimmed.chars().count()>MAX_ANSWER{
        return Err("외부 Agent 최종 응답이 비어 있거나 허용 길이를 초과했습니다.".into());
    }
    Ok(trimmed.to_string())
}

async fn cancelled(state:&AppState,job_id:&str,session:&str,parent:&str,epoch:u64){
    loop{
        if state.is_cancelled(session)||state.is_cancelled(parent)
            ||state.cancellation_generation(parent)!=epoch
            ||state.store.get("job",job_id)
                .is_none_or(|job|job["cancelRequested"]==true||job["status"]=="cancelled"){
            return;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Execute an explicitly chosen external inspector in an isolated worktree.
/// A crash with the in-flight marker is reviewed rather than replayed.
pub async fn inspect(
    context:&JobContext,session_id:&str,workspace:&Path,prompt:&str,
    runtime:&str,resume_from_job:Option<&str>,
)->Result<(String,String),String>{
    let kind=ExternalAgent::parse(runtime)?;
    if prompt.trim().is_empty()||prompt.len()>MAX_PROMPT{
        return Err("외부 Agent 프롬프트 길이가 허용 범위를 벗어났습니다.".into());
    }
    let session=context.state.store.get("session",session_id)
        .ok_or("외부 Agent 세션이 없습니다.")?;
    if session["delegationProfile"]!="inspection"||session["readOnly"]!=true
        ||!session["delegationWorkspace"].is_object() {
        return Err("외부 실행기는 격리된 읽기 전용 inspection 작업에서만 실행할 수 있습니다.".into());
    }
    let parent=session["parentSessionId"].as_str()
        .ok_or("외부 Agent 상위 세션이 없습니다.")?;
    let workspace=workspace.canonicalize().map_err(|e|e.to_string())?;
    let expected=crate::engine::delegation_worktree::scoped_workspace(
        &context.state,&session)?.ok_or("외부 Agent 격리 워크트리가 없습니다.")?;
    if workspace!=expected.canonicalize().map_err(|e|e.to_string())?{
        return Err("외부 실행기 작업 폴더가 격리 워크트리와 다릅니다.".into());
    }
    if context.state.is_cancelled(parent) {
        return Err("상위 작업이 취소되어 외부 실행기를 시작하지 않습니다.".into());
    }
    let generation=context.state.cancellation_generation(parent);
    // Explicit continuation is claimed once; a timeout, crash or cancellation
    // after this point is review-only and never automatically replayed.
    let resume_session=resume_from_job
        .map(|previous|claim_completed_session(context,&session,kind,previous))
        .transpose()?;
    let id=format!("external:{}",context.job_id);
    let started=context.state.store.put_if_absent("external_agent_run",
        &json!({"id":id,"jobId":context.job_id,"sessionId":session_id,
            "parentSessionId":parent,"agentId":session["agentId"],
            "runtime":kind.key(),"policy":"isolated-read-only",
            "resumeFromJobId":resume_from_job,
            "nativeToolReceipts":false,"status":"running",
            "startedAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("이 외부 Agent 실행은 이미 기록되어 있습니다. 중단 후 자동 재실행하지 않습니다.")?;
    context.state.bus.record("external_agent_run",&started);
    let (binary,argv)=kind.command_with_resume(prompt,resume_session.as_deref())?;
    let mut command=Command::new(binary);
    command.args(argv).current_dir(&workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let execution=async{
        let mut child=command.spawn()
            .map_err(|error|format!("외부 실행기 {binary} 시작 실패: {error}"))?;
        let mut stdout=child.stdout.take().ok_or("표준 출력 파이프를 열지 못했습니다.")?;
        let mut stderr=child.stderr.take().ok_or("오류 출력 파이프를 열지 못했습니다.")?;
        let mut output=Vec::new();
        let mut errors=Vec::new();
        let work=async{
            let mut bounded_stdout=(&mut stdout).take(MAX_STDOUT+1);
            let mut bounded_stderr=(&mut stderr).take(MAX_STDERR+1);
            let a=bounded_stdout.read_to_end(&mut output);
            let b=bounded_stderr.read_to_end(&mut errors);
            let (_a,_b,status)=tokio::try_join!(a,b,child.wait())
                .map_err(|e|e.to_string())?;
            Ok::<_,String>(status)
        };
        let done=tokio::select!{
            result=tokio::time::timeout(Duration::from_secs(180),work)=>{
                result.map_err(|_|"외부 Agent 180초 실행 제한을 초과했습니다.".to_string())
                    .and_then(|result|result)
            }
            _=cancelled(&context.state,&context.job_id,session_id,parent,generation)=>{
                Err("상위 작업 또는 외부 Agent 실행이 취소되었습니다.".into())
            }
        };
        if done.is_err(){let _=child.start_kill();let _=tokio::time::timeout(
            Duration::from_secs(2),child.wait()).await;}
        let status=done?;
        if output.len() as u64>MAX_STDOUT||errors.len() as u64>MAX_STDERR {
            return Err("외부 Agent 출력 제한을 초과했습니다.".into());
        }
        if !status.success(){
            return Err(format!("외부 Agent {binary} 실행이 실패했습니다(종료 코드 {}).",
                status.code().unwrap_or(-1)));
        }
        let raw=String::from_utf8(output).map_err(|_|"외부 실행기 출력이 UTF-8이 아닙니다.")?;
        let answer=final_answer(kind,&raw)?;
        let session=terminal_session_id(kind,&raw);
        // For a resumed process, do not accept a different conversation ID.
        if resume_session.as_ref().is_some_and(|id|
            session.as_ref().is_some_and(|observed|observed!=id)){
            return Err("외부 Agent가 다른 대화 세션으로 재연결되었습니다.".into());
        }
        let hash=format!("{:x}",Sha256::digest(raw.as_bytes()));
        Ok::<(String,String,Option<String>),String>((answer,hash,session))
    }.await;
    let mut record=context.state.store.get("external_agent_run",&id)
        .ok_or("외부 Agent 실행 기록이 사라졌습니다.")?;
    match &execution{
        Ok((answer,hash,session))=>{
            record["status"]=json!("completed");
            record["outputSha256"]=json!(hash);
            record["answerLength"]=json!(answer.len());
            if let Some(session)=session{
                record["externalSessionId"]=json!(session);
            }
        }
        Err(error)=>{
            record["status"]=json!("needs_review");
            record["errorClass"]=json!(if error.contains("취소"){"cancelled"}
                else if error.contains("제한"){"limit"}else{"execution_error"});
        }
    }
    record["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
    let saved=context.state.store.put("external_agent_run",&record)
        .ok_or("외부 Agent 최종 증거를 기록하지 못했습니다.")?;
    context.state.bus.record("external_agent_run",&saved);
    execution.map(|(answer,_,_)|(id,answer))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn headless_resume_tokens_are_terminal_verified_and_scoped(){
        let token="123e4567-e89b-12d3-a456-426614174000";
        let codex=format!(
            "{{\"type\":\"thread.started\",\"thread_id\":\"{token}\"}}\n\
             {{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"ok\"}}}}\n\
             {{\"type\":\"turn.completed\"}}\n");
        assert_eq!(terminal_session_id(ExternalAgent::Codex,&codex).as_deref(),Some(token));
        assert_eq!(terminal_session_id(ExternalAgent::Claude,
            &json!({"type":"result","subtype":"success","result":"ok",
                "session_id":token}).to_string()).as_deref(),Some(token));
        assert_eq!(terminal_session_id(ExternalAgent::Qwen,
            &json!([{"type":"system","session_id":token},
                {"type":"result","subtype":"success","result":"ok",
                 "session_id":token}]).to_string()).as_deref(),Some(token));
        assert!(terminal_session_id(ExternalAgent::Claude,
            &json!({"type":"result","subtype":"error_max_turns","result":"partial",
                "session_id":token}).to_string()).is_none());
        assert!(terminal_session_id(ExternalAgent::Codex,
            "{\"type\":\"turn.completed\"}\n").is_none());
        assert!(ExternalAgent::Qwen.command_with_resume("hello",Some("--last")).is_err());
        for kind in [ExternalAgent::Codex,ExternalAgent::Claude,ExternalAgent::Qwen]{
            let (_binary,args)=kind.command_with_resume("new question",Some(token)).unwrap();
            assert!(args.iter().any(|v|v==token));
            assert!(args.iter().any(|v|v=="new question"));
            assert!(!args.iter().any(|v|v=="--last"||v=="--yolo"
                ||v=="danger-full-access"));
            if kind==ExternalAgent::Codex{
                assert!(args.windows(2).any(|w|w==["--sandbox","read-only"]));
                assert!(args.windows(2).any(|w|w==["resume",token]));
            }else{
                assert!(args.windows(2).any(|w|w==["--resume",token]));
            }
        }
    }
    #[test]
    fn agent_executors_are_explicitly_read_only_and_never_use_shell(){
        for name in ["codex","claude","qwen"]{
            let (binary,args)=ExternalAgent::parse(name).unwrap().command("test; echo danger");
            assert_eq!(binary,name);
            assert!(args.iter().any(|arg|arg=="test; echo danger"));
            assert!(!args.iter().any(|arg|arg=="--yolo"||arg=="danger-full-access"));
            assert!(args.iter().any(|arg|arg=="read-only"||arg=="plan"));
        }
        assert!(ExternalAgent::parse("bash").is_err());
    }
    #[test]
    fn terminal_evidence_formats_are_not_confused_with_model_provider_text(){
        assert_eq!(final_answer(ExternalAgent::Claude,
            r#"{"type":"result","result":"Claude result","is_error":false}"#).unwrap(),
            "Claude result");
        assert_eq!(final_answer(ExternalAgent::Qwen,
            r#"[{"type":"system"},{"type":"result","result":"Qwen result","is_error":false}]"#).unwrap(),
            "Qwen result");
        assert_eq!(final_answer(ExternalAgent::Codex,
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Codex result\"}}\n{\"type\":\"turn.completed\"}\n").unwrap(),
            "Codex result");
        assert!(final_answer(ExternalAgent::Codex,
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"not final\"}}\n").is_err());
        assert!(final_answer(ExternalAgent::Claude,
            r#"{"type":"result","result":"fake","is_error":true}"#).is_err());
        assert!(final_answer(ExternalAgent::Claude,
            r#"{"type":"assistant","result":"unconfirmed"}"#).is_err());
        assert!(final_answer(ExternalAgent::Claude,
            r#"{"type":"result","subtype":"error_max_turns","result":"partial","is_error":false}"#).is_err());
        assert_eq!(final_answer(ExternalAgent::Claude,
            r#"{"type":"result","subtype":"success","result":"verified","is_error":false}"#).unwrap(),
            "verified");
        assert!(final_answer(ExternalAgent::Qwen,
            r#"[{"type":"result","result":"old success","is_error":false},
                {"type":"result","result":"new failure","is_error":true}]"#).is_err());
        assert!(final_answer(ExternalAgent::Qwen,
            r#"[{"type":"assistant","result":"not completed"}]"#).is_err());
    }
}
