use crate::AppState;
mod files;
mod strategy;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::{Component, Path, PathBuf}, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

fn scoped_path(root: &Path, name: &str, write: bool) -> Result<PathBuf, String> {
    if name.is_empty() || name.len() > 512 { return Err("잘못된 파일 경로입니다.".into()); }
    let relative = Path::new(name);
    if relative.is_absolute() ||
        !relative.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("작업공간 밖의 경로는 사용할 수 없습니다.".into());
    }
    let base = root.canonicalize().map_err(|e| e.to_string())?;
    let candidate = base.join(relative);
    let parent = candidate.parent().ok_or("경로에 상위 디렉터리가 없습니다.")?
        .canonicalize().map_err(|e| e.to_string())?;
    if !parent.starts_with(&base) { return Err("작업공간 경계를 벗어났습니다.".into()); }
    if candidate.exists() {
        if !candidate.canonicalize().map_err(|e| e.to_string())?.starts_with(&base) {
            return Err("외부 파일 링크는 사용할 수 없습니다.".into());
        }
        if !candidate.is_file() { return Err("일반 파일만 사용할 수 있습니다.".into()); }
    } else if !write {
        return Err("파일이 존재하지 않습니다.".into());
    }
    Ok(candidate)
}

fn specs(allow_write: bool, allow_shell: bool) -> Vec<Value> {
    let mut result = vec![json!({
        "type":"function","function":{
            "name":"workspace_read",
            "description":"Read a UTF-8 file within the configured workspace.",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}
        }
    })];
    result.push(json!({
        "type":"function","function":{
            "name":"workspace_list",
            "description":"Discover up to 200 source files recursively in the workspace. Use before reading files.",
            "parameters":{"type":"object","properties":{"path":{"type":"string","description":"Relative folder or '.'"}},"required":["path"],"additionalProperties":false}
        }
    }));
    result.push(json!({
        "type":"function","function":{
            "name":"workspace_search",
            "description":"Find exact text in bounded workspace source files, returning paths and line previews.",
            "parameters":{"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"}},"required":["query","path"],"additionalProperties":false}
        }
    }));
    if allow_write {
        result.push(json!({
            "type":"function","function":{
                "name":"workspace_replace",
                "description":"Replace one exact UTF-8 source span, refusing ambiguous or stale edits. Optional sha256 locks the input revision.",
                "parameters":{"type":"object","properties":{
                    "path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"},
                    "sha256":{"type":"string"}
                },"required":["path","old","new"],"additionalProperties":false}
            }
        }));
        result.push(json!({
            "type":"function","function":{
                "name":"workspace_write",
                "description":"Write a UTF-8 file and verify its SHA-256 by reading it back. File's parent directory must exist.",
                "parameters":{"type":"object","properties":{
                    "path":{"type":"string"},"content":{"type":"string"}
                },"required":["path","content"],"additionalProperties":false}
            }
        }));
    }
    if allow_shell {
        result.push(json!({
            "type":"function","function":{
                "name":"workspace_check",
                "description":"Run an explicitly permitted local test, build, or read-only inspection command.",
                "parameters":{"type":"object","properties":{
                    "program":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}
                },"required":["program","args"],"additionalProperties":false}
            }
        }));
    }
    result
}

fn allowed_check(program: &str, args: &[String]) -> bool {
    match (program, args.first().map(String::as_str)) {
        ("cargo", Some("check" | "test" | "fmt")) => true,
        ("npm", Some("test" | "run")) => {
            args.first().is_some_and(|a| a == "test") ||
                (args.len() == 2 && matches!(args[1].as_str(), "build" | "typecheck"))
        }
        ("git", Some("status" | "diff")) => args.iter().skip(1)
            .all(|v| matches!(v.as_str(),"--short"|"--stat"|"--no-ext-diff")),
        ("node", Some("--test")) => true,
        ("rg", Some(pattern)) => args.len()==1 && !pattern.starts_with('-'),
        _ => false,
    }
}

