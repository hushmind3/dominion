//! Bounded executor/reviewer/planner resolution and prompt utilities.
use super::*;

pub(super) fn role_model(
    state: &AppState,
    role: &Value,
    default_provider: &str,
    default_model: &str,
) -> (String, String) {
    let agent = role["agentId"]
        .as_str()
        .and_then(|id| state.store.get("agent", id))
        .unwrap_or(json!({}));
    (
        role["providerId"]
            .as_str()
            .or(agent["providerId"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(default_provider)
            .into(),
        role["model"]
            .as_str()
            .or(agent["model"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(default_model)
            .into(),
    )
}


pub(super) fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
pub(super) fn tail(text: &str, limit: usize) -> String {
    text.chars()
        .rev()
        .take(limit)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
