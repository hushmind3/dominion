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
}

fn final_answer(kind:ExternalAgent,raw:&str)->Result<String,String>{
    let result=match kind{
        ExternalAgent::Claude=>{
            let parsed:Value=serde_json::from_str(raw)
                .map_err(|_|"Claude CLI 출력 형식이 변경되었습니다.")?;
            if parsed["is_error"]==true{return Err("Claude Agent 실행이 실패했습니다.".into());}
            parsed["result"].as_str().map(str::to_string)
        }
        ExternalAgent::Qwen=>{
            let parsed:Value=serde_json::from_str(raw)
                .map_err(|_|"Qwen CLI 출력 형식이 변경되었습니다.")?;
            parsed.as_array().and_then(|rows|rows.iter().rev()
                .find(|row|row["type"]=="result"&&row["is_error"]!=true))
                .and_then(|row|row["result"].as_str().map(str::to_string))
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
    context:&JobContext,session_id:&str,workspace:&Path,prompt:&str,runtime:&str
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
    let id=format!("external:{}",context.job_id);
    let started=context.state.store.put_if_absent("external_agent_run",
        &json!({"id":id,"jobId":context.job_id,"sessionId":session_id,
            "parentSessionId":parent,"agentId":session["agentId"],
            "runtime":kind.key(),"policy":"isolated-read-only",
            "nativeToolReceipts":false,"status":"running",
            "startedAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("이 외부 Agent 실행은 이미 기록되어 있습니다. 중단 후 자동 재실행하지 않습니다.")?;
    context.state.bus.record("external_agent_run",&started);
    let (binary,argv)=kind.command(prompt);
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
            let a=(&mut stdout).take(MAX_STDOUT+1).read_to_end(&mut output);
            let b=(&mut stderr).take(MAX_STDERR+1).read_to_end(&mut errors);
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
        let hash=format!("{:x}",Sha256::digest(raw.as_bytes()));
        Ok::<(String,String),String>((answer,hash))
    }.await;
    let mut record=context.state.store.get("external_agent_run",&id)
        .ok_or("외부 Agent 실행 기록이 사라졌습니다.")?;
    match &execution{
        Ok((answer,hash))=>{
            record["status"]=json!("completed");
            record["outputSha256"]=json!(hash);
            record["answerLength"]=json!(answer.len());
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
    execution.map(|(answer,_)|(id,answer))
}

#[cfg(test)]
mod tests{
    use super::*;
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
    }
}
