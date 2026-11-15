//! Product adapter for the independent DOMINION Rust agent SDK.
//!
//! The Rust job worker, GUI and external TypeScript client all call this exact
//! execution path. Existing provider, vault, SQLite, MCP, tool and approval
//! implementations are adapters, not owners of the new agent loop.
use crate::{engine::{chat, gateway, jobs::JobContext, llm, mcp, tools, verification}, state::AppState};
use dominion_sdk::harness::{
    Action, BoxFuture, Criterion, CriterionKind, Evidence, Journal, Kernel,
    Limits, Model, ModelReply, RunRequest, Snapshot, Strategy, ToolSpec, Tools,
};
use serde_json::{json, Value};
use std::{path::{Component, Path, PathBuf}, collections::HashSet};

struct ConfiguredModel {
    state: AppState,
    provider: String,
    model: String,
    session: String,
    allow_fallback: bool,
}
impl Model for ConfiguredModel {
    fn complete<'a>(&'a self, system: &'a str, prompt: &'a str)
        -> BoxFuture<'a, Result<ModelReply, String>> {
        Box::pin(async move {
            let content = if self.allow_fallback {
                llm::complete_with_fallback_for_session(
                    &self.state, &self.provider, &self.model, system, prompt, &self.session
                ).await?.0
            } else {
                llm::complete_for_session(
                    &self.state, &self.provider, &self.model, system, prompt, &self.session
                ).await?
            };
            // This adapter's helper API already records provider-reported
            // usage centrally. None explicitly means the kernel must estimate.
            Ok(ModelReply {content, input_tokens: None, output_tokens: None})
        })
    }
}
struct NativeTools {
    state: AppState,
    workspace: PathBuf,
    session: String,
    agent: Option<String>,
    run_id: String,
    tool_specs: Vec<ToolSpec>,
    tools_enabled: bool,
}
// Keep the tail of compiler/test diagnostics rather than the beginning of
// potentially enormous build logs. This is the portion that usually contains
// the failing test, exception, or final actionable compiler error.
fn diagnostic_tail(source:&str, max_chars:usize)->String {
    let mut tail=source.chars().rev().take(max_chars).collect::<Vec<_>>();
    tail.reverse();
    let data=tail.into_iter().collect::<String>();
    if source.chars().count()>max_chars {
        format!("[earlier output elided] {data}")
    } else {data}
}
/// Tool transport success is not evidence that the action succeeded.
/// The SDK must not accept a failing command, HTTP response or MCP result.
fn verified_tool_result(name: &str, value: Value) -> Result<Value,String> {
    if value["isError"] == true || value["success"] == false
        || value["ok"] == false || value.get("error").is_some_and(|v| !v.is_null())
    {
        return Err(format!("{name} returned an error: {}",value));
    }
    if matches!(name,"workspace_write"|"workspace_replace"|"workspace_replace_range"|"file_write")
        && value["written"]!=true
    {
        return Err(format!("{name} produced no confirmed file write: {value}"));
    }
    if matches!(name,"shell_exec"|"terminal_exec") {
        match value.get("exitCode").and_then(Value::as_i64) {
            Some(0) => {}
            other => return Err(format!(
                "{name} failed (exitCode={other:?}). stderr_tail={} stdout_tail={}",
                diagnostic_tail(value["stderr"].as_str().unwrap_or(""),2000),
                diagnostic_tail(value["stdout"].as_str().unwrap_or(""),800),
            ))
        }
    }
    if let Some(code)=value.get("statusCode").and_then(Value::as_u64)
        .or_else(||value.get("status").and_then(Value::as_u64))
    {
        if code>=400 { return Err(format!("{name} HTTP {code}: {value}")); }
    }
    Ok(value)
}

/// A pending human approval must never authorize a command for a goal
/// that was changed or cancelled while the dialog was open.
fn may_dispatch_native_action(job:Option<&Value>, expected_directive:&Value,
    cancelled_session:bool)->bool {
    !cancelled_session && job.is_some_and(|job|
        job["status"]=="running" && job["cancelRequested"]!=true
            && job["nativeDirective"]==*expected_directive)
}

