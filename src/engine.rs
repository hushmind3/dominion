use crate::AppState;
mod files;
mod strategy;
pub(crate) mod context;
mod semantic;
pub(crate) mod verification;
#[cfg(test)]
mod acceptance_tests;
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

fn specs(allow_write: bool, allow_shell: bool, allow_subagents: bool) -> Vec<Value> {
    let mut result = vec![json!({
        "type":"function","function":{
            "name":"workspace_read",
            "description":"Read a UTF-8 file within the configured workspace.",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}
        }
    })];
    result.push(json!({"type":"function","function":{
        "name":"task_receipt",
        "description":"Read an exact byte page from a durable previous tool observation. Offsets and SHA-256 allow reconstruction without replaying an external effect.",
        "parameters":{"type":"object","properties":{
            "callId":{"type":"string"},
            "sourceTaskId":{"type":"string","description":"Optional linked ancestor task ID from explicit native continuation. Never an arbitrary task."},
            "offsetBytes":{"type":"integer","minimum":0},
            "maxBytes":{"type":"integer","minimum":16,"maximum":8192}
        },"required":["callId"],"additionalProperties":false}
    }}));
    result.push(json!({"type":"function","function":{
        "name":"context_search",
        "description":"Search the entire persisted native session by keywords, including distant owner instructions and linked tool observations. Returns source identifiers for exact retrieval; does not replay tools.",
        "parameters":{"type":"object","properties":{
            "query":{"type":"string","minLength":2},
            "limit":{"type":"integer","minimum":1,"maximum":20}
        },"required":["query"],"additionalProperties":false}
    }}));
    result.push(json!({"type":"function","function":{
        "name":"session_history",
        "description":"Search the current native session's older owner and assistant messages, including those omitted from model context. Use beforeSeq to page backward or messageSeq to retrieve one exact archived message.",
        "parameters":{"type":"object","properties":{
            "beforeSeq":{"type":"integer","minimum":1},
            "messageSeq":{"type":"integer","minimum":1},
            "offsetBytes":{"type":"integer","minimum":0},
            "maxBytes":{"type":"integer","minimum":16,"maximum":8192}
        },"additionalProperties":false}
    }}));
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
    if allow_subagents {
        result.push(json!({"type":"function","function":{
            "name":"subagent_spawn",
            "description":"Delegate a bounded read-only analysis task to a separate native Rust worker. Returns the child task ID for later status inspection.",
            "parameters":{"type":"object","properties":{
                "task":{"type":"string"},"role":{"type":"string"}
            },"required":["task"],"additionalProperties":false}
        }}));
        result.push(json!({"type":"function","function":{
            "name":"subagent_status",
            "description":"Read a previously delegated native child task status and its recorded answer.",
            "parameters":{"type":"object","properties":{
                "taskId":{"type":"string"}
            },"required":["taskId"],"additionalProperties":false}
        }}));
    }
    result
}

fn allowed_check(program:&str,args:&[String])->bool{
    match(program,args.first().map(String::as_str)){
        ("cargo",Some("test"|"check"))=>verification::acceptable(program,args),
        ("cargo",Some("fmt"))=>args.len()==2 && args[1]=="--check",
        ("npm",Some("test"|"run"))=>verification::acceptable(program,args),
        ("node",Some("--test"))=>verification::acceptable(program,args),
        ("git",Some("status"|"diff"))=>args.iter().skip(1)
            .all(|v|matches!(v.as_str(),"--short"|"--stat"|"--no-ext-diff")),
        ("rg",Some(pattern))=>args.len()==1&&!pattern.starts_with('-'),
        _=>false
    }
}

