//! Lightweight action plan and semantic review.
use super::*;

pub(super) fn simple_file_creation(
    state: &AppState, input: &str, acceptance: &Option<(String, Value)>,
) -> Option<std::path::PathBuf> {
    if !verification::simple_file_creation_goal(input) { return None; }
    let (_, checks) = acceptance.as_ref()?;
    let checks = checks.as_array()?;
    let [check] = checks.as_slice() else { return None; };
    if check["type"] != "file_exists" || check["inferred"] != true
        || check["creationOnly"] != true { return None; }
    crate::engine::verification::resolve_path(state, check["path"].as_str()?).ok()
}

/// A dedicated model planning call is justified only for truly composite
/// tasks. Ordinary single-step chat actions use their EXISTING model turn,
/// including tasks whose inferred acceptance checks happen to be empty.
/// The owner-selected planning policy and validated decomposition in turn.rs
/// may still explicitly request a plan.
pub(super) fn needs_plan(input: &str, acceptance: &Option<(String, Value)>) -> bool {
    let text = input.trim();
    if text.chars().count() > 240 || text.contains('\n') {
        return true;
    }
    if ["그리고", "그 다음", "다음으로", "한 뒤", "후에", "then", "after", ";"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        return true;
    }
    acceptance.as_ref()
        .and_then(|(_, checks)| checks.as_array())
        .is_some_and(|checks| checks.len() > 2)
}

pub(super) async fn action_plan(state: &AppState, request: &TurnRequest, text: &str) -> Option<String> {
    let plan = llm::complete_for_turn(
        state,
        request.provider_id.as_deref().unwrap_or(""),
        request.model.as_deref().unwrap_or(""),
        "You are the planner of a computer-use agent. Produce a short ordered plan (max 5 steps) in Korean. Include how to verify the result. Do not claim work was done.",
        &format!("요청: {text}"),
        request.session_id.as_deref(),
    )
    .await
    .ok()?;
    let plan = plan.trim().chars().take(1200).collect::<String>();
    (!plan.is_empty()).then_some(plan)
}

pub(crate) fn tool_succeeded(value: &Value) -> bool {
    value.get("isError").and_then(Value::as_bool) != Some(true)
        && value.get("success").and_then(Value::as_bool) != Some(false)
        && value.get("error").is_none_or(Value::is_null)
        && value
            .get("exitCode")
            .is_none_or(|code| code.as_i64() == Some(0))
        && value
            .get("status")
            .and_then(Value::as_u64)
            .is_none_or(|code| code < 400)
}


#[cfg(test)]
mod planning_cost_tests {
    use super::*;

    #[test]
    fn simple_action_without_inferred_checks_does_not_prepend_a_second_model_call() {
        assert!(!needs_plan("파일 읽어줘", &None));
        assert!(!needs_plan("README.md 검사", &Some((
            "README.md 검사".into(), json!([])
        ))));
        assert!(!needs_plan("파일 읽어줘", &Some((
            "파일 읽어줘".into(), json!([{"type":"file_exists","path":"a"}])
        ))));
    }

    #[test]
    fn composite_work_still_requests_planning_without_disabling_policy_override() {
        assert!(needs_plan("수정한 뒤 테스트하고 결과를 검증해", &None));
        assert!(needs_plan(&"복합 작업 ".repeat(50), &None));
        assert!(needs_plan("파일 처리", &Some((
            "파일 처리".into(), json!([
                {"type":"file_exists"},{"type":"file_changed"},
                {"type":"tool_succeeded"}
            ])
        ))));
    }
}