impl Tools for NativeTools {
    fn definitions(&self) -> Vec<ToolSpec> { self.tool_specs.clone() }
    fn execute<'a>(&'a self, action: &'a Action) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            if !self.tools_enabled {
                return Err("The owner disabled tools for this run".into());
            }
            let operation_id=format!("native-op-{}-{}",self.run_id,action.id);
            let op=json!({
                "id": operation_id, "runId": self.run_id,
                "sessionId":self.session, "tool":action.tool,
                "args":action.arguments,"status":"running","outcomeUnknown":true,
                "phase":"requested","startedAt":chrono::Utc::now().to_rfc3339()
            });
            self.state.store.put_if_absent("execution_operation",&op)
                .ok_or("Could not durably record tool intent")?;
            // A crash after this record is never blindly replayed by the job
            // recovery worker; the write-ahead marker remains in the snapshot.
            // A human may steer or stop the task while an approval dialog
            // is open. Never dispatch a tool authorized for the stale goal.
            let planned_directive=self.state.store.get("job",&self.run_id)
                .and_then(|job|job.get("nativeDirective").cloned())
                .unwrap_or(Value::Null);
            let approval_needed=if action.tool.starts_with("mcp__") {
                mcp::requires_approval(&self.state,&action.tool)
            } else { tools::requires_approval(&action.tool) };
            let outcome = if approval_needed && !chat::approve_native_tool(
                &self.state,&self.session,&action.tool,
                &action.arguments,&self.run_id,&operation_id
            ).await {
                Err("Tool execution was not approved".to_string())
            } else if !may_dispatch_native_action(
                self.state.store.get("job",&self.run_id).as_ref(),
                &planned_directive,self.state.is_cancelled(&self.session)) {
                Err("Owner cancelled or changed the task before tool dispatch".into())
            } else if action.tool.starts_with("mcp__") {
                mcp::call(&self.state,&action.tool,&action.arguments).await
                    .and_then(|value|verified_tool_result(&action.tool,value))
            } else {
                let context=tools::ToolContext {
                    workspace:&self.workspace, store:&self.state.store,
                    state:&self.state,session_id:&self.session,
                    agent_id:self.agent.as_deref(),
                    tools_enabled:self.tools_enabled,tools_execution_enabled:self.tools_enabled,
                };
                tools::execute(&context,&action.tool,&action.arguments,&self.run_id).await
                    .and_then(|value|verified_tool_result(&action.tool,value))
            };
            let patch=match &outcome {
                Ok(value)=>json!({"status":"completed","outcomeUnknown":false,"result":value,
                    "finishedAt":chrono::Utc::now().to_rfc3339()}),
                Err(error)=>json!({"status":"failed","outcomeUnknown":false,"error":error,
                    "finishedAt":chrono::Utc::now().to_rfc3339()}),
            };
            self.state.store.patch_if(
                "execution_operation",&operation_id,"status",&json!("running"),&patch
            ).ok_or("Operation result could not be persisted; outcome uncertain")?;
            outcome
        })
    }
}
struct FsEvidence { workspace: PathBuf }
impl FsEvidence {
    fn safe_path(&self, raw: &str) -> Result<PathBuf, String> {
        if raw.is_empty() || raw.len() > 4096 {
            return Err("Invalid verification path".into());
        }
        let relative=Path::new(raw);
        if !relative.components().all(|part|matches!(part,Component::Normal(_))) {
            return Err("Verification cannot escape the workspace".into());
        }
        let root=std::fs::canonicalize(&self.workspace).map_err(|e|e.to_string())?;
        let target=root.join(relative);
        let mut existing=target.as_path();
        while std::fs::symlink_metadata(existing).is_err() {
            existing=existing.parent().ok_or("No existing verification parent")?;
        }
        let canonical=std::fs::canonicalize(existing).map_err(|e|e.to_string())?;
        if !canonical.starts_with(&root) {
            return Err("Verification path escapes the workspace through a symlink".into());
        }
        Ok(target)
    }
}
impl Evidence for FsEvidence {
    fn check<'a>(&'a self, criterion: &'a Criterion) -> BoxFuture<'a,Result<bool,String>> {
        Box::pin(async move {
            let path=self.safe_path(criterion.path.as_deref().unwrap_or(""))?;
            match criterion.kind {
                CriterionKind::FileExists=>Ok(path.is_file()),
                CriterionKind::FileAbsent=>Ok(!path.exists()),
                CriterionKind::FileContains=>{
                    if std::fs::metadata(&path).map_err(|e|e.to_string())?.len()>16*1024*1024 {
                        return Err("Verification file exceeds 16 MiB".into());
                    }
                    let content=std::fs::read_to_string(&path).map_err(|e|e.to_string())?;
                    Ok(content.contains(criterion.contains.as_deref().unwrap_or("")))
                }
                CriterionKind::ToolSucceeded=>Err("Tool success requires a durable tool receipt".into()),
            }
        })
    }
}
struct DurableJournal<'a> { context: &'a JobContext }
impl Journal for DurableJournal<'_> {
    fn save(&self, snapshot:&Snapshot)->Result<(),String> {
        let in_flight=snapshot.model_in_flight || !snapshot.in_flight.is_empty();
        self.context.checkpoint(json!({
            "nativeState":snapshot,
            "nativeInFlight":in_flight,
            "stepIndex":snapshot.step,
            "modelCalls":snapshot.model_calls,
            "estimatedTokens":snapshot.charged_tokens
        }))
    }
    fn cancelled(&self, id: &str)->bool {
        self.context.state.store.get("job",id).is_none_or(|job| {
            job["cancelRequested"]==true || job["status"]=="cancelled" ||
            job["sessionId"].as_str().is_some_and(|session|
                self.context.state.is_cancelled(session))
        })
    }
    fn directive(&self, id: &str)->Result<Option<String>,String> {
        let job=self.context.state.store.get("job",id)
            .ok_or("Running job missing while checking owner directive")?;
        Ok(job["nativeDirective"].as_str().map(str::to_string))
    }
}
fn strategy(value:&Value)->Strategy {
    match value.as_str() {
        Some("low_cost")=>Strategy::LowCost,
        Some("verify_first")=>Strategy::VerifyFirst,
        _=>Strategy::Balanced,
    }
}
/// Tool schema and execution permission come from the Rust registry; the
/// model cannot forge tools or elevate a disabled permission.
async fn allowed_tools(state:&AppState, input:&Value)->Vec<ToolSpec> {
    if input["toolsEnabled"]==false || input["toolsExecutionEnabled"]==false {return vec![];}
    let requested: Option<HashSet<&str>>=input["allowedTools"].as_array().map(|values|
        values.iter().filter_map(Value::as_str).collect());
    let builtin = tools::definitions().into_iter().filter_map(|item| {
        let function=&item["function"];
        let name=function["name"].as_str()?;
        if requested.as_ref().is_some_and(|allowed| !allowed.contains(name)) {return None;}
        let read_only=matches!(name,
            "workspace_list"|"workspace_read"|"workspace_read_range"|"code_search"|
            "file_list"|"file_read"|"memory_search"|"task_status"|"task_result");
        Some(ToolSpec {
            name:name.to_string(),
            description:function["description"].as_str().unwrap_or("").to_string(),
            schema:function["parameters"].clone(),read_only,
        })
    });
    let mut collected: Vec<_> = builtin.collect();
    // MCP tools use the same registry and authorization as interactive chat.
    for item in mcp::tools(state).await {
        let name=item.qualified();
        if requested.as_ref().is_some_and(|allowed| !allowed.contains(name.as_str())) {continue;}
        collected.push(ToolSpec {name,description:item.description,schema:item.schema,
            read_only:false});
    }
    collected
}
/// New default entry for persistent Rust prompt jobs. No separate goals API.
pub async fn run(context:&JobContext,job:&Value)->Result<Value,String> {
    let input=&job["input"];
    let task=input["prompt"].as_str().filter(|text|!text.trim().is_empty())
        .ok_or("Native agent prompt cannot be empty")?;
    let provider=input["providerId"].as_str().unwrap_or("");
    let requested_model=input["model"].as_str();
    let agent=input["agentId"].as_str()
        .and_then(|id|context.state.store.get("agent",id));
    let provider=if provider.is_empty() {
        agent.as_ref().and_then(|a|a["providerId"].as_str()).unwrap_or("")
    } else {provider};
    let chosen_model=requested_model.or_else(||agent.as_ref().and_then(|a|a["model"].as_str()));
    let route=gateway::resolve(&context.state,provider,chosen_model)?;
    let session = if let Some(id)=job["sessionId"].as_str().or_else(||input["sessionId"].as_str()) {
        id.to_string()
    } else {
        context.state.store.put("session",&json!({
            "title":task.chars().take(100).collect::<String>(),
            "agentId":input["agentId"],"providerId":route.id,"model":route.model
        })).and_then(|saved|saved["id"].as_str().map(str::to_string))
            .ok_or("Could not save native agent session")?
    };
    // Recovered work must retain its original uncertainty marker. Reset it
    // only for brand-new jobs, never before checking prior external effects.
    let prior_job=context.state.store.get("job",&context.job_id);
    let previous=prior_job.as_ref().and_then(|j|j.get("nativeState").cloned());
    let uncertain=prior_job.as_ref().is_some_and(|j|j["nativeInFlight"]==true);
    if previous.is_some() {
        context.checkpoint(json!({"sessionId":session}))?;
    } else {
        context.checkpoint(json!({"sessionId":session,"nativeInFlight":false}))?;
    }
    if uncertain {
        return Ok(json!({"status":"needs_review",
            "reason":"재시작 중 도구 또는 모델 호출 결과가 불명확합니다. 기존 실행 기록을 확인하세요.",
            "snapshot":previous}));
    }
    let resume_snapshot=previous.map(|value|serde_json::from_value::<Snapshot>(value)
        .map_err(|e|format!("Invalid native checkpoint: {e}"))).transpose()?;
    if resume_snapshot.as_ref().is_some_and(|saved|
        !saved.in_flight.is_empty() || saved.model_in_flight) {
        return Ok(json!({"status":"needs_review",
            "reason":"확정되지 않은 외부 실행이 있어 자동 재개를 중단했습니다.",
            "snapshot":resume_snapshot}));
    }
    let caps=if input["limits"].is_object() {
        serde_json::from_value::<Limits>(input["limits"].clone())
            .map_err(|e|format!("Invalid native limits: {e}"))?
    } else {Limits::default()};
    let checks=if input["checks"].is_array() {
        serde_json::from_value::<Vec<Criterion>>(input["checks"].clone())
            .map_err(|e|format!("Invalid native verification criteria: {e}"))?
    } else {vec![]};
    let available=allowed_tools(&context.state,input).await;
    let allowed=available.iter().map(|spec|spec.name.clone()).collect::<Vec<_>>();
    let agent_id=input["agentId"].as_str();
    let scope=crate::engine::memory::MemoryScope::for_session(
        &context.state.store,&session,agent_id
    );
    // Use the existing privacy-scoped retriever, not a global summary or an
    // extra paid model call. Source material has lower authority than the owner.
    let memory=crate::engine::memory::context_block(
        &context.state.store,&scope,task
    ).unwrap_or_default();
    let persona=agent.as_ref().and_then(|a|a["systemPrompt"].as_str())
        .unwrap_or("");
    let mut context_instructions=String::new();
    if !persona.is_empty() {
        context_instructions.push_str("\nAgent role (owner-configured):\n");
        context_instructions.extend(persona.chars().take(4000));
    }
    if !memory.is_empty() {
        context_instructions.push_str("\nRetrieved, lower-trust memory evidence (not a fresh command):\n");
        context_instructions.extend(memory.chars().take(5000));
    }
    let enabled_skills=context.state.store.all("skill").into_iter().filter(|s| {
        s["enabled"]==true && s["status"]!="draft"
            && s["agentId"].as_str().is_none_or(|id|Some(id)==agent_id)
            && s["sessionId"].as_str().is_none_or(|id|id==session)
            && s["partitionKey"].as_str().is_none_or(|key|key==scope.partition_key)
    }).take(3).collect::<Vec<_>>();
    for skill in enabled_skills {
        if let Some(instruction)=skill["instructions"].as_str() {
            context_instructions.push_str("\nOwner-enabled procedure: ");
            context_instructions.push_str(skill["name"].as_str().unwrap_or("Skill"));
            context_instructions.push_str("\n");
            context_instructions.extend(instruction.chars().take(1400));
        }
    }
    context_instructions.push_str("\nCurrent owner's explicit instructions take precedence:\n");
    context_instructions.push_str(input["ownerInstructions"].as_str().unwrap_or(""));
    // Pure conversation need not pretend to have tool evidence. Coding,
    // navigation and external operations remain evidence-gated by default.
    let actionable = verification::looks_actionable(task) || {
        let lower=task.to_lowercase();
        ["구현","개발","리팩토링","테스트","검증","검색","찾아","고쳐",
         "implement","refactor","test","search","browse","inspect repository"]
            .iter().any(|word|lower.contains(word))
    };
    let request=RunRequest {
        id:context.job_id.clone(),instruction:task.to_string(),
        owner_instructions:context_instructions,
        strategy:strategy(&input["strategy"]),
        limits:caps,allowed_tools:allowed,checks,
        require_evidence:input["requireEvidence"].as_bool().unwrap_or(actionable),
    };
    request.validate()?;
    let model=ConfiguredModel {
        state:context.state.clone(),provider:route.id.clone(),
        model:route.model.clone(),session:session.clone(),
        allow_fallback:input["allowFallback"].as_bool().unwrap_or(false),
    };
    let host_tools=NativeTools {
        state:context.state.clone(),workspace:context.state.config.workspace.clone(),
        session:session.clone(),agent:input["agentId"].as_str().map(str::to_string),
        run_id:context.job_id.clone(),tool_specs:available,tools_enabled:input["toolsEnabled"]!=false,
    };
    let evidence=FsEvidence{workspace:context.state.config.workspace.clone()};
    let journal=DurableJournal{context};
    if let Some(existing)=context.state.store.get("execution",&context.job_id) {
        if existing["sessionId"].as_str()!=Some(session.as_str()) {
            return Err("Recovered execution session does not match the original".into());
        }
        if existing["status"]=="interrupted" {
            context.state.store.patch_if("execution",&context.job_id,
                "status",&json!("interrupted"),&json!({
                    "status":"running","resumedAt":chrono::Utc::now().to_rfc3339()
                })).ok_or("Could not resume interrupted execution record")?;
        }
    } else {
        context.state.store.put_if_absent("execution",&json!({
            "id":context.job_id,"status":"running","sessionId":session,
            "task":task,"startedAt":chrono::Utc::now().to_rfc3339()
        })).ok_or("Could not persist native execution")?;
    }
    // Keep native SDK tasks visible in the same session store as chat.
    let source_message=json!({
        "id":format!("native-user-{}",context.job_id),
        "session_id":session,"role":"user","content":task,
        "metadata":{"authorKind":"owner","source":"rust-sdk","jobId":context.job_id}
    });
    if context.state.store.message(source_message["id"].as_str().unwrap_or("")).is_none() {
        context.state.store.write_message(&source_message)
            .ok_or("Could not persist owner instruction before native execution")?;
    }
    let kernel=Kernel{model:&model,tools:&host_tools,evidence:&evidence,journal:&journal};
    let snapshot=if let Some(saved)=resume_snapshot {
        kernel.resume(request,saved).await?
    } else {
        kernel.run(request).await?
    };
    let status=serde_json::to_value(&snapshot.status).unwrap_or(json!("needs_review"));
    if let Some(answer)=snapshot.answer.as_deref() {
        let final_message=json!({
            "id":format!("native-answer-{}",context.job_id),
            "session_id":session,"role":"assistant","content":answer,
            "metadata":{"authorKind":"agent","source":"rust-sdk","jobId":context.job_id}
        });
        if context.state.store.message(final_message["id"].as_str().unwrap_or("")).is_none() {
            context.state.store.write_message(&final_message)
                .ok_or("Could not persist final native agent answer")?;
        }
    }
    let persisted=context.state.store.patch_if("execution",&context.job_id,
        "status",&json!("running"),&json!({
            "status":status,"finishedAt":chrono::Utc::now().to_rfc3339(),
            "toolCalls":snapshot.receipts.len(),"modelCalls":snapshot.model_calls,
            "estimatedTokens":snapshot.charged_tokens,"answer":snapshot.answer,
        }));
    if let Some(record)=persisted {context.state.bus.record("execution",&record);}
    Ok(json!({
        "status":status,"sessionId":session,
        "answer":snapshot.answer, "reason":snapshot.error,
        "modelCalls":snapshot.model_calls, "toolCalls":snapshot.receipts.len(),
        "estimatedTokens":snapshot.charged_tokens,
        "checks":snapshot.checks,"receipts":snapshot.receipts
    }))
}


