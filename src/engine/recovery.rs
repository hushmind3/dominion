//! H4 failure classification shared by finalization, goals and jobs.
//!
//! The classifier is intentionally conservative: it only uses recorded
//! evidence (status, operations, acceptance checks) and never blames a specific
//! model. The hint is fed back into the next planning/replan step.
use crate::state::AppState;
use serde_json::{json, Value};

pub const UNKNOWN_EXTERNAL: &str = "unknown_external_result";
pub const PERMISSION_DENIED: &str = "permission_denied";
pub const LIMIT_EXCEEDED: &str = "limit_exceeded";
pub const MODEL_ERROR: &str = "model_error";
pub const VERIFICATION_FAILED: &str = "verification_failed";
pub const TOOL_FAILED: &str = "tool_failed";
pub const CANCELLED: &str = "cancelled";
pub const EXECUTION_ERROR: &str = "execution_error";

fn clamped_contains(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub fn classify(execution: &Value, operations: &[Value]) -> &'static str {
    if execution["status"].as_str() == Some("cancelled") {
        return CANCELLED;
    }
    if operations.iter().any(|operation| {
        operation["outcomeUnknown"] == true
            || matches!(
                operation["status"].as_str(),
                Some("interrupted" | "running")
            )
    }) {
        return UNKNOWN_EXTERNAL;
    }
    let reason = execution["reason"].as_str().unwrap_or("");
    let lowered = reason.to_lowercase();
    if lowered.contains("승인")
        || lowered.contains("거부")
        || lowered.contains("권한")
        || lowered.contains("denied")
        || lowered.contains("permission")
    {
        return PERMISSION_DENIED;
    }
    if lowered.contains("예산")
        || lowered.contains("시간 제한")
        || lowered.contains("token")
        || lowered.contains("limit")
    {
        return LIMIT_EXCEEDED;
    }
    if lowered.contains("연결 실패")
        || lowered.contains("http")
        || lowered.contains("모델")
        || lowered.contains("provider")
    {
        return MODEL_ERROR;
    }
    if clamped_contains(&lowered, &["절차 위반", "검증", "verification", "필수 절차"]) {
        return VERIFICATION_FAILED;
    }
    if !execution["acceptanceText"].is_null()
        && execution["acceptancePassed"].as_bool() != Some(true)
    {
        return VERIFICATION_FAILED;
    }
    if operations
        .iter()
        .any(|operation| operation["status"].as_str() == Some("failed"))
    {
        return TOOL_FAILED;
    }
    EXECUTION_ERROR
}

/// What the next attempt/replan should change. Not a claim that a retry will
/// succeed; an unknown external effect is explicitly non-retryable.
pub fn hint(class: &str) -> &'static str {
    match class {
        UNKNOWN_EXTERNAL => {
            "외부 도구 결과가 확정되지 않았습니다. 자동 재실행하지 말고 실제 상태를 먼저 확인하세요."
        }
        PERMISSION_DENIED => {
            "권한/승인이 거부되었습니다. 같은 호출을 반복하지 말고 허용된 범위나 다른 도구를 선택하세요."
        }
        LIMIT_EXCEEDED => {
            "예산 또는 시간 한도에 도달했습니다. 작업을 더 작은 단계로 나누고 불필요한 호출을 줄이세요."
        }
        MODEL_ERROR => {
            "모델/제공자 호출이 실패했습니다. 입력을 줄이거나 다른 제공자·모델로 전환하세요."
        }
        VERIFICATION_FAILED => {
            "검증 조건이 충족되지 않았습니다. 주장을 반복하지 말고 누락된 실제 산출물을 확인해 보완하세요."
        }
        TOOL_FAILED => {
            "도구가 실패했습니다. 인자와 환경을 확인하고 같은 명령을 그대로 반복하지 마세요."
        }
        CANCELLED => "실행이 취소되었습니다.",
        _ => "실행이 실패했습니다. 마지막 오류와 실제 증거를 확인한 뒤 다른 접근을 선택하세요.",
    }
}

