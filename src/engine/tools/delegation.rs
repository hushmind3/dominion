//! Task-scoped delegation and owned child evidence retrieval.
//! Reuses the one native execution driver, budget tree, approvals and journal.
use super::*;

const MAX_DELEGATION_DEPTH: usize = 4;

/// Number of delegated sessions between the owner-facing session and this one.
/// A chain cannot loop forever, so a cycle is cut defensively.
pub(super) fn delegation_depth(store: &Store, session_id: &str) -> usize {
    let mut depth = 0;
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(session_id.to_string());
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(session) = store.get("session", &id) else {
            break;
        };
        if session["delegated"].as_bool() == Some(true) {
            depth += 1;
        }
        current = session["parentSessionId"].as_str().map(str::to_string);
    }
    depth
}

pub(super) async fn delegate(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let agent_id = required_arg(args, "agentId")?;
    // A delegated worker cannot delegate beyond a bounded depth. This keeps a
    // runaway chain from consuming unbounded workers.
    if delegation_depth(ctx.store, ctx.session_id) >= MAX_DELEGATION_DEPTH {
        return Err(format!(
            "위임 깊이 제한({MAX_DELEGATION_DEPTH})에 도달했습니다. 상위 작업으로 결과를 모아 처리하세요."
        ));
    }
    let prompt = required_arg(args, "prompt")?;
    let profile = args["profile"].as_str().unwrap_or("general");
    if !matches!(profile, "general" | "inspection" | "implementation" | "verification") {
        return Err("지원하지 않는 위임 프로필입니다.".into());
    }
    // An explicit task-specific role cannot inherit the unrestricted parent
    // filesystem. Require a separate, preflight-verified worktree and sandbox
    // before reserving budget or creating the child job.
    if profile != "general" && args["isolatedWorktree"] != true {
        return Err("역할별 위임은 격리 Git 워크트리(isolatedWorktree=true)가 필요합니다.".into());
    }
    let agent = ctx
        .state
        .store
        .get("agent", &agent_id)
        .ok_or_else(|| format!("위임 대상 에이전트를 찾을 수 없습니다: {agent_id}"))?;
    let wait = args["wait"].as_bool().unwrap_or(true);
    let timeout = args["timeoutSeconds"]
        .as_u64()
        .unwrap_or(90)
        .clamp(5, 300);

    let session_id=uuid::Uuid::new_v4().to_string();
    let job_id=uuid::Uuid::new_v4().to_string();
    let mut session=json!({"id":session_id,"workspaceDir":ctx.workspace.canonicalize().map_err(|e|e.to_string())?,"title":format!("Delegate · {}",prompt.chars().take(36).collect::<String>()),
        "agentId":agent_id,"providerId":agent["providerId"],"model":agent["model"],
        "parentSessionId":ctx.session_id,"delegated":true,"delegationProfile":profile,
        "readOnly":args["readOnly"]==true || profile=="inspection"});
    if crate::engine::sandbox::required(ctx.state,ctx.session_id){session["shellSandbox"]=json!("required");}
    if args["isolatedWorktree"]==true {
        session["delegationWorkspace"]=crate::engine::delegation_worktree::prepare(ctx.state,ctx.workspace,&job_id).await?;
        session["shellSandbox"]=json!("required");
    }
    let job=json!({"id":job_id,"kind":"prompt","status":"queued","queuedAt":chrono::Utc::now().to_rfc3339(),
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[],"input":{
            "prompt":prompt,"agentId":agent_id,"providerId":agent["providerId"],"model":agent["model"],
            "sessionId":session_id,"toolsEnabled":ctx.tools_enabled&&ctx.tools_execution_enabled,
            "toolsExecutionEnabled":ctx.tools_execution_enabled,"parentSessionId":ctx.session_id,
            "parentAgentId":ctx.agent_id,"delegation":true,"delegationProfile":profile}});
    let prepared=session["delegationWorkspace"].clone();
    if let Err(error)=crate::engine::execution_budget::enqueue_child(ctx.state,ctx.session_id,session,job,args["budget"].as_u64()){
        if prepared.is_object(){let _=crate::engine::delegation_worktree::discard_prepared(&prepared).await;}
        return Err(error);
    }
    // Subscribe before scheduling: a very fast child can finish before the
    // parent starts waiting. Durable SQLite state remains the final authority.
    let mut events = ctx.state.bus.subscribe();
    crate::engine::jobs::start_now(ctx.state.clone(), job_id.clone());

    if !wait {
        return Ok(json!({
            "jobId": job_id, "status": "queued", "sessionId": session_id,
            "hint": "task_status 또는 task_result로 확인하세요."
        }));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        if ctx.state.is_cancelled(ctx.session_id) {
            ctx.state.cancel_session(&session_id);
            return Err("부모 실행과 위임 작업이 함께 중단되었습니다.".into());
        }
        let Some(current) = ctx.state.store.get("job", &job_id) else {
            return Err("위임 작업 기록이 사라졌습니다.".into());
        };
        let status = current["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed" | "cancelled" | "needs_review") {
            return task_payload(ctx, &json!({"jobId": job_id}), true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(json!({
                "jobId": job_id, "status": status, "sessionId": session_id,
                "hint": "아직 실행 중입니다. task_result로 나중에 확인하세요."
            }));
        }
        // Wait for this child's durable job state instead of polling SQLite
        // four times a second. The bounded fallback covers lag/restarts and
        // parent cancellation when no child event is emitted.
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        wait_for_child_signal(&mut events, &job_id,
            remaining.min(std::time::Duration::from_secs(3))).await;
    }
}

async fn wait_for_child_signal(
    events: &mut tokio::sync::broadcast::Receiver<Value>,
    job_id: &str,
    max_wait: std::time::Duration,
) {
    let deadline = tokio::time::Instant::now() + max_wait;
    loop {
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => break,
            event = events.recv() => match event {
                Ok(event) if event["type"]=="record"
                    && event["kind"]=="job"
                    && event["record"]["id"].as_str()==Some(job_id) => break,
                Ok(_) => {}, // unrelated room, tool and job events don't force DB work
                Err(_) => break, // lagged/closed => verify the durable job row
            }
        }
    }
}

