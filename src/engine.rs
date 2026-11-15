use crate::AppState;
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
    if allow_write {
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
        ("git", Some("status" | "diff")) => true,
        ("node", Some("--test")) => true,
        ("rg", Some(_)) => true,
        _ => false,
    }
}

async fn tool(state: &AppState, name: &str, args: &Value,
    allow_write: bool, allow_shell: bool) -> Result<Value, String> {
    match name {
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
    let require_evidence = input["requireEvidence"] == true;
    let steps = input["maxSteps"].as_u64().unwrap_or(12).clamp(1,24);
    let token_budget = input["tokenBudget"].as_u64().unwrap_or(40_000).clamp(256,500_000);
    let available = specs(allow_write,allow_shell);
    let mut messages = vec![json!({"role":"system",
        "content":"You are DOMINION, a local-first agent. Obey the latest owner's instruction. Use the offered tools for real work; never fabricate a file edit, command output, or verification. A failed operation must be described accurately. You cannot call unadvertised tools."})];
    messages.extend(state.store.history(&session, 12));
    messages.push(json!({"role":"user","content":prompt}));
    state.store.add_message(&session,"user",&prompt);
    state.emit(&id,"running",json!({"sessionId":session,"model":state.model_name}));
    let mut verified_writes = 0_u64;
    let mut total_calls = 0_u64;

    for step in 0..steps {
        if state.store.cancelled(&id) {
            settle(&state,&id,"cancelled","",verified_writes,None);
            return;
        }
        if let Some(instruction) = state.store.take_steer(&id) {
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
        if calls.is_empty() {
            let text = message["content"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                settle(&state,&id,"needs_review","",verified_writes,
                    Some("모델이 최종 내용을 제공하지 않았습니다.".into()));
                return;
            }
            state.store.add_message(&session,"assistant",&text);
            let complete = !require_evidence || verified_writes > 0;
            settle(&state,&id,if complete {"completed"} else {"needs_review"},
                &text,verified_writes,
                (!complete).then_some("실제 파일 수정 증거가 없습니다.".into()));
            return;
        }
        messages.push(message);
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
            let arguments = call["function"]["arguments"].as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or(Value::Null);
            state.emit(&id,"tool_start",json!({"name":tool_name,"callId":tool_id}));
            let outcome = tool(&state,tool_name,&arguments,allow_write,allow_shell).await;
            if tool_name == "workspace_write" && outcome.as_ref()
                .is_ok_and(|v|v["verified"]==true) {
                verified_writes += 1;
            }
            let value = match outcome {
                Ok(receipt) => json!({"ok":true,"receipt":receipt}),
                Err(error) => json!({"ok":false,"error":error}),
            };
            state.emit(&id,"tool_result",json!({"name":tool_name,"callId":tool_id,
                "outcome":value}));
            messages.push(json!({"role":"tool","tool_call_id":tool_id,
                "content":value.to_string()}));
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
}