#[cfg(test)]
mod native_evidence_tests {
    use super::*;
    fn temp() -> PathBuf {
        let dir=std::env::temp_dir().join(format!("dominion-native-evidence-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[tokio::test]
    async fn own_workspace_is_verifiable_but_parent_escape_is_not() {
        let root=temp();
        std::fs::write(root.join("output.md"),"verified artifact").unwrap();
        let evidence=FsEvidence{workspace:root.clone()};
        assert!(evidence.check(&Criterion {
            kind:CriterionKind::FileContains,path:Some("output.md".into()),
            contains:Some("verified".into()),tool:None
        }).await.unwrap());
        assert!(evidence.safe_path("../secrets.txt").is_err());
        assert!(evidence.safe_path("/etc/passwd").is_err());
        let _=std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_to_outside_never_satisfy_acceptance() {
        use std::os::unix::fs::symlink;
        let root=temp();
        let other=temp();
        std::fs::write(other.join("secret.txt"),"outside").unwrap();
        symlink(other.join("secret.txt"),root.join("alias")).unwrap();
        symlink(other.join("absent.txt"),root.join("broken")).unwrap();
        let evidence=FsEvidence{workspace:root.clone()};
        assert!(evidence.safe_path("alias").is_err());
        assert!(evidence.safe_path("broken").is_err());
        let _=std::fs::remove_dir_all(root);
        let _=std::fs::remove_dir_all(other);
    }
}

#[cfg(test)]
mod native_result_tests {
    use super::*;
    #[test]
    fn failing_shell_exit_is_not_a_successful_sdk_receipt() {
        assert!(verified_tool_result("shell_exec",json!({"exitCode":1,"stdout":"","stderr":"failed"})).is_err());
        assert!(verified_tool_result("terminal_exec",json!({"exitCode":null,"stderr":"terminated"})).is_err());
        assert!(verified_tool_result("shell_exec",json!({"exitCode":0,"stdout":"OK"})).is_ok());
        let long_stderr=format!("{}END_ERROR", "prelude".repeat(1000));
        let failure=verified_tool_result("shell_exec",json!({
            "exitCode":101,"stdout":"", "stderr":long_stderr
        })).unwrap_err();
        assert!(failure.contains("END_ERROR"));
        assert!(!failure.contains("prelude".repeat(400).as_str()));
        assert!(failure.len()<3600);
    }
    #[test]
    fn wrapped_tool_errors_are_not_success_evidence() {
        assert!(verified_tool_result("http_fetch",json!({"statusCode":503,"body":"fail"})).is_err());
        assert!(verified_tool_result("mcp__server__op",json!({"isError":true})).is_err());
        assert!(verified_tool_result("mcp__server__op",json!({"statusCode":503})).is_err());
        assert!(verified_tool_result("mcp__server__op",json!({"error":"denied"})).is_err());
        assert!(verified_tool_result("workspace_write",json!({"success":false})).is_err());
        assert!(verified_tool_result("workspace_replace_range",
            json!({"written":false,"unchanged":true})).is_err());
        assert!(verified_tool_result("workspace_replace",
            json!({"written":true,"replaced":true})).is_ok());
        assert!(verified_tool_result("workspace_read",json!({"content":"ok"})).is_ok());
    }
    #[test]
    fn pending_approval_respects_live_owner_changes_and_cancel() {
        let original=json!({"status":"running","nativeDirective":"original",
            "cancelRequested":false});
        let expected=json!("original");
        assert!(may_dispatch_native_action(Some(&original),&expected,false));
        let changed=json!({"status":"running","nativeDirective":"new goal"});
        assert!(!may_dispatch_native_action(Some(&changed),&expected,false));
        let cancelled=json!({"status":"running","nativeDirective":"original",
            "cancelRequested":true});
        assert!(!may_dispatch_native_action(Some(&cancelled),&expected,false));
        assert!(!may_dispatch_native_action(Some(&original),&expected,true));
        assert!(!may_dispatch_native_action(None,&expected,false));
    }

}