pub(super) fn task_payload(ctx: &ToolContext<'_>, args: &Value, with_result: bool) -> Result<Value, String> {
    let job_id = required_arg(args, "jobId")?;
    let job = ctx
        .state
        .store
        .get("job", &job_id)
        .ok_or_else(|| format!("작업을 찾을 수 없습니다: {job_id}"))?;
    // A job result is visible only to the session that created it or to the
    // parent session that delegated it.
    let owned = job["input"]["sessionId"].as_str() == Some(ctx.session_id)
        || job["input"]["parentSessionId"].as_str() == Some(ctx.session_id);
    if !owned {
        return Err("이 작업에 접근할 권한이 없습니다.".into());
    }
    let status = job["status"].as_str().unwrap_or("unknown");
    let mut payload = json!({
        "jobId": job_id,
        "kind": job["kind"],
        "status": status,
        "stepIndex": job["stepIndex"],
        "stepTotal": job["stepTotal"],
        "error": job["error"],
        "result": job["result"],
        "workspace":job["delegationWorkspace"],
        "patchSha256":job["delegationPatchSha256"],
        "budget":job["input"]["delegationBudget"],
    });
    if !with_result || !matches!(status, "completed" | "failed" | "needs_review") {
        return Ok(payload);
    }
    let Some(session_id) = job
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .or_else(|| job["sessionId"].as_str())
    else {
        return Ok(payload);
    };
    let messages = ctx.state.store.messages(session_id);
    let answer = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .and_then(|message| message["content"].as_str())
        .unwrap_or("")
        .chars()
        .take(8000)
        .collect::<String>();
    let operations = ctx
        .store
        .recent_completed_session_operations(session_id, 20)?
        .into_iter()
        .map(|operation| {
            json!({"tool":operation["tool"],"arguments":operation["arguments"],"result":operation["result"]})
        })
        .collect::<Vec<_>>();
    payload["sessionId"] = json!(session_id);
    payload["content"] = json!(answer);
    payload["operations"] = json!(operations);
    Ok(payload)
}