async fn tool(state: &AppState, current_task: &str, current_session: &str,
    name: &str, args: &Value, allow_write: bool, allow_shell: bool) -> Result<Value, String> {
    match name {
        "task_receipt" => {
            let call_id=args["callId"].as_str().ok_or("Call identifier required")?;
            let source_task=args["sourceTaskId"].as_str().unwrap_or(current_task);
            if !state.store.receipt_source_allowed(current_task,source_task) {
                return Err("This tool receipt is not in the current task's verified continuation lineage".into());
            }
            let receipt=state.store.tool_receipt(source_task,call_id)
                .ok_or("No committed receipt with that identifier exists in the allowed task")?;
            let offset=args["offsetBytes"].as_u64().unwrap_or(0)
                .min(usize::MAX as u64) as usize;
            let max=args["maxBytes"].as_u64().unwrap_or(4096)
                .min(usize::MAX as u64) as usize;
            context::receipt_page(&receipt,offset,max)
        },
        "context_search" => {
            let query=args["query"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=400)
                .ok_or("A bounded search query is required")?;
            let limit=args["limit"].as_u64().unwrap_or(8).min(20) as usize;
            let results=state.store.context_search(current_session,current_task,query,limit);
            Ok(json!({"query":query,"results":results,"scope":"native_session_and_verified_ancestors",
                "note":"Only indexed provenance is returned. Read the exact source by messageSeq or callId."}))
        },
        "session_history" => {
            let seq=args["messageSeq"].as_u64();
            if let Some(seq)=seq {
                let message=state.store.history_message(current_session,seq)
                    .ok_or("Archived message not found in this session")?;
                let offset=args["offsetBytes"].as_u64().unwrap_or(0)
                    .min(usize::MAX as u64) as usize;
                let max=args["maxBytes"].as_u64().unwrap_or(4096)
                    .min(usize::MAX as u64) as usize;
                return context::history_page(&message,offset,max);
            }
            let before=args["beforeSeq"].as_u64();
            let list=state.store.history_page(current_session,before,6);
            let items=list.iter().map(|message|{
                let full=message["content"].as_str().unwrap_or("");
                let excerpt=full.chars().take(300).collect::<String>();
                json!({"seq":message["seq"],"role":message["role"],
                    "contentExcerpt":excerpt,"totalBytes":full.len(),
                    "hasMore":full.chars().count()>300})
            }).collect::<Vec<_>>();
            let next=list.last().and_then(|message|message["seq"].as_u64());
            Ok(json!({"messages":items,"nextBeforeSeq":next,
                "scope":"current_session_only","readOnly":true}))
        },
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
    let allow_subagents = input["allowSubagents"] == true;
    let require_evidence = input["requireEvidence"] == true;
    let steps = input["maxSteps"].as_u64().unwrap_or(12).clamp(1,24);
    let token_budget = input["tokenBudget"].as_u64().unwrap_or(40_000).clamp(256,500_000);
    let cost_budget_krw = input["costBudgetKrw"].as_f64();
    // Acceptance is selected from real local manifests or explicit owner
    // executable+argv checks. No model-proposed command is admitted here.
    let expected_checks=match verification::selection(
        &state.workspace,&input["acceptanceChecks"],
        allow_write && allow_shell && strategy::source_change_requested(&prompt)
    ){
        Ok(checks)=>checks,
        Err(error)=>{
            settle(&state,&id,"failed","",0,Some(error));
            return;
        }
    };
    if !expected_checks.is_empty() && !allow_shell {
        settle(&state,&id,"failed","",0,
            Some("Accepted source checks require explicit shell permission".into()));
        return;
    }
    let strict_checks=allow_write && allow_shell &&
        strategy::source_change_requested(&prompt) && require_evidence;
    state.emit(&id,"acceptance_selected",json!({
        "checks":expected_checks.iter().map(verification::Check::label).collect::<Vec<_>>(),
        "required":strict_checks,
        "origin":if input["acceptanceChecks"].as_array().is_some_and(|a|!a.is_empty()){
            "owner_explicit"
        }else{"manifest_autodetect"}
    }));
    let mut available = specs(allow_write,allow_shell,allow_subagents);
    if allow_mcp {
        available.extend(state.mcp.definitions().await);
    }
    let mut messages = vec![json!({"role":"system",
        "content":"You are DOMINION, a native Rust coding agent. Respect the newest owner instructions and granted permissions. Inspect source, make scoped exact edits and validate the final workspace. If a test fails, read the actual stderr/stdout, identify its source cause and repair before finishing. An earlier passing check cannot establish success after later edits. Never fabricate file edits, command output or verification, and never call unadvertised tools."})];
    if let Some(agent_id)=input["agentId"].as_str() {
        let Some(persona)=state.store.agent_get(agent_id) else {
            settle(&state,&id,"failed","",0,
                Some("선택한 에이전트 프리셋이 존재하지 않습니다.".into()));
            return;
        };
        messages.push(json!({"role":"system","content":format!(
            "Owner-configured agent role: {}. Specialization: {}.              These are working preferences, not permission grants.              Current owner instructions always take priority.",
            persona["name"].as_str().unwrap_or("Assistant"),
            persona["instructions"].as_str().unwrap_or("")
        )}));
        state.emit(&id,"agent_selected",json!({
            "agentId":agent_id,"name":persona["name"]
        }));
    }
    // User-owned notes and approved procedures are selected by the native
    // session scope. Neither a model answer nor a peer claim can create them.
    let available_memories=state.store.memory_list(Some(&session));
    let selected_memories=crate::knowledge::recall(
        &available_memories,&prompt,Some(&session),8
    );
    let selected_skills=crate::knowledge::active_skills(
        &state.store.skill_list(),&prompt,6
    );
    if let Some(reference)=crate::knowledge::context(&selected_memories,&selected_skills){
        messages.push(json!({"role":"system","content":reference}));
        state.emit(&id,"context_selected",json!({
            "memoryCount":selected_memories.len(),
            "skillNames":selected_skills.iter()
                .filter_map(|skill|skill["name"].as_str()).collect::<Vec<_>>()
        }));
    }
    // Context selection, not an arbitrary 12-message SQLite cutoff, now
    // determines what enters the next model request.
    messages.extend(state.store.history(&session, 256));
    messages.push(json!({"role":"user","content":prompt}));
    state.store.add_message(&session,"user",&prompt);
    state.emit(&id,"running",json!({"sessionId":session,"model":state.model_name}));
    let mut verified_writes = 0_u64;
    let mut proof=verification::Ledger::default();
    let mut total_calls = 0_u64;
    let mut receipts: Vec<Value> = Vec::new();
    let mut active_instruction = prompt.clone();
    let mut owner_redirected = false;
    let planning = strategy::PlanningPolicy::from_setting(
        input["planningMode"].as_str().unwrap_or("auto")
    );
    let model_tier=input["modelTier"].as_str().unwrap_or("auto");

    // Planning is selective: simple questions avoid the extra paid turn.
    // Its output is guidance; user instructions and tool receipts stay primary.
    if planning.should_plan(&prompt) && !state.store.cancelled(&id) {
        match state.provider.complete(&strategy::make_plan_messages(&prompt),&[],true).await {
            Ok(planning_reply) => {
                let tokens = planning_reply.input_tokens + planning_reply.output_tokens;
                state.store.account_aux_tokens(&id,tokens);
                if let Err(error)=state.store.account_cost(&id,planning_reply.cost_krw) {
                    settle(&state,&id,"needs_review","",0,
                        Some(format!("계획 비용 기록 오류: {error}")));
                    return;
                }
                state.emit(&id,"model_usage",json!({"phase":"planning",
                    "model":planning_reply.model,"inputTokens":planning_reply.input_tokens,
                    "outputTokens":planning_reply.output_tokens,
                    "cachedTokens":planning_reply.cached_tokens,"usageEstimated":planning_reply.usage_estimated,
                     "estimatedKrw":planning_reply.cost_krw}));
                if let Some(plan) = strategy::extract_plan(&planning_reply.raw) {
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
        let consumed = if allow_subagents {
            state.store.family_token_usage(&id)
        } else {
            state.store.get(&id)
                .and_then(|v|v["tokens"].as_u64()).unwrap_or(0)
        };
        if consumed >= token_budget {
            settle(&state,&id,"needs_review","",verified_writes,
                Some("전체 토큰 예산을 소진했습니다.".into()));
            return;
        }
        if let Some(ceiling)=cost_budget_krw {
            let spent=if allow_subagents {state.store.family_cost_krw(&id)}
                else {state.store.get(&id).and_then(|v|v["costKrw"].as_f64())
                    .unwrap_or(f64::INFINITY)};
            if spent>=ceiling {
                state.emit(&id,"budget_exhausted",json!({
                    "type":"krw","spentKrw":spent,"limitKrw":ceiling
                }));
                settle(&state,&id,"needs_review","",verified_writes,
                    Some("요청한 원화 사용 예산에 도달했습니다.".into()));
                return;
            }
        }
        // Full source history never changes. Budget both the tool declarations
        // AND the message JSON before the read-only provider request.
        let context_limit=std::env::var("DOMINION_CONTEXT_MAX_BYTES")
            .or_else(|_|std::env::var("DOMINION_CONTEXT_MAX_CHARS")).ok()
            .and_then(|value|value.parse::<usize>().ok())
            .unwrap_or(96_000).clamp(8192,2_000_000);
        // A model may propose which old statements matter; every selected
        // constraint must match a real owner message in SQLite. Avoid an
        // extra paid summarizer request until the active context has pressure.
        let raw_bytes=serde_json::to_vec(&messages).map(|v|v.len()).unwrap_or(usize::MAX);
        let needs_refresh=step==0 && (
            raw_bytes>context_limit.saturating_mul(3)/5 ||
            prompt.starts_with("Continue a previously persisted native task")
        );
        if needs_refresh && !state.store.cancelled(&id) {
            let allow_model=consumed.saturating_add(3500)<token_budget &&
                cost_budget_krw.is_none_or(|cap|
                    state.store.get(&id).and_then(|v|v["costKrw"].as_f64())
                        .unwrap_or(f64::INFINITY)<cap);
            if let Err(error)=semantic::refresh(
                &state,&id,&session,&active_instruction,allow_model
            ).await {
                state.emit(&id,"semantic_checkpoint_error",json!({"message":error}));
            }
        }
        let facts=state.store.context_checkpoint(&session,&active_instruction,5500);
        let found=state.store.context_search(&session,&id,&active_instruction,8)
            .into_iter().filter(|item|item["kind"]=="owner" &&
                item["excerpt"].as_str()!=Some(active_instruction.as_str()))
            .take(3).collect::<Vec<_>>();
        // Only literal owner text is promoted to the pinned checkpoint.
        // Model replies and tool output remain ordinary untrusted evidence.
        let mut outbound=messages.clone();
        if let Some(note)=semantic::checkpoint_text(&facts,&found) {
            let prefix=outbound.iter().take_while(|item|
                matches!(item["role"].as_str(),Some("system"|"developer"))).count();
            outbound.insert(prefix,json!({"role":"system","content":note}));
            state.emit(&id,"context_checkpoint_selected",json!({
                "exactOwnerFacts":facts["facts"].as_array().map(Vec::len).unwrap_or(0),
                "retrievedOwnerSources":found.len()
            }));
        }
        let declarations=serde_json::to_vec(&available).map(|v|v.len())
            .unwrap_or(usize::MAX);
        let Some(mut message_budget)=context_limit.checked_sub(declarations.saturating_add(2048))
            .filter(|remaining|*remaining>=2048) else {
            settle(&state,&id,"needs_review","",verified_writes,
                Some("도구 목록과 필수 프로토콜 크기가 모델 문맥 예산을 초과했습니다.".into()));
            return;
        };
        let economical=strategy::use_economy(model_tier,&active_instruction,
            allow_write,require_evidence);
        let durable=state.store.tool_receipt_ids(&id);
        let mut attempt=0usize;
        let mut previous_projection:Option<String>=None;
        // The provider request has no side effects from model-proposed tools.
        // A canonical context overflow may therefore retry with a *smaller*
        // read-only projection; no tool effect is ever replayed here.
        let reply=loop {
            let projected=match context::project(&outbound,message_budget,&durable){
                Ok(view)=>view,
                Err(error)=>{
                    state.emit(&id,"context_rejected",json!({
                        "message":error,"byteBudget":message_budget
                    }));
                    settle(&state,&id,"needs_review","",verified_writes,
                        Some(format!("문맥 보존 검증 실패: {error}")));
                    return;
                }
            };
            // Never resend identical context after a window-overflow error.
            // Only an actually smaller provider input justifies a paid retry.
            let projection_hash=projected.projection_sha256.clone();
            if previous_projection.as_deref()==Some(projection_hash.as_str()) {
                settle(&state,&id,"needs_review","",verified_writes,
                    Some("모델 문맥 초과 후 입력을 더 줄일 수 없어 재호출을 중단했습니다.".into()));
                return;
            }
            state.emit(&id,"context_projected",json!({
                "sourceSha256":projected.source_sha256,
                "projectionSha256":projected.projection_sha256,
                "originalBytes":projected.original_bytes,
                "modelBytes":projected.projected_bytes,
                "budgetBytes":message_budget,
                "schemaBytes":declarations,
                "omittedMessages":projected.omitted_messages,
                "clippedReceipts":projected.clipped_receipts,
                "retainedOwnerMessages":projected.retained_owner_messages,
                "recoverableCallIds":projected.recoverable_call_ids,
                "pressureRetry":attempt
            }));
            state.emit(&id,"model_step",json!({
                "step":step+1,"limit":steps,
                "modelContextBytes":projected.projected_bytes,
                "routing":if economical {"economy"}else{"quality"},
                "pressureRetry":attempt
            }));
            match state.provider.complete(&projected.messages,&available,economical).await{
                Ok(value)=>break value,
                Err(error)=>{
                    let overflow=context::is_context_overflow(&error);
                    if !overflow || attempt>=2 || message_budget<=4096 ||
                        state.store.cancelled(&id) {
                        settle(&state,&id,"failed","",verified_writes,Some(error));
                        return;
                    }
                    let smaller=(message_budget*2/3).max(4096);
                    if smaller>=message_budget {
                        settle(&state,&id,"needs_review","",verified_writes,
                            Some("Context pressure did not permit a smaller safe request".into()));
                        return;
                    }
                    state.emit(&id,"context_pressure_retry",json!({
                        "previousBudgetBytes":message_budget,
                        "newBudgetBytes":smaller,
                        "attempt":attempt+1,
                        "reason":"provider_rejected_context_window",
                        "billingNote":"Provider may charge for failed attempts; usage not reported"
                    }));
                    previous_projection=Some(projection_hash);
                    message_budget=smaller;
                    attempt+=1;
                }
            }
        };
        let tokens = reply.raw["usage"]["total_tokens"].as_u64()
            .unwrap_or(reply.input_tokens.saturating_add(reply.output_tokens));
        state.store.measure(&id,tokens);
        if let Err(error)=state.store.account_cost(&id,reply.cost_krw) {
            settle(&state,&id,"needs_review","",verified_writes,
                Some(format!("모델 사용 비용을 저장하지 못했습니다: {error}")));
            return;
        }
        state.emit(&id,"model_usage",json!({"phase":"execute",
            "model":reply.model,"inputTokens":reply.input_tokens,
            "outputTokens":reply.output_tokens,
            "cachedTokens":reply.cached_tokens,"usageEstimated":reply.usage_estimated,
             "estimatedKrw":reply.cost_krw}));
        let response = reply.raw;
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
        // A model's tool-call arguments are historical evidence too.
        // Commit the entire assistant reply BEFORE any proposed side effect.
        // The ledger can then reconstruct an interrupted turn exactly.
        if let Err(error)=state.record_event(&id,"model_reply",
            json!({"step":step+1,"message":message})) {
            settle(&state,&id,"needs_review","",verified_writes,
                Some(format!("모델 실행 기록 저장 실패: {error}")));
            return;
        }
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
            // A compiler/test exit code is independent of a model's claim.
            // Never accept an old green check after a newer edit or any
            // uncertain source modification. Feed real diagnostics to the
            // same Rust model loop for bounded autonomous repair.
            if verified_writes>0 && allow_shell &&
                strategy::source_change_requested(&active_instruction) {
                if strict_checks && expected_checks.is_empty() {
                    settle(&state,&id,"needs_review",&text,verified_writes,
                        Some("No runnable acceptance test was found. Supply an explicit acceptanceChecks command.".into()));
                    return;
                }
                if !expected_checks.is_empty() {
                    match verification::audit(
                        &state,&id,&session,&expected_checks,&mut proof,&mut receipts
                    ).await {
                        Ok(Some((check,failure)))=>{
                            let feedback=verification::repair_feedback(
                                &check,&failure,proof.revision);
                            state.emit(&id,"repair_required",json!({
                                "failedCheck":check.label(),"revision":proof.revision,
                                "nextStep":step+2,"stepsRemaining":steps-(step+1)
                            }));
                            if step+1<steps {
                                messages.push(message);
                                messages.push(json!({"role":"user","content":feedback}));
                                continue;
                            }
                            settle(&state,&id,"needs_review",&text,verified_writes,
                                Some(feedback));
                            return;
                        },
                        Ok(None)=>{
                            if !proof.complete(&expected_checks){
                                settle(&state,&id,"needs_review",&text,verified_writes,
                                    Some("Source acceptance did not produce complete fresh proof".into()));
                                return;
                            }
                            state.emit(&id,"acceptance_complete",json!({
                                "checks":expected_checks.iter().map(verification::Check::label)
                                    .collect::<Vec<_>>(),
                                "revision":proof.revision,"passed":true
                            }));
                        },
                        Err(error)=>{
                            settle(&state,&id,"needs_review",&text,verified_writes,
                                Some(format!("Source acceptance could not be saved: {error}")));
                            return;
                        }
                    }
                }
            }
            // For complex work a separate model critique may demand further
            // corrections. It cannot override the native evidence check.
            if strategy::complex(&active_instruction) && strategy::action_requested(&active_instruction)
                && !receipts.is_empty() {
                let review = state.provider.complete(
                    &strategy::review_messages(&active_instruction,&text,&receipts),&[],false).await;
                match review {
                    Ok(verdict) => {
                        let tokens=verdict.input_tokens + verdict.output_tokens;
                        state.store.account_aux_tokens(&id,tokens);
                         if let Err(error)=state.store.account_cost(&id,verdict.cost_krw) {
                             settle(&state,&id,"needs_review",&text,verified_writes,
                                 Some(format!("검증 비용 기록 오류: {error}")));
                             return;
                         }
                        state.emit(&id,"model_usage",json!({"phase":"verification",
                            "model":verdict.model,"inputTokens":verdict.input_tokens,
                            "outputTokens":verdict.output_tokens,
                            "cachedTokens":verdict.cached_tokens,"usageEstimated":verdict.usage_estimated,
                             "estimatedKrw":verdict.cost_krw}));
                        let review=strategy::parse_review(&verdict.raw);
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
            let outcome = if tool_name=="subagent_spawn" && allow_subagents {
                let instruction=arguments["task"].as_str().unwrap_or("").trim();
                let role=arguments["role"].as_str().unwrap_or("Independent research assistant").trim();
                if instruction.is_empty() || instruction.len()>8000 || role.len()>200 {
                    Err("Invalid delegated task length".into())
                } else {
                    let key_digest=Sha256::digest(format!("{id}/{tool_id}").as_bytes());
                    let key=format!("sub-{:x}",key_digest);
                    let child=crate::runtime::TaskInput{
                        prompt:format!("Read-only independent analysis. Role: {role}\\nTask: {instruction}"),
                        request_id:Some(key),max_steps:Some(6),token_budget:Some(2000),
                        allow_write:false,allow_shell:false,allow_mcp:false,allow_subagents:false,
                        require_evidence:false,planning_mode:Some("auto".into()),
                        ..Default::default()
                    };
                    let sdk=crate::runtime::DominionSdk::from_state(state.clone());
                    sdk.delegate(&id,child).await.map(|run|json!({
                        "taskId":run.id,"status":run.status,"sessionId":run.input["sessionId"]
                    })).map_err(|e|e.to_string())
                }
            } else if tool_name=="subagent_status" && allow_subagents {
                let task_id=arguments["taskId"].as_str().unwrap_or("");
                let child=state.store.child_tasks(&id)
                    .into_iter().find(|record|record["id"].as_str()==Some(task_id));
                child.map(|record|json!({
                    "taskId":record["id"],"status":record["status"],
                    "result":record["result"],"error":record["error"],
                    "tokens":record["tokens"]
                })).ok_or_else(||"Child task not found in this parent execution".into())
            } else if tool_name.starts_with("mcp__") && allow_mcp {
                state.mcp.call(&crate::mcp::ToolCall {
                    id:tool_id.clone(),name:tool_name.into(),arguments:arguments.clone()
                }).await.map(|response|json!({"content":response}))
            } else {
                tool(&state,&id,&session,tool_name,&arguments,allow_write,allow_shell).await
            };
            if matches!(tool_name, "workspace_write" | "workspace_replace") && outcome.as_ref()
                .is_ok_and(|v|v["verified"]==true) {
                verified_writes += 1;
            }
            let value = match outcome {
                Ok(receipt) => json!({
                    "ok":tool_name!="workspace_check" || receipt["success"]==true,
                    "receipt":receipt
                }),
                Err(error) => json!({"ok":false,"error":error}),
            };
            if let Err(error)=state.record_event(&id,"tool_result",
                json!({"name":tool_name,"callId":tool_id,"outcome":value})) {
                settle(&state,&id,"needs_review","",verified_writes,
                    Some(format!("도구 결과가 기록되지 않았습니다. 재실행하지 마세요: {error}")));
                return;
            }
            proof.observe(tool_name,&arguments,&value);
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

    async fn mock_state(url:String) -> (AppState,std::path::PathBuf) {
        let root=std::env::temp_dir().join(
            format!("dominion-runtime-test-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store=crate::store::Store::open(&root.join("state.sqlite")).unwrap();
        let (bus,_)=tokio::sync::broadcast::channel(128);
        (AppState {store,bus,
            mcp:std::sync::Arc::new(crate::mcp::Mcp::new(vec![]).unwrap()),
            groups:std::sync::Arc::new(crate::group::Groups::open(root.join("groups")).await.unwrap()),
            videos:std::sync::Arc::new(crate::video::Videos::open(root.join("videos")).await.unwrap()),
            improvements:std::sync::Arc::new(crate::improvement::Improvements::open(
                root.join("improvements"),root.clone()).await.unwrap()),
            provider:crate::provider::ModelClient::new(
                crate::provider::ModelConfig::basic(url.clone(),"mock".into(),None),None
            ).unwrap(),
            workspace:root.clone(),workspace_write_lock:std::sync::Arc::new(tokio::sync::Mutex::new(())),
            model_name:"mock".into()},root)
    }

    #[tokio::test]
    async fn mock_model_writes_file_and_rust_verifies_real_bytes() {
        let (url,_)=mock_provider(false).await;
        let (state,root)=mock_state(url).await;
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
    async fn context_overflow_does_not_repeat_an_identical_paid_request(){
        use axum::{http::StatusCode,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let counter=Arc::new(AtomicUsize::new(0));
        let calls=counter.clone();
        let app=Router::new().route("/v1/chat/completions",post(
            move ||{
                let calls=calls.clone();
                async move {
                    calls.fetch_add(1,Ordering::SeqCst);
                    (StatusCode::BAD_REQUEST,Json(json!({
                        "error":{"code":"context_length_exceeded"}
                    })))
                }
            }
        ));
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let (state,root)=mock_state(url).await;
        let (task,_)=state.store.submit("small-overflow-2026",
            &json!({"prompt":"Say hello","planningMode":"never",
                "maxSteps":3,"tokenBudget":2000})).unwrap();
        let id=task["id"].as_str().unwrap().to_owned();
        run(state.clone(),id.clone()).await;
        let record=state.store.get(&id).unwrap();
        assert_eq!(record["status"],"needs_review");
        assert_eq!(counter.load(Ordering::SeqCst),1,
            "An unchanged projected request must not be sent twice");
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn provider_context_pressure_retries_only_after_real_projection_reduction(){
        use axum::{http::StatusCode,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let counter=Arc::new(AtomicUsize::new(0));
        let calls=counter.clone();
        let app=Router::new().route("/v1/chat/completions",post(
            move |Json(body):Json<Value>|{
                let calls=calls.clone();
                async move{
                    // Semantic selection is an independent bounded model phase;
                    // its call must not consume the execution retry counter.
                    if body["messages"][0]["content"].as_str()
                        .is_some_and(|text|text.contains("Extract important ORIGINAL OWNER")) {
                        return (StatusCode::OK,Json(json!({
                            "choices":[{"message":{"role":"assistant",
                                "content":"{\"items\":[]}"}}],
                            "usage":{"prompt_tokens":100,"completion_tokens":15}
                        })));
                    }
                    let call=calls.fetch_add(1,Ordering::SeqCst);
                    if call==0 {
                        (StatusCode::BAD_REQUEST,Json(json!({
                            "error":{"code":"context_length_exceeded"}
                        })))
                    }else{
                        (StatusCode::OK,Json(json!({
                            "choices":[{"message":{"role":"assistant",
                                "content":"Compressed context accepted."}}],
                            "usage":{"prompt_tokens":100,"completion_tokens":20}
                        })))
                    }
                }
            }
        ));
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let (state,root)=mock_state(url).await;
        for n in 0..15 {
            state.store.add_message("long-history","assistant",
                &format!("{n}:{}", "context".repeat(2400)));
        }
        let (task,_)=state.store.submit("pressure-success-2026",
            &json!({"prompt":"Summarize earlier facts","sessionId":"long-history",
                "planningMode":"never","maxSteps":2,"tokenBudget":10000})).unwrap();
        let id=task["id"].as_str().unwrap().to_owned();
        run(state.clone(),id.clone()).await;
        let record=state.store.get(&id).unwrap();
        assert_eq!(record["status"],"completed","provider result: {record}");
        assert_eq!(counter.load(Ordering::SeqCst),2);
        let events=state.store.events_after(&id,0,200);
        let projected=events.iter().filter(|event|event["type"]=="context_projected")
            .collect::<Vec<_>>();
        assert_eq!(projected.len(),2);
        assert!(projected[1]["detail"]["modelBytes"].as_u64().unwrap()
            <projected[0]["detail"]["modelBytes"].as_u64().unwrap());
        assert!(events.iter().any(|event|event["type"]=="context_pressure_retry"));
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resumed_task_can_read_prior_receipt_but_not_an_unrelated_task(){
        let (url,_)=mock_provider(false).await;
        let (state,root)=mock_state(url).await;
        let (prior,_)=state.store.submit("prior-receipt-2026",
            &json!({"prompt":"read","sessionId":null})).unwrap();
        let old_id=prior["id"].as_str().unwrap().to_string();
        state.store.append_event(&old_id,"tool_result",
            json!({"callId":"previous-proof","outcome":{
                "receipt":{"content":"persisted evidence"}}})).unwrap();
        let (next,_)=state.store.submit_continuation("new-receipt-2026",
            &json!({"prompt":"continue","sessionId":old_id}),&old_id,"completed").unwrap();
        let new_id=next["id"].as_str().unwrap();
        let page=tool(&state,new_id,&old_id,"task_receipt",
            &json!({"sourceTaskId":old_id,"callId":"previous-proof",
                "offsetBytes":0,"maxBytes":4096}),false,false).await.unwrap();
        assert!(page["page"].as_str().unwrap().contains("persisted evidence"));
        let (foreign,_)=state.store.submit("foreign-receipt-2026",
            &json!({"prompt":"other","sessionId":old_id})).unwrap();
        let outside=foreign["id"].as_str().unwrap();
        state.store.append_event(outside,"tool_result",
            json!({"callId":"previous-proof","outcome":{"secret":"foreign"}})).unwrap();
        let denied=tool(&state,new_id,&old_id,"task_receipt",
            &json!({"sourceTaskId":outside,"callId":"previous-proof"}),
            false,false).await;
        assert!(denied.is_err());
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_searches_unknown_prior_call_id_then_recovers_verified_ancestor(){
        use axum::{routing::post,Json,Router};
        let app=Router::new().route("/v1/chat/completions",post(
            move |Json(body):Json<Value>|async move{
                let messages=body["messages"].as_array().cloned().unwrap_or_default();
                let tool_result=messages.iter().rev().find(|item|item["role"]=="tool")
                    .and_then(|item|item["content"].as_str())
                    .and_then(|content|serde_json::from_str::<Value>(content).ok());
                let answer=if let Some(result)=tool_result{
                    let receipt=&result["receipt"];
                    if receipt["page"].as_str().is_some_and(|text|
                        text.contains("CobaltThistleMarker")) {
                        json!({"role":"assistant","content":"Recovered exact archival proof."})
                    }else{
                        let indexed=receipt["results"].as_array().unwrap().iter()
                            .find(|item|item["kind"]=="tool").unwrap();
                        json!({"role":"assistant","content":null,"tool_calls":[{
                            "id":"reinspect-record","type":"function","function":{
                                "name":"task_receipt","arguments":json!({
                                    "sourceTaskId":indexed["sourceTaskId"],
                                    "callId":indexed["callId"],
                                    "offsetBytes":0,"maxBytes":4096
                                }).to_string()
                            }
                        }]})
                    }
                }else{
                    json!({"role":"assistant","content":null,"tool_calls":[{
                        "id":"find-lost-receipt","type":"function","function":{
                            "name":"context_search","arguments":json!({
                                "query":"CobaltThistleMarker","limit":4
                            }).to_string()
                        }
                    }]})
                };
                Json(json!({"choices":[{"message":answer}],
                    "usage":{"prompt_tokens":130,"completion_tokens":50}}))
            }
        ));
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let (state,root)=mock_state(url).await;
        let (old,_)=state.store.submit("ancestry-first-2026",
            &json!({"prompt":"inspect","sessionId":"repo-review"})).unwrap();
        let prior=old["id"].as_str().unwrap().to_owned();
        assert!(state.store.claim(&prior));
        state.store.append_event(&prior,"tool_result",json!({
            "callId":"unknown-until-found","name":"workspace_read",
            "outcome":{"ok":true,"receipt":{
                "path":"archived.txt","content":"CobaltThistleMarker audited evidence from old run"
            }}
        })).unwrap();
        state.store.finish(&prior,"completed",json!({"answer":"done"}),None).unwrap();
        let (current,_)=state.store.submit_continuation("ancestry-second-2026",
            &json!({"prompt":"Find archived CobaltThistleMarker evidence, cite the exact saved result.",
                "sessionId":"repo-review","planningMode":"never",
                "maxSteps":4,"tokenBudget":10000}),&prior,"completed").unwrap();
        let id=current["id"].as_str().unwrap().to_owned();
        run(state.clone(),id.clone()).await;
        let record=state.store.get(&id).unwrap();
        assert_eq!(record["status"],"completed","native result: {record}");
        assert_eq!(record["result"]["answer"],"Recovered exact archival proof.");
        let trace=state.store.events_after(&id,0,100);
        assert!(trace.iter().any(|entry|entry["type"]=="tool_result"
            && entry["detail"]["name"]=="context_search"));
        assert!(trace.iter().any(|entry|entry["type"]=="tool_result"
            && entry["detail"]["name"]=="task_receipt"));
        assert!(!trace.iter().any(|entry|entry["type"]=="tool_start"
            && entry["detail"]["name"]=="workspace_read"));
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn early_owner_constraint_survives_far_beyond_recent_session_window(){
        use axum::{routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let summary_calls=Arc::new(AtomicUsize::new(0));
        let counter=summary_calls.clone();
        let app=Router::new().route("/v1/chat/completions",post(
            move |Json(body):Json<Value>|{
                let counter=counter.clone();
                async move{
                    let messages=body["messages"].as_array().cloned().unwrap_or_default();
                    if messages.first().and_then(|v|v["content"].as_str())
                        .is_some_and(|v|v.contains("Extract important ORIGINAL OWNER")){
                        counter.fetch_add(1,Ordering::SeqCst);
                        let packed:Value=serde_json::from_str(
                            messages[1]["content"].as_str().unwrap()).unwrap();
                        let source=packed["sources"].as_array().unwrap().iter()
                            .find(|row|row["content"].as_str().unwrap_or("")
                                .contains("반드시 구리색 버튼을 유지하세요")).unwrap();
                        let facts=json!({"items":[{"seq":source["seq"],
                            "kind":"constraint","topic":"button colors",
                            "quote":"반드시 구리색 버튼을 유지하세요"}]}).to_string();
                        return Json(json!({"choices":[{"message":{
                            "role":"assistant","content":facts}}],
                            "usage":{"prompt_tokens":110,"completion_tokens":80}}));
                    }
                    let retained=messages.iter().any(|message|
                        message["role"]=="system" &&
                        message["content"].as_str().unwrap_or("")
                            .contains("반드시 구리색 버튼을 유지하세요"));
                    Json(json!({"choices":[{"message":{
                        "role":"assistant","content":if retained{
                            "Original owner restriction retained."
                        }else{"Owner restriction missing."}
                    }}],"usage":{"prompt_tokens":300,"completion_tokens":45}}))
                }
            }
        ));
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let (state,root)=mock_state(url).await;
        state.store.add_message("long-session","user",
            "반드시 구리색 버튼을 유지하세요. 비용이 많이 드는 변경은 하지 마세요.");
        for index in 0..310 {
            state.store.add_message("long-session","assistant",
                &format!("Older response {index}: {}", "context archive ".repeat(35)));
        }
        assert!(state.store.history("long-session",256).iter().all(|v|
            !v["content"].as_str().unwrap_or("").contains("구리색")));
        let (task,_)=state.store.submit("constraint-older-than-256-2026",
            &json!({"prompt":"Report whether the original design rule was retained",
                "sessionId":"long-session","planningMode":"never",
                "modelTier":"quality","maxSteps":2,"tokenBudget":15000})).unwrap();
        let id=task["id"].as_str().unwrap().to_owned();
        run(state.clone(),id.clone()).await;
        let result=state.store.get(&id).unwrap();
        assert_eq!(result["status"],"completed","record: {result}");
        assert_eq!(result["result"]["answer"],"Original owner restriction retained.");
        assert_eq!(summary_calls.load(Ordering::SeqCst),1);
        let checkpoint=state.store.context_checkpoint("long-session","design rule",4096);
        assert!(checkpoint.to_string().contains("반드시 구리색 버튼을 유지하세요"));
        assert!(state.store.events_after(&id,0,200).iter()
            .any(|item|item["type"]=="semantic_checkpoint"));
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn full_native_loop_compacts_large_tools_and_recovers_exact_persisted_page(){
        use axum::{routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let calls=Arc::new(AtomicUsize::new(0));
        let observed=calls.clone();
        let app=Router::new().route("/v1/chat/completions",post(
            move |Json(body):Json<Value>|{
                let observed=observed.clone();
                async move{
                    observed.fetch_add(1,Ordering::SeqCst);
                    let messages=body["messages"].as_array().cloned().unwrap_or_default();
                    let got_page=messages.iter().any(|item|
                        item["role"]=="tool" && item["content"].as_str().is_some_and(
                            |text|text.contains("nextOffsetBytes")&&text.contains("sha256")));
                    let is_clipped=messages.iter().any(|item|
                        item["role"]=="tool" && item["content"].as_str().is_some_and(
                            |text|text.contains("[DOMINION persisted tool result;")));
                    let assistant=if got_page{
                        json!({"role":"assistant","content":"Recovered exact persisted evidence page."})
                    }else if is_clipped{
                        json!({"role":"assistant","content":null,"tool_calls":[{
                            "id":"retrieve-one","type":"function","function":{
                                "name":"task_receipt","arguments":json!({
                                    "callId":"first-read","offsetBytes":0,
                                    "maxBytes":4096
                                }).to_string()
                            }
                        }]})
                    }else{
                        json!({"role":"assistant","content":null,"tool_calls":[
                            {"id":"first-read","type":"function","function":{
                                "name":"workspace_read",
                                "arguments":json!({"path":"large.txt"}).to_string()
                            }},
                            {"id":"second-read","type":"function","function":{
                                "name":"workspace_read",
                                "arguments":json!({"path":"large.txt"}).to_string()
                            }}
                        ]})
                    };
                    Json(json!({"choices":[{"message":assistant}],
                        "usage":{"prompt_tokens":180,"completion_tokens":60}}))
                }
            }
        ));
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let (state,root)=mock_state(url).await;
        std::fs::write(root.join("large.txt"),"가".repeat(19_000)).unwrap();
        let input=json!({"prompt":"Inspect large.txt and recover archived evidence",
            "sessionId":null,"allowWrite":false,"allowShell":false,
            "requireEvidence":false,"maxSteps":5,"tokenBudget":20000,
            "planningMode":"never","modelTier":"quality"});
        let (task,_)=state.store.submit("bounded-context-2026",&input).unwrap();
        let id=task["id"].as_str().unwrap().to_string();
        run(state.clone(),id.clone()).await;
        let saved=state.store.get(&id).unwrap();
        assert_eq!(saved["status"],"completed","native failure: {}",saved);
        assert_eq!(saved["result"]["answer"],"Recovered exact persisted evidence page.");
        assert_eq!(calls.load(Ordering::SeqCst),3);
        let events=state.store.events_after(&id,0,100);
        assert!(events.iter().any(|e|e["type"]=="context_projected"
            && e["detail"]["clippedReceipts"].as_u64().unwrap_or(0)>0));
        assert!(events.iter().any(|e|e["type"]=="model_reply"));
        assert!(events.iter().any(|e|e["type"]=="tool_result"
            && e["detail"]["callId"]=="retrieve-one"
            && e["detail"]["outcome"]["receipt"]["sha256"].is_string()));
        let original=state.store.tool_receipt(&id,"first-read").unwrap();
        assert!(original["detail"]["outcome"]["receipt"]["content"]
            .as_str().unwrap().len()>55_000);
        assert_ne!(original["detail"]["outcome"]["receipt"]["content"],Value::Null);
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mid_request_owner_instruction_prevents_unstarted_file_write() {
        let (url,arrival)=mock_provider(true).await;
        let (state,root)=mock_state(url).await;
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
