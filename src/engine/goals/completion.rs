//! Durable completion, evidence publication and independent reviewer JSON contract.
use super::*;

pub(super) fn patch_goal(state: &AppState, id: &str, patch: Value) {
    if let Some(record) = state.store.get("goal", id) {
        if record["status"] == "cancelled" && patch["status"] != "cancelled" {
            return;
        }
        if let Some(saved) = state
            .store
            .patch_if("goal", id, "status", &record["status"], &patch)
        {
            state.bus.record("goal", &saved);
        }
    }
}
pub(super) fn finish(
    context: &JobContext,
    id: &str,
    status: &str,
    gaps: Vec<String>,
    error: Option<String>,
) -> Value {
    let cancelled = context
        .state
        .store
        .get("job", &context.job_id)
        .is_some_and(|job| job["cancelRequested"].as_bool() == Some(true));
    let status = if cancelled { "cancelled" } else { status };
    patch_goal(
        &context.state,
        id,
        json!({"status":status,"phase":if status=="completed" {"complete"} else {status},"gaps":gaps,"error":error,"finishedAt":chrono::Utc::now().to_rfc3339()}),
    );
    let goal = context
        .state
        .store
        .get("goal", id)
        .unwrap_or_else(|| json!({}));
    if let Some(task_id) = goal["taskId"].as_str().filter(|id| !id.is_empty()) {
        let final_status = goal["status"].as_str().unwrap_or(status);
        crate::engine::episode::set_root_outcome(
            &context.state,
            task_id,
            crate::engine::episode::goal_outcome(final_status),
            &gaps,
        );
    }
    context.state.store.audit(
        "goal.finished",
        json!({"goalId":id,"status":status,"iterations":goal.get("iterations")}),
    );
    crate::engine::goal_receipts::record(context,id,status,&goal,&gaps);
    if status == "completed" {
        // Only the independent reviewer + deterministic gate can generate
        // a reward, and an owner-rewritten plan does not credit the original
        // decomposition. Finalization may repeat after a crash: count once.
        if goal["planRevision"].as_u64().unwrap_or(0)==0 {
            if let Some(revision)=goal["decomposition"]["revision"].as_str() {
                let reward=json!({"id":format!("goal-policy-reward-{id}"),
                    "goalId":id,"kind":"task.decomposition",
                    "revision":revision,"source":"verified_goal_completion"});
                if let Some(saved)=context.state.store.put_if_absent("goal_policy_reward",&reward) {
                    context.state.bus.record("goal_policy_reward",&saved);
                    crate::engine::algorithms::record_owner_feedback(
                        &context.state,"task.decomposition",revision,true);
                }
            }
        }
        if let Some(session_id) = goal["sessionId"].as_str().filter(|id| !id.is_empty()) {
            let executor = goal.get("executor").cloned().unwrap_or_else(|| json!({}));
            let agent_id = executor["agentId"].as_str().filter(|id| !id.is_empty());
            let provider_id = goal["resolvedExecutor"]["providerId"].as_str()
                .or_else(||executor["providerId"].as_str()).unwrap_or("");
            let model = goal["resolvedExecutor"]["model"].as_str()
                .or_else(||executor["model"].as_str()).unwrap_or("");
            let through = context
                .state
                .store
                .messages(session_id)
                .last()
                .and_then(|message| message["id"].as_str().map(str::to_string));
            if let Some(agent_id) = agent_id {
                crate::engine::learning::enqueue_semantic_for_agent_through(
                    &context.state,
                    session_id,
                    Some(agent_id),
                    provider_id,
                    model,
                    through.as_deref(),
                );
            } else {
                crate::engine::learning::enqueue_semantic(
                    &context.state,
                    session_id,
                    provider_id,
                    model,
                );
            }
        }
    }
    context.emit(json!({"type":"goal_finished","goalId":id,"status":status}));
    json!({"goalId":id,"status":status,"sessionId":goal.get("sessionId"),
        "iterations":goal.get("iterations"),"artifacts":goal.get("artifacts"),
        "planHistory":goal.get("planHistory"),"reviewHistory":goal.get("reviewHistory"),
        "evidence":goal.get("evidence"),"gaps":gaps})
}

pub fn parse_review(text: &str) -> (bool, Vec<String>) {
    let trimmed = text.trim();
    let raw = if trimmed.starts_with("```") && trimmed.ends_with("```") {
        trimmed
            .split_once('\n')
            .map(|(_, body)| body[..body.len() - 3].trim())
            .unwrap_or("")
    } else {
        trimmed
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return (
            false,
            vec!["검토 응답이 유효한 JSON 객체가 아닙니다.".into()],
        );
    };
    let Some(complete) = value.get("complete").and_then(Value::as_bool) else {
        return (false, vec!["검토 complete는 boolean이어야 합니다.".into()]);
    };
    let Some(gaps) = value
        .get("gaps")
        .and_then(Value::as_array)
        .filter(|items| items.iter().all(Value::is_string))
    else {
        return (false, vec!["검토 gaps는 문자열 배열이어야 합니다.".into()]);
    };
    let gaps = gaps
        .iter()
        .filter_map(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.chars().take(1000).collect::<String>())
        .take(20)
        .collect::<Vec<_>>();
    (complete && gaps.is_empty(), gaps)
}