/// Task-specific tool admission: the model catalog, progressive discovery,
/// and the unified executor use this same rule. Default delegation keeps the
/// existing tool set. Verification can run a narrowly specified Cargo test
/// command inside the mandatory isolated worktree and sandbox.
pub(crate) fn profile_tool_visible(session: &Value, name: &str) -> bool {
    if matches!(session["delegationProfile"].as_str(),
        Some("inspection" | "implementation" | "verification"))
        && !session["delegationWorkspace"].is_object()
    {
        return false;
    }
    match session["delegationProfile"].as_str().unwrap_or("general") {
        "inspection" => observational(name),
        "verification" => observational(name) || name == "terminal_exec",
        "implementation" | "general" => true,
        _ => false,
    }
}

pub(crate) fn profile_tool_allowed(session: &Value, name: &str, args: &Value) -> bool {
    if !profile_tool_visible(session, name) { return false; }
    if session["delegationProfile"] == "verification" && name == "terminal_exec" {
        let program = args["command"].as_str().unwrap_or("");
        // An arbitrary /workspace/cargo executable is not a trusted test
        // runner just because its basename matches the real Cargo command.
        let command_allowed = matches!(program, "cargo" | "cargo.exe");
        let arguments = args["args"].as_array();
        let subcommand = arguments.and_then(|args| args.first())
            .and_then(Value::as_str);
        return command_allowed && subcommand == Some("test");
    }
    true
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    #[tokio::test]
    async fn child_wait_observes_only_its_job_and_falls_back_after_timeout() {
        let (sender,mut receiver)=tokio::sync::broadcast::channel(16);
        let waiting=tokio::spawn(async move {
            wait_for_child_signal(&mut receiver,"child-a",
                std::time::Duration::from_secs(2)).await;
        });
        sender.send(json!({"type":"record","kind":"job",
            "record":{"id":"other-child","status":"completed"}})).unwrap();
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(),"unrelated child cannot wake parent polling");
        sender.send(json!({"type":"record","kind":"job",
            "record":{"id":"child-a","status":"completed"}})).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1),waiting)
            .await.expect("matching job should wake immediately").unwrap();
        let (_sender,mut rx)=tokio::sync::broadcast::channel(4);
        tokio::time::timeout(std::time::Duration::from_millis(100),
            wait_for_child_signal(&mut rx,"absent",
                std::time::Duration::from_millis(5))).await.unwrap();
    }

    #[test]
    fn delegated_role_tool_matrix_restricts_mutations_and_verification_commands() {
        let inspect=json!({"delegationProfile":"inspection","readOnly":true,
            "delegationWorkspace":{"path":"/tmp"}});
        let verify=json!({"delegationProfile":"verification","delegationWorkspace":{"path":"/tmp"}});
        let implement=json!({"delegationProfile":"implementation","delegationWorkspace":{"path":"/tmp"}});
        assert!(profile_tool_visible(&inspect, "workspace_read"));
        assert!(!profile_tool_visible(&inspect, "workspace_write"));
        assert!(!profile_tool_visible(&inspect, "terminal_exec"));
        assert!(profile_tool_visible(&verify, "terminal_exec"));
        assert!(!profile_tool_visible(&verify, "workspace_write"));
        assert!(!profile_tool_visible(&verify, "shell_exec"));
        assert!(profile_tool_allowed(&verify, "terminal_exec",
            &json!({"command":"cargo","args":["test","--locked"]})));
        assert!(!profile_tool_allowed(&verify, "terminal_exec",
            &json!({"command":"cargo","args":["run","--bin","unsafe"]})));
        assert!(!profile_tool_allowed(&verify, "terminal_exec",
            &json!({"command":"sh","args":["-c","touch example"]})));
        assert!(profile_tool_visible(&implement, "workspace_replace"));
        assert!(profile_tool_visible(&implement, "delegate_task"));
        assert!(!profile_tool_visible(
            &json!({"delegationProfile":"implementation"}), "workspace_write"
        ),"task role without an isolated workspace is never admitted");
        assert!(!profile_tool_allowed(&verify, "terminal_exec",
            &json!({"command":"/tmp/cargo","args":["test"]})));
        assert!(profile_tool_visible(&json!({}), "workspace_write"));
    }
}