/// Latest recorded failure hint for a task, used by goal replanning.
pub fn latest_failure_hint(state: &AppState, task_id: &str) -> Option<String> {
    let executions = match state.store.executions_for_task(task_id) {
        Ok(rows) => rows,
        Err(error) => {
            state.store.audit("recovery.task_query_failed", json!({
                "taskId": task_id, "error": error,
            }));
            return None;
        }
    };
    let latest = executions
        .into_iter()
        .filter(|execution| execution["failureClass"].as_str().is_some())
        .max_by_key(|execution| {
            execution["startedAt"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })?;
    let class = latest["failureClass"].as_str()?;
    Some(format!(
        "실행 실패 분류: {class} — {}",
        latest["recoveryHint"]
            .as_str()
            .unwrap_or_else(|| hint(class))
    ))
}

pub fn failure_record(execution: &Value, operations: &[Value]) -> Value {
    let class = classify(execution, operations);
    json!({"failureClass": class, "recoveryHint": hint(class)})
}

/// Records actual acceptance progress across tool rounds and model reviews.
/// Only verifier gaps and durable operation receipts can move the goal state.
/// This never converts a model claim or a successful read into task completion.
pub fn acceptance_progress(
    state: &AppState,
    execution_id: &str,
    round: usize,
    gaps: &[String],
    evidence: &[Value],
) -> Result<Value, String> {
    let mut execution = state.store.get("execution", execution_id)
        .ok_or("acceptance progress has no execution record")?;
    let mut history = execution["acceptanceTrajectory"]
        .as_array().cloned().unwrap_or_default();
    let previous = history.last();
    let mut current = gaps.to_vec();
    current.sort();
    current.dedup();
    // Stored operation ids are receipts, not claims that the goal is done.
    let mut receipts = evidence.iter().filter_map(|item| {
        item["id"].as_str().map(str::to_string)
    }).collect::<Vec<_>>();
    receipts.sort();
    receipts.dedup();
    if let Some(last) = previous {
        if last["round"].as_u64() == Some(round as u64)
            && last["gaps"] == json!(current)
            && last["receiptIds"] == json!(receipts) {
            return Ok(last.clone());
        }
    }
    let prior = previous.map(|last| last["gaps"].as_array()
        .into_iter().flatten().filter_map(Value::as_str)
        .map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let old_receipts = previous.map(|last| last["receiptIds"].as_array()
        .into_iter().flatten().filter_map(Value::as_str)
        .map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let resolved = prior.iter().filter(|item| !current.contains(item))
        .cloned().collect::<Vec<_>>();
    let introduced = current.iter().filter(|item| !prior.contains(item))
        .cloned().collect::<Vec<_>>();
    let fresh = receipts.iter().filter(|id| !old_receipts.contains(id))
        .cloned().collect::<Vec<_>>();
    let unchanged = !history.is_empty() && current == prior;
    let stagnant = if current.is_empty() { 0 } else if unchanged {
        previous.and_then(|item| item["stagnantRounds"].as_u64())
            .unwrap_or(0).saturating_add(1)
    } else { 0 };
    let mode = if current.is_empty() { "verified" }
        else if stagnant >= 1 { "reconsider" }
        else if !resolved.is_empty() { "advance" }
        else { "execute" };
    let focus = current.first().map(String::as_str).unwrap_or("");
    let action = if current.is_empty() {
        "All deterministic acceptance checks passed. Stop tool use.".to_string()
    } else if mode == "reconsider" {
        format!("The same unmet condition survived another execution round: {focus}. \
            Examine actual state/receipts and change the causal approach; \
            do not replay the same call just because it returned success.")
    } else if focus.contains("변경 또는 실행을 입증하는 성공한 도구 결과") {
        "Read-only observations cannot complete an action goal. Perform and verify \
         the requested effect through an authorized tool.".to_string()
    } else if focus.contains("file_replace_exact") {
        format!("Inspect the frozen input/output fingerprint and verify the exact replacement: {focus}")
    } else if focus.contains("file_exists") || focus.contains("file_changed")
        || focus.contains("file_equals") || focus.contains("file_contains") {
        format!("Read the named artifact and satisfy its missing filesystem postcondition: {focus}")
    } else if focus.contains("command_succeeded") {
        format!("Inspect the actual command receipt, exit code and stderr, then fix the cause: {focus}")
    } else {
        format!("Satisfy the first concrete missing postcondition with real evidence: {focus}")
    };
    let lineage = crate::engine::obligation_repair::reconstruct(
        &execution["acceptanceChecks"], &current,
        &state.store.execution_operations(execution_id)?,
    );
    let lineage_directive = lineage["directive"].as_str().unwrap_or("");
    // A verifier-derived, receipt-scoped directive overrides generic retry
    // advice only when the current attempt is actually stalled.
    let action = if mode == "reconsider" && !lineage_directive.is_empty() {
        format!("Replan ONLY the invalidated proof frontier:\n{lineage_directive}")
    } else { action };
    let record = json!({
        "obligationLineage": lineage,
        "round": round,
        "gaps": current,
        "receiptIds": receipts,
        "resolved": resolved,
        "introduced": introduced,
        "newReceiptIds": fresh,
        "stagnantRounds": stagnant,
        "mode": mode,
        "nextAction": action.chars().take(1300).collect::<String>(),
        "verified": current.is_empty(),
    });
    history.push(record.clone());
    if history.len() > 12 { history = history.split_off(history.len() - 12); }
    execution["acceptanceTrajectory"] = json!(history);
    execution["acceptanceProgress"] = record.clone();
    let saved = state.store.put("execution", &execution)
        .ok_or("acceptance progress could not be stored")?;
    state.bus.record("execution", &saved);
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceptance_feedback_repair_frontier_uses_real_verifier_and_sqlite_receipts() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir = std::env::temp_dir().join(format!(
            "dominion-obligation-runtime-{}", uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // The actual artifact contradicts the model/tool success claim.
        std::fs::write(dir.join("report.md"), "stale bytes").unwrap();
        let config = Config {
            port: 0, data_dir: dir.clone(), web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let check = json!({
            "type":"file_contains", "path":"report.md", "contains":"fresh bytes",
        });
        let checks = json!([check]);
        state.store.put("execution", &json!({
            "id":"obligation-run","sessionId":"obligation-session",
            "status":"running","acceptanceChecks":checks,
            "request":"update report.md to fresh bytes"
        }));
        for (id,tool,seq,args,result) in [
            ("read-old","workspace_read",1,json!({"path":"report.md"}),
                json!({"content":"stale bytes"})),
            ("write-claimed","workspace_write",2,json!({
                "path":"report.md","content":"fresh bytes"
            }),json!({"success":true})),
            ("read-again","workspace_read",3,json!({"path":"report.md"}),
                json!({"content":"stale bytes"})),
        ] {
            state.store.put("execution_operation",&json!({
                "id":id, "runId":"obligation-run",
                "sessionId":"obligation-session","tool":tool,
                "opSeq":seq,"status":"completed",
                "arguments":args,"result":result,
            }));
        }
        let evidence=state.store.execution_operations("obligation-run").unwrap();
        let gaps=crate::engine::verification::check_acceptance(
            &state,"update report.md to fresh bytes",&checks,&evidence,
        );
        assert!(!gaps.is_empty(), "real file must fail the frozen verifier");
        assert!(gaps.iter().any(|gap|gap.contains("file_contains")));
        let first=acceptance_progress(&state,"obligation-run",0,&gaps,&evidence).unwrap();
        assert_eq!(first["obligationLineage"]["frontiers"][0]["kind"],
            "postcondition_contradicts_mutation");
        assert_eq!(first["obligationLineage"]["frontiers"][0]["receiptIds"],
            json!(["read-old","write-claimed","read-again"]));
        let stalled=acceptance_progress(&state,"obligation-run",1,&gaps,&evidence).unwrap();
        assert_eq!(stalled["mode"],"reconsider");
        assert!(stalled["nextAction"].as_str().unwrap()
            .contains("CONTRADICTORY RECEIPTS"));
        assert_eq!(stalled["verified"],false);
        let persisted=state.store.get("execution","obligation-run").unwrap();
        assert_eq!(persisted["acceptanceProgress"]["obligationLineage"]["frontiers"][0]
            ["resource"],"workspace:report.md");
        // Real postcondition must succeed only when physical bytes change.
        std::fs::write(dir.join("report.md"), "fresh bytes").unwrap();
        let corrected=crate::engine::verification::check_acceptance(
            &state,"update report.md to fresh bytes",&checks,&evidence,
        );
        assert!(!corrected.iter().any(|gap|gap.contains("file_contains")),
            "the deterministic file verifier must observe corrected bytes");
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn acceptance_feedback_uses_verifier_delta_not_extra_tool_success() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir = std::env::temp_dir().join(format!("dominion-progress-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config { port: 0, data_dir: dir.clone(), web_dir: dir.join("web"), workspace: dir.clone() };
        let state = AppState::new(config.clone(), Store::open(&config.db_path()).unwrap(), Vault::open(&config.key_path()));
        state.store.put("execution", &json!({"id":"run"}));
        let evidence = vec![json!({"id":"op-a","tool":"workspace_read","result":{"ok":true}})];
        let first = acceptance_progress(&state,"run",0,&["file_exists A".into(),"file_exists B".into()],&evidence).unwrap();
        assert_eq!(first["mode"], "execute");
        let second = acceptance_progress(&state,"run",1,&["file_exists A".into(),"file_exists B".into()],&[evidence[0].clone(),json!({"id":"op-b"})]).unwrap();
        assert_eq!(second["mode"], "reconsider");
        assert_eq!(second["newReceiptIds"], json!(["op-b"]));
        assert_eq!(second["stagnantRounds"], 1);
        let third = acceptance_progress(&state,"run",2,&["file_exists B".into()],&evidence).unwrap();
        assert_eq!(third["mode"], "advance");
        assert_eq!(third["resolved"], json!(["file_exists A"]));
        assert_eq!(third["stagnantRounds"], 0);
        let fourth = acceptance_progress(&state,"run",3,&[],&evidence).unwrap();
        assert_eq!(fourth["verified"], true);
        let replay = acceptance_progress(&state,"run",3,&[],&evidence).unwrap();
        assert_eq!(replay, fourth);
        assert_eq!(state.store.get("execution","run").unwrap()["acceptanceTrajectory"].as_array().unwrap().len(),4);
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }


    #[test]
    fn task_failure_hint_uses_only_matching_task_history() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir = std::env::temp_dir().join(format!("dominion-task-recovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config { port: 0, data_dir: dir.clone(), web_dir: dir.join("web"), workspace: dir.clone() };
        let store = Store::open(&config.db_path()).unwrap();
        let state = AppState::new(config.clone(), store, Vault::open(&config.key_path()));
        state.store.put("execution", &json!({
            "id":"other", "taskId":"other-task", "failureClass":"permission_denied",
            "recoveryHint":"foreign hint", "startedAt":"2026-11-18T12:00:00Z"
        }));
        state.store.put("execution", &json!({
            "id":"older", "taskId":"task", "failureClass":"model_error",
            "recoveryHint":"older hint", "startedAt":"2026-11-17T12:00:00Z"
        }));
        state.store.put("execution", &json!({
            "id":"latest", "taskId":"task", "failureClass":"tool_failed",
            "recoveryHint":"matching hint", "startedAt":"2026-11-18T11:00:00Z"
        }));
        assert_eq!(state.store.executions_for_task("task").unwrap().len(), 2);
        assert!(latest_failure_hint(&state, "task").unwrap().contains("matching hint"));
        assert!(latest_failure_hint(&state, "other-task").unwrap().contains("foreign hint"));
        assert!(latest_failure_hint(&state, "missing").is_none());
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_external_effect_is_never_a_retryable_tool_error() {
        let execution = json!({"status":"failed","reason":"tool failed"});
        let operations = vec![json!({"status":"interrupted","outcomeUnknown":true})];
        assert_eq!(classify(&execution, &operations), UNKNOWN_EXTERNAL);
        assert!(hint(UNKNOWN_EXTERNAL).contains("자동 재실행하지"));
    }

    #[test]
    fn permission_and_limits_are_distinguished() {
        assert_eq!(
            classify(&json!({"status":"failed","reason":"승인이 거부되었습니다."}), &[]),
            PERMISSION_DENIED
        );
        assert_eq!(
            classify(&json!({"status":"failed","reason":"실행 시간 제한(10분)을 초과했습니다."}), &[]),
            LIMIT_EXCEEDED
        );
        assert_eq!(
            classify(&json!({"status":"failed","reason":"모델 호출 실패: HTTP 500"}), &[]),
            MODEL_ERROR
        );
    }

    #[test]
    fn verification_beats_generic_execution_error() {
        let execution = json!({
            "status":"completed",
            "acceptanceText":"file exists",
            "acceptancePassed":false
        });
        assert_eq!(classify(&execution, &[]), VERIFICATION_FAILED);
        assert_eq!(
            classify(&json!({"status":"cancelled"}), &[]),
            CANCELLED
        );
        assert_eq!(
            classify(
                &json!({"status":"failed","reason":"필수 절차 위반으로 완료할 수 없습니다"}),
                &[]
            ),
            VERIFICATION_FAILED
        );
    }
}
