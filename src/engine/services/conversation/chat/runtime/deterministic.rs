//! Frozen deterministic replacements and approved tool admission.
use super::*;

/// A fully specified replacement is executable without a model only when every
/// acceptance requirement is the same deterministic operation and its expected
/// hash was frozen before the turn. Anything less falls back to the model.
pub(super) fn frozen_replacements(checks: &Value) -> Option<Vec<Value>> {
    let items = checks.as_array()?;
    (!items.is_empty()
        && items.len() <= 32
        && items.iter().all(|check| {
            check["type"] == "file_replace_exact"
                && check["path"].as_str().is_some_and(|path| !path.is_empty())
                && check["old"].as_str().is_some_and(|old| !old.is_empty())
                && check["new"].as_str().is_some()
                && check["expectedSha256"].as_str().is_some()
        }))
    .then(|| items.to_vec())
}

/// Applies the literal replacement only when the old text occurs once and the
/// result matches the hash captured in the contract. A mismatch is not a
/// failure of the task; it means the deterministic path does not apply.
pub(super) fn replacement_content(check: &Value, content: &str) -> Option<String> {
    let old = check["old"].as_str()?;
    let new = check["new"].as_str()?;
    if content.matches(old).count() != 1 {
        return None;
    }
    let replaced = content.replacen(old, new, 1);
    (format!("{:x}", Sha256::digest(replaced.as_bytes())) == check["expectedSha256"].as_str()?)
        .then_some(replaced)
}

/// Runs one deterministic tool call through the same approval, file lock,
/// effect receipt and evidence store as a model-issued call.
#[allow(clippy::too_many_arguments)]
pub(super) async fn deterministic_tool(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    execution_id: &str,
    tool: &str,
    arguments: &Value,
    op_seq: u64,
    metrics: &std::sync::Mutex<Metrics>,
) -> Result<(Value, Value), String> {
    // The zero-model optimization is an alternative decision source, NOT an
    // alternative tool executor. Use exactly the same admission, normalization,
    // approval lineage, effect receipt, audit, and error contract as model and
    // ProgramIR calls. A persisted operation is the source of truth.
    let call_index = usize::try_from(op_seq)
        .map_err(|_| "deterministic tool sequence exceeds platform range")?;
    let call = ToolCall {
        id: format!("frozen-{op_seq}-{tool}"),
        name: tool.to_string(),
        arguments: arguments.to_string(),
    };
    let operation_id = format!("{execution_id}:0:{call_index}:{}", call.id);
    let session = state.store.get("session", session_id)
        .ok_or("결정적 도구 실행의 세션을 찾을 수 없습니다.")?;
    let (result, _accepted, _content) = dispatch_tool_call(
        state,
        tx,
        &session,
        request,
        session_id,
        execution_id,
        &call,
        arguments.clone(),
        metrics,
        op_seq,
        0,
        call_index,
        false,
        "deterministic_shortcut",
        None,
        None,
        None,
    ).await?;
    let operation = state.store.get("execution_operation", &operation_id)
        .ok_or("결정적 도구 실행의 영속 영수증을 찾을 수 없습니다.")?;
    Ok((result, operation))
}

/// Records what was already applied when a later write cannot proceed. The
/// operations remain the primary evidence; this marker makes the partial state
/// explicit on the execution record.
pub(super) fn record_partial_shortcut(
    state: &AppState,
    execution_id: &str,
    applied: &[Value],
    failed: &std::path::Path,
    reason: &str,
) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["deterministicShortcut"] = json!({
            "kind": "file_replace_exact",
            "modelCalls": 0,
            "tokens": 0,
            "partial": true,
            "appliedEffectIds": applied,
            "failedPath": failed.to_string_lossy(),
            "reason": reason,
            "note": "deterministic execution optimization; excluded from learning evidence",
        });
        state.store.put("execution", &execution);
    }
}

/// No in-flight deterministic file operation may be followed by a new
/// operation or a final response after the owner cancelled this run.
fn ensure_frozen_not_cancelled(
    state:&AppState,request:&TurnRequest,session_id:&str,generation:u64,
)->Result<(),String>{
    if state.is_cancelled(session_id)
        || state.cancellation_generation(session_id)!=generation
        || super::super::transport::request_cancelled(
            state,session_id,request.request_id.as_deref()){
        return Err("실행이 중단되었습니다.".into());
    }
    Ok(())
}