async fn tool(state: &AppState, name: &str, args: &Value,
    allow_write: bool, allow_shell: bool) -> Result<Value, String> {
    match name {
        "workspace_list" => {
            files::list(&state.workspace, args["path"].as_str().unwrap_or("."))
        }
        "workspace_search" => {
            let query = args["query"].as_str().ok_or("검색할 문자열이 필요합니다.")?;
            files::search(&state.workspace, query, args["path"].as_str().unwrap_or("."))
        }
        "workspace_replace" if allow_write => {
            let path = args["path"].as_str().ok_or("수정할 파일 경로가 필요합니다.")?;
            let old = args["old"].as_str().ok_or("변경 전 정확한 문자열이 필요합니다.")?;
            let new = args["new"].as_str().ok_or("변경할 새 문자열이 필요합니다.")?;
            files::replace(&state.workspace, path, old, new, args["sha256"].as_str()).await
        }
        "workspace_read" => {
            let path = args["path"].as_str().ok_or("파일 경로가 필요합니다.")?;
            let file = scoped_path(&state.workspace, path, false)?;
            let bytes = tokio::fs::read(&file).await.map_err(|e| e.to_string())?;
            if bytes.len() > 65_536 { return Err("파일 크기가 읽기 한도를 초과했습니다.".into()); }
            let text = String::from_utf8(bytes).map_err(|_| "UTF-8 파일만 읽을 수 있습니다.")?;
            Ok(json!({"path":path,"content":text}))
        }
        "workspace_write" if allow_write => {
            let path = args["path"].as_str().ok_or("파일 경로가 필요합니다.")?;
            let content = args["content"].as_str().ok_or("파일 내용이 필요합니다.")?;
            if content.len() > 131_072 { return Err("쓰기 크기 한도를 초과했습니다.".into()); }
            let file = scoped_path(&state.workspace, path, true)?;
            tokio::fs::write(&file, content).await.map_err(|e| e.to_string())?;
            let observed = tokio::fs::read(&file).await.map_err(|e| e.to_string())?;
            let expected = Sha256::digest(content.as_bytes());
            let actual = Sha256::digest(&observed);
            if actual != expected { return Err("작성 후 파일 검증이 실패했습니다.".into()); }
            Ok(json!({"path":path,"bytes":observed.len(),
                "sha256":format!("{actual:x}"),"verified":true}))
        }
        "workspace_check" if allow_shell => {
            let program = args["program"].as_str().ok_or("프로그램 이름이 필요합니다.")?;
            let params = args["args"].as_array().ok_or("인수 배열이 필요합니다.")?
                .iter().map(|v| v.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>()
                .ok_or("인수는 문자열만 허용합니다.")?;
            if !allowed_check(program, &params) || params.len() > 20 ||
                params.iter().any(|v| v.len()>256 || v.starts_with('-') && v.contains('=') && v.contains('/')) {
                return Err("허용되지 않은 검사 명령입니다.".into());
            }
            let mut command = Command::new(program);
            command.args(&params)
                .current_dir(&state.workspace).stdin(Stdio::null())
                .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
            if program=="git" {
                command.env_remove("GIT_EXTERNAL_DIFF");
                if params.first().is_some_and(|s|s=="diff") {
                    command.arg("--no-ext-diff");
                }
            }
            let output = timeout(Duration::from_secs(90), command.output()).await
                .map_err(|_| "검사 명령 제한 시간이 초과되었습니다.".to_string())?
                .map_err(|e| e.to_string())?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(json!({
                "program":program,"args":params,"success":output.status.success(),
                "exitCode":output.status.code(),
                "stdout":stdout.chars().take(16_000).collect::<String>(),
                "stderr":stderr.chars().take(8_000).collect::<String>()
            }))
        }
        _ => Err("이 작업에서는 해당 도구 실행 권한이 없습니다.".into()),
    }
}

async fn completion(state: &AppState, messages: &[Value], tools: &[Value])
    -> Result<Value, String> {
    let body = if tools.is_empty() {
        json!({"model":state.model_name,"messages":messages,"stream":false})
    } else {
        json!({"model":state.model_name,"messages":messages,
            "tools":tools,"tool_choice":"auto","stream":false})
    };
    let mut request = state.http.post(&state.model_url).json(&body);
    if let Some(key) = &state.api_key { request = request.bearer_auth(key); }
    let response = request.send().await.map_err(|e| format!("모델 연결 오류: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("모델 HTTP {}: {}",status.as_u16(),body.chars().take(500).collect::<String>()));
    }
    response.json::<Value>().await.map_err(|e| format!("모델 응답 형식 오류: {e}"))
}

fn settle(state: &AppState, id: &str, status: &str, answer: &str,
    verified_writes: u64, error: Option<String>) {
    let result = json!({"answer":answer,"verifiedWrites":verified_writes,
        "verification":"workspace_readback_sha256"});
    if let Some(task) = state.store.finish(id,status,result,error) {
        state.emit(id,"finished",json!({"status":task["status"],
            "result":task["result"],"error":task["error"]}));
    }
}

pub async fn run(state: AppState, id: String) {
    if !state.store.claim(&id) { return; }
    let Some(task) = state.store.get(&id) else { return };
    let input = &task["input"];
    let prompt = input["prompt"].as_str().unwrap_or("").to_string();
    let session = input["sessionId"].as_str().unwrap_or(&id).to_string();
    let allow_write = input["allowWrite"] == true;
    let allow_shell = input["allowShell"] == true;
    let allow_mcp = input["allowMcp"] == true;
    let require_evidence = input["requireEvidence"] == true;
    let steps = input["maxSteps"].as_u64().unwrap_or(12).clamp(1,24);
    let token_budget = input["tokenBudget"].as_u64().unwrap_or(40_000).clamp(256,500_000);
    let mut available = specs(allow_write,allow_shell);
    if allow_mcp {
        available.extend(state.mcp.definitions().await);
    }
    let mut messages = vec![json!({"role":"system",
        "content":"You are DOMINION, a local-first agent. Obey the latest owner's instruction. Use the offered tools for real work; never fabricate a file edit, command output, or verification. A failed operation must be described accurately. You cannot call unadvertised tools."})];
    messages.extend(state.store.history(&session, 12));
    messages.push(json!({"role":"user","content":prompt}));
    state.store.add_message(&session,"user",&prompt);
    state.emit(&id,"running",json!({"sessionId":session,"model":state.model_name}));
    let mut verified_writes = 0_u64;
    let mut total_calls = 0_u64;
    let mut receipts: Vec<Value> = Vec::new();
    let mut active_instruction = prompt.clone();
    let mut owner_redirected = false;
    let planning = strategy::PlanningPolicy::from_setting(
        input["planningMode"].as_str().unwrap_or("auto")
    );

    // Planning is selective: simple questions avoid the extra paid turn.
    // Its output is guidance; user instructions and tool receipts stay primary.
    if planning.should_plan(&prompt) && !state.store.cancelled(&id) {
        match completion(&state,&strategy::make_plan_messages(&prompt),&[]).await {
            Ok(planning_response) => {
                let tokens = planning_response["usage"]["total_tokens"].as_u64().unwrap_or(0);
                state.store.account_aux_tokens(&id,tokens);
                if let Some(plan) = strategy::extract_plan(&planning_response) {
                    messages.insert(1,json!({"role":"system","content":format!(
                        "Provisional execution checklist. Adapt to observed results.\\n{plan}"
                    )}));
                    state.emit(&id,"plan",json!({"text":plan,"tokens":tokens}));
                }
            }
            Err(error) => state.emit(&id,"plan_error",json!({"message":error})),
        }
    }

    for step in 0..steps {
        if state.store.cancelled(&id) {
            settle(&state,&id,"cancelled","",verified_writes,None);
            return;
        }
        if let Some(instruction) = state.store.take_steer(&id) {
            active_instruction = instruction.clone();
            owner_redirected = true;
            messages.push(json!({"role":"user","content":instruction}));
            state.store.add_message(&session,"user",&instruction);
            state.emit(&id,"instruction_applied",json!({"text":instruction}));
        }
        let consumed = state.store.get(&id)
            .and_then(|v| v["tokens"].as_u64()).unwrap_or(0);
        if consumed >= token_budget {
            settle(&state,&id,"needs_review","",verified_writes,
                Some("전체 토큰 예산을 소진했습니다.".into()));
            return;
        }
        let compacted = strategy::prune_tool_context(&mut messages,32_000);
        if compacted > 0 {
            state.emit(&id,"context_compacted",json!({"oldToolResults":compacted}));
        }
        state.emit(&id,"model_step",json!({"step":step+1,"limit":steps}));
        let response = match completion(&state,&messages,&available).await {
            Ok(value) => value,
            Err(error) => {
                settle(&state,&id,"failed","",verified_writes,Some(error));
                return;
            }
        };
        let tokens = response["usage"]["total_tokens"].as_u64().unwrap_or(0);
        state.store.measure(&id,tokens);
        if state.store.cancelled(&id) {
            settle(&state,&id,"cancelled","",verified_writes,None);
            return;
        }
        let Some(message) = response["choices"][0]["message"].as_object() else {
            settle(&state,&id,"failed","",verified_writes,
                Some("모델 응답에 메시지가 없습니다.".into()));
            return;
        };
        let message = Value::Object(message.clone());
        let calls = message["tool_calls"].as_array().cloned().unwrap_or_default();
        // A correction arriving during an in-flight model request supersedes
        // every not-yet-started tool call. Keep tool/result pairs complete.
        if let Some(instruction) = state.store.take_steer(&id) {
            active_instruction = instruction.clone();
            owner_redirected = true;
            messages.push(message.clone());
            for call in &calls {
                let tool_id=call["id"].as_str().unwrap_or("unidentified");
                let result=json!({"ok":false,"skipped":true,"reason":"owner_instruction_changed"});
                messages.push(json!({"role":"tool","tool_call_id":tool_id,
                    "content":result.to_string()}));
                state.emit(&id,"tool_skipped",json!({"callId":tool_id,"reason":"owner_instruction_changed"}));
            }
            messages.push(json!({"role":"user","content":instruction}));
            state.store.add_message(&session,"user",&instruction);
            state.emit(&id,"instruction_applied",json!({"text":instruction}));
            continue;
        }
        if calls.is_empty() {
            let text = message["content"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                settle(&state,&id,"needs_review","",verified_writes,
                    Some("모델이 최종 내용을 제공하지 않았습니다.".into()));
                return;
            }

            // Deterministic evidence gate: model claims never prove a write.
            let required_write = require_evidence || (allow_write && !owner_redirected &&
                strategy::source_change_requested(&active_instruction));
            if required_write && verified_writes == 0 {
                state.emit(&id,"verification_gap",json!({
                    "reason":"requested source change lacks verified write receipt"
                }));
                if step + 1 < steps {
                    messages.push(message);
                    messages.push(json!({"role":"user","content":
                        "Your response does not satisfy the requested file change: no verified write/edit receipt exists. Inspect source and perform the edit with actual tools, or report the blocking error."}));
                    continue;
                }
                settle(&state,&id,"needs_review",&text,verified_writes,
                    Some("요청한 변경을 입증하는 실제 파일 수정 기록이 없습니다.".into()));
                return;
            }
            // For complex work a separate model critique may demand further
            // corrections. It cannot override the native evidence check.
            if strategy::complex(&active_instruction) && strategy::action_requested(&active_instruction)
                && !receipts.is_empty() {
                let review = completion(&state,
                    &strategy::review_messages(&active_instruction,&text,&receipts),&[]).await;
                match review {
                    Ok(verdict) => {
                        let tokens=verdict["usage"]["total_tokens"].as_u64().unwrap_or(0);
                        state.store.account_aux_tokens(&id,tokens);
                        let review=strategy::parse_review(&verdict);
                        state.emit(&id,"verification",json!({
                            "complete":review.as_ref().map(|item|item.complete),
                            "feedback":review.as_ref().map(|item|item.feedback.as_str()),
                            "tokens":tokens
                        }));
                        if !review.as_ref().is_some_and(|item|item.complete) {
                            let correction=review
                                .map(|item|item.feedback)
                                .filter(|text|!text.is_empty())
                                .unwrap_or_else(||"Reviewer did not provide valid completion evidence".into());
                            if step + 1 < steps {
                                messages.push(message);
                                messages.push(json!({"role":"user","content":format!(
                                    "Independent review found unfinished work: {correction}. Reinspect actual files or test results and repair the work before responding."
                                )}));
                                continue;
                            }
                            settle(&state,&id,"needs_review",&text,verified_writes,Some(correction));
                            return;
                        }
                    }
                    Err(error) => {
                        settle(&state,&id,"needs_review",&text,verified_writes,
                            Some(format!("독립 검증 모델 오류: {error}")));
                        return;
                    }
                }
            }
            state.store.add_message(&session,"assistant",&text);
            settle(&state,&id,"completed",&text,verified_writes,None);
            return;
        }
        messages.push(message);
        let mut pending_correction:Option<String>=None;
        for call in calls {
            if state.store.cancelled(&id) {
                settle(&state,&id,"cancelled","",verified_writes,None);
                return;
            }
            total_calls += 1;
            if total_calls > 64 {
                settle(&state,&id,"needs_review","",verified_writes,
                    Some("도구 호출 한도를 초과했습니다.".into()));
                return;
            }
            let tool_name = call["function"]["name"].as_str().unwrap_or("");
            let tool_id = call["id"].as_str().unwrap_or("unidentified").to_string();
            // The owner may redirect execution while earlier calls in the
            // same batch are running. Skip every remaining unstarted effect.
            if pending_correction.is_none() {
                pending_correction=state.store.take_steer(&id);
            }
            if pending_correction.is_some() {
                let value=json!({"ok":false,"skipped":true,"reason":"owner_instruction_changed"});
                state.emit(&id,"tool_skipped",json!({"name":tool_name,"callId":tool_id}));
                messages.push(json!({"role":"tool","tool_call_id":tool_id,
                    "content":value.to_string()}));
                continue;
            }
            let arguments = call["function"]["arguments"].as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or(Value::Null);
            // Persist intent before every possible external effect. Crash
            // recovery quarantines incomplete runs rather than replaying them.
            if let Err(error)=state.record_event(&id,"tool_start",
                json!({"name":tool_name,"callId":tool_id,"arguments":arguments})) {
                settle(&state,&id,"needs_review","",verified_writes,
                    Some(format!("도구 실행 기록을 저장하지 못했습니다: {error}")));
                return;
            }
            let outcome = if tool_name.starts_with("mcp__") && allow_mcp {
                state.mcp.call(&crate::mcp::ToolCall {
                    id:tool_id.clone(),name:tool_name.into(),arguments:arguments.clone()
                }).await.map(|response|json!({"content":response}))
            } else {
                tool(&state,tool_name,&arguments,allow_write,allow_shell).await
            }.and_then(|receipt| {
                if tool_name == "workspace_check" && receipt["success"] == false {
                    Err(format!("검사 명령 실패: {receipt}"))
                } else { Ok(receipt) }
            });
            if matches!(tool_name, "workspace_write" | "workspace_replace") && outcome.as_ref()
                .is_ok_and(|v|v["verified"]==true) {
                verified_writes += 1;
            }
            let value = match outcome {
                Ok(receipt) => json!({"ok":true,"receipt":receipt}),
                Err(error) => json!({"ok":false,"error":error}),
            };
            if let Err(error)=state.record_event(&id,"tool_result",
                json!({"name":tool_name,"callId":tool_id,"outcome":value})) {
                settle(&state,&id,"needs_review","",verified_writes,
                    Some(format!("도구 결과가 기록되지 않았습니다. 재실행하지 마세요: {error}")));
                return;
            }
            receipts.push(json!({"tool":tool_name,"outcome":value}));
            if receipts.len()>80 { receipts.remove(0); }
            messages.push(json!({"role":"tool","tool_call_id":tool_id,
                "content":value.to_string()}));
        }
        if let Some(instruction)=pending_correction {
            active_instruction = instruction.clone();
            owner_redirected = true;
            messages.push(json!({"role":"user","content":instruction}));
            state.store.add_message(&session,"user",&instruction);
            state.emit(&id,"instruction_applied",json!({"text":instruction}));
        }
    }
    settle(&state,&id,"needs_review","",verified_writes,
        Some("실행 단계 한도에 도달했습니다. 작업 기록을 검토하세요.".into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_never_accepts_parent_or_absolute_paths() {
        let dir = std::env::temp_dir();
        assert!(scoped_path(&dir,"../escape.txt",true).is_err());
        assert!(scoped_path(&dir,"/etc/passwd",false).is_err());
    }

    #[test]
    fn local_commands_are_explicit_and_bounded() {
        assert!(allowed_check("cargo",&["test".into()]));
        assert!(allowed_check("git",&["diff".into()]));
        assert!(!allowed_check("bash",&["-c".into(),"curl example.com".into()]));
        assert!(!allowed_check("git",&["reset".into(),"--hard".into()]));
    }

    async fn mock_provider(slow:bool) -> (String, std::sync::Arc<tokio::sync::Notify>) {
        use axum::{routing::post, Json, Router};
        let arrival=std::sync::Arc::new(tokio::sync::Notify::new());
        let signal=arrival.clone();
        let app=Router::new().route("/v1/chat/completions",post(
            move |Json(input):Json<Value>| {
                let signal=signal.clone();
                async move {
                    signal.notify_one();
                    if slow { tokio::time::sleep(Duration::from_millis(120)).await; }
                    let history=input["messages"].as_array().cloned().unwrap_or_default();
                    let corrected=history.iter().any(|m|
                        m["role"]=="user" && m["content"]=="Do not write anything");
                    let performed=history.iter().any(|m|m["role"]=="tool");
                    if corrected || performed {
                        Json(json!({"choices":[{"message":{"role":"assistant","content":"Checked and finished."}}],
                            "usage":{"total_tokens":52}}))
                    } else {
                        let args=json!({"path":"verified.txt","content":"native readback"}).to_string();
                        Json(json!({"choices":[{"message":{"role":"assistant","content":null,
                            "tool_calls":[{"id":"write-1","type":"function","function":{
                                "name":"workspace_write","arguments":args}}]}}],
                            "usage":{"total_tokens":83}}))
                    }
                }
            }
        ));
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener,app).await.unwrap(); });
        (url,arrival)
    }

    fn mock_state(url:String) -> (AppState,std::path::PathBuf) {
        let root=std::env::temp_dir().join(
            format!("dominion-runtime-test-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store=crate::store::Store::open(&root.join("state.sqlite")).unwrap();
        let (bus,_)=tokio::sync::broadcast::channel(128);
        (AppState {store,bus,http:reqwest::Client::new(),
            mcp:std::sync::Arc::new(crate::mcp::Mcp::new(vec![]).unwrap()),
            workspace:root.clone(),model_url:url,model_name:"mock".into(),
            api_key:None},root)
    }

    #[tokio::test]
    async fn mock_model_writes_file_and_rust_verifies_real_bytes() {
        let (url,_)=mock_provider(false).await;
        let (state,root)=mock_state(url);
        let input=json!({"prompt":"Create verified.txt","sessionId":null,
            "allowWrite":true,"allowShell":false,"requireEvidence":true,
            "maxSteps":4,"tokenBudget":2000});
        let (task,created)=state.store.submit("mock-write-2026",&input).unwrap();
        assert!(created);
        let id=task["id"].as_str().unwrap().to_string();
        run(state.clone(),id.clone()).await;
        let saved=state.store.get(&id).unwrap();
        assert_eq!(saved["status"],"completed");
        assert_eq!(saved["result"]["verifiedWrites"],1);
        assert_eq!(std::fs::read_to_string(root.join("verified.txt")).unwrap(),"native readback");
        let events=state.store.events_after(&id,0,100);
        assert!(events.iter().any(|e|e["type"]=="tool_result"));
        assert!(events.iter().any(|e|e["type"]=="finished"));
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mid_request_owner_instruction_prevents_unstarted_file_write() {
        let (url,arrival)=mock_provider(true).await;
        let (state,root)=mock_state(url);
        let input=json!({"prompt":"Create verified.txt","sessionId":null,
            "allowWrite":true,"allowShell":false,"requireEvidence":false,
            "maxSteps":4,"tokenBudget":2000});
        let (task,_)=state.store.submit("mock-steer-2026",&input).unwrap();
        let id=task["id"].as_str().unwrap().to_string();
        let next=state.clone();let task_id=id.clone();
        let runner=tokio::spawn(async move {run(next,task_id).await;});
        tokio::time::timeout(Duration::from_secs(3),arrival.notified()).await.unwrap();
        state.store.steer(&id,"Do not write anything").unwrap();
        runner.await.unwrap();
        let saved=state.store.get(&id).unwrap();
        assert_eq!(saved["status"],"completed");
        assert!(!root.join("verified.txt").exists());
        let events=state.store.events_after(&id,0,100);
        assert!(events.iter().any(|e|e["type"]=="tool_skipped"));
        assert!(events.iter().any(|e|e["type"]=="instruction_applied"));
        let _=std::fs::remove_dir_all(root);
    }

}