/// Executes every frozen replacement before any model call. Reads are all
/// completed and validated first; a single mismatch falls back to the normal
/// model path without mutating anything.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_frozen_replacements(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    execution_id: &str,
    task_id: &str,
    text: &str,
    checks: Vec<Value>,
    generation:u64,
    metrics: &std::sync::Mutex<Metrics>,
) -> Result<Option<Value>, String> {
    if !request.tools_enabled
        || !request.tools_execution_enabled
        || std::env::var("DOMINION_DETERMINISTIC_REPLACEMENT").as_deref() == Ok("0")
    {
        return Ok(None);
    }
    ensure_frozen_not_cancelled(state,request,session_id,generation)?;
    let existing = state.store.execution_operations(execution_id)?;
    let mut op_seq = existing
        .iter()
        .filter_map(|operation| operation["opSeq"].as_u64())
        .max()
        .unwrap_or(0);
    let mut paths = Vec::new();
    for check in &checks {
        let raw = check["path"].as_str().ok_or("경로가 없습니다.")?;
        let path = crate::engine::verification::resolve_path(state, raw)?;
        if paths.iter().any(|existing| existing == &path) {
            // Two independently frozen hashes on one file cannot both be
            // validated after the first change. Let the model handle it.
            return Ok(None);
        }
        paths.push(path);
    }
    let mut prepared = Vec::new();
    for (check, path) in checks.iter().zip(paths) {
        ensure_frozen_not_cancelled(state,request,session_id,generation)?;
        op_seq += 1;
        let (read, _) = deterministic_tool(
            state,
            request,
            tx,
            session_id,
            execution_id,
            "file_read",
            &json!({"path": path.to_string_lossy()}),
            op_seq,
            metrics,
        )
        .await?;
        let Some(content) = read["content"].as_str() else {
            return Ok(None);
        };
        let source_sha256 = format!("{:x}", Sha256::digest(content.as_bytes()));
        let Some(expected) = replacement_content(check, content) else {
            return Ok(None);
        };
        prepared.push((path, source_sha256, expected));
    }
    let mut effect_ids = Vec::new();
    for (path, source_sha256, expected) in prepared {
        ensure_frozen_not_cancelled(state,request,session_id,generation)?;
        op_seq += 1;
        let (_, operation) = deterministic_tool(
            state,
            request,
            tx,
            session_id,
            execution_id,
            "file_write",
            &json!({
                "path": path.to_string_lossy(),
                "content": expected,
                // Compare immediately before writing, under the per-file lock.
                "expectedSha256": source_sha256,
            }),
            op_seq,
            metrics,
        )
        .await?;
        if operation["result"]["staleVersion"].as_bool() == Some(true) {
            record_partial_shortcut(
                state,
                execution_id,
                &effect_ids,
                &path,
                "source version changed before write",
            );
            return Err(format!(
                "{} 파일이 읽은 뒤 변경되어 쓰기를 중단했습니다. 이미 적용된 변경은 실행 기록에 남아 있습니다.",
                path.display()
            ));
        }
        if operation["effectState"] != "verified" {
            record_partial_shortcut(
                state,
                execution_id,
                &effect_ids,
                &path,
                "write was not independently verified",
            );
            return Err(format!(
                "{} 변경을 독립적으로 검증하지 못했습니다. 이미 적용된 변경은 실행 기록에 남아 있습니다.",
                path.display()
            ));
        }
        effect_ids.push(operation["effectId"].clone());
    }
    ensure_frozen_not_cancelled(state,request,session_id,generation)?;
    let answer = "요청한 정확 치환을 모델 호출 없이 수행하고 SHA-256으로 검증했습니다.".to_string();
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["acceptancePassed"] = json!(true);
        execution["finalAnswer"] = json!(answer);
        execution["toolsUsed"] = json!(["file_read", "file_write"]);
        execution["deterministicShortcut"] = json!({
            "kind": "file_replace_exact",
            "modelCalls": 0,
            "tokens": 0,
            "checks": checks.len(),
            "effectIds": effect_ids,
            "note": "deterministic execution optimization; excluded from learning evidence",
        });
        state.store.put("execution", &execution);
    }
    let (saved,linked)=state.store.write_final_message_and_link(execution_id,
        &json!({
            "session_id": session_id,
            "role": "assistant",
            "content": answer,
            "agent_id": request.agent_id,
            "metadata": {
                "authorKind": "agent",
                "executionId": execution_id,
                "taskId": task_id,
                "deterministicShortcut": true,
                "acceptanceText": text,
            },
        }))
        .ok_or("결정적 실행의 최종 응답을 원자적으로 저장할 수 없습니다.")?;
    state.bus.message(&saved);
    state.bus.record("execution",&linked);
    Ok(Some(saved))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    fn fixture(read_only: bool) -> (AppState, std::path::PathBuf, TurnRequest) {
        let root = std::env::temp_dir()
            .join(format!("dominion-frozen-gateway-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create fixture");
        let config = Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let store = Store::open(&config.db_path()).expect("open store");
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({
            "id": "frozen-session",
            "parallelReadOnly": read_only
        })).expect("persist session");
        state.store.put("execution", &json!({
            "id": "frozen-run",
            "sessionId": "frozen-session",
            "taskId": "frozen-task",
            "status": "running"
        })).expect("persist run");
        state.store.put("policy", &json!({
            "id": "default",
            "autoApproveAll": true
        })).expect("persist fixture approvals");
        let request = TurnRequest::from_value(&json!({
            "input": "Perform a frozen exact file replacement.",
            "sessionId": "frozen-session",
            "toolsEnabled": true,
            "toolsExecutionEnabled": true
        })).expect("valid request");
        (state, root, request)
    }

    #[tokio::test]
    async fn frozen_read_write_reuse_native_receipts_and_effect_verification() {
        let (state, root, request) = fixture(false);
        let path = root.join("contract.txt");
        std::fs::write(&path, "before").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let (read, read_op) = deterministic_tool(
            &state, &request, &tx, "frozen-session", "frozen-run",
            "file_read", &json!({"path": path}), 1, &metrics,
        ).await.expect("native read");
        assert_eq!(read["content"], "before");
        assert_eq!(read_op["origin"], "deterministic_shortcut");
        assert_eq!(read_op["toolReceipt"]["tool"], "file_read");
        assert_eq!(read_op["status"], "completed");
        let source_hash = format!("{:x}", Sha256::digest(b"before"));
        let (write, write_op) = deterministic_tool(
            &state, &request, &tx, "frozen-session", "frozen-run",
            "file_write", &json!({
                "path": path, "content": "after", "expectedSha256": source_hash
            }), 2, &metrics,
        ).await.expect("native conditional write");
        assert!(tool_succeeded(&write), "{write}");
        assert_eq!(write_op["origin"], "deterministic_shortcut");
        assert_eq!(write_op["status"], "completed");
        assert_eq!(write_op["effectState"], "verified", "{write_op}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after");
        assert_eq!(metrics.lock().unwrap().tool_calls, 2);
        let recorded = state.store.execution_operations("frozen-run").unwrap();
        assert_eq!(recorded.len(), 2);
        assert!(recorded.iter().any(|op| op["id"] == write_op["id"]));
        // No assistant tool_calls frame exists in a model-free shortcut.
        // Persisting bare tool responses would break the next provider turn.
        assert!(state.store.messages("frozen-session").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn frozen_shortcut_cannot_mutate_a_read_only_child() {
        let (state, root, request) = fixture(true);
        let path = root.join("must-not-appear.txt");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let (result, operation) = deterministic_tool(
            &state, &request, &tx, "frozen-session", "frozen-run",
            "file_write", &json!({"path": path, "content": "not allowed"}),
            1, &metrics,
        ).await.expect("denial must be a durable tool result");
        assert!(!tool_succeeded(&result), "{result}");
        assert_eq!(operation["status"], "failed");
        assert_eq!(operation["origin"], "deterministic_shortcut");
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
