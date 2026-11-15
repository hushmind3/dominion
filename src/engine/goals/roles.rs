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

/// Resolve and freeze the actual executor
pub(super) fn resolve_executor(
    state:&AppState,goal:&Value,text:&str
)->Result<(Option<String>,String,String),String>{
    let executor = goal.get("executor").cloned().unwrap_or_else(|| json!({}));
    let mut provider_id = string(&executor, "providerId");
    let mut model = string(&executor, "model");
    let agent_id = optional(&executor, "agentId");
    if let Some(agent) = agent_id
        .as_deref()
        .and_then(|id| state.store.get("agent", id))
    {
        if provider_id.is_empty() {
            provider_id = string(&agent, "providerId");
        }
        if model.is_empty() {
            model = string(&agent, "model");
        }
    }
    // A goal is at least standard complexity, even if its one-line text
    // looks like casual chat. Freeze the selected provider/model before
    // planning so reviews and resumed turns do not drift between models.
    if provider_id.is_empty() && model.is_empty() {
        if let (Some(provider),Some(selected_model))=(
            goal["resolvedExecutor"]["providerId"].as_str(),
            goal["resolvedExecutor"]["model"].as_str()
        ) {
            if state.store.get("provider",provider).is_some_and(|item|item["enabled"]!=false) {
                provider_id=provider.into();
                model=selected_model.into();
            }
        }
    }
    if provider_id.is_empty() && model.is_empty() {
        let complexity=match crate::engine::router::classify(&text,0,0) {
            crate::engine::router::Complexity::Light=>
                crate::engine::router::Complexity::Standard,
            other=>other,
        };
        if let Some(choice)=crate::engine::router::select_provider(state,complexity) {
            if let Ok(route)=crate::engine::gateway::route_from_provider(state,&choice,None) {
                provider_id=route.id;
                model=route.model;
            }
        }
    }
    if provider_id.is_empty() || model.is_empty() {
        // A named model without a provider resolves against the configured
        // default; no arbitrary provider is silently attached to an agent.
        let selected=crate::engine::gateway::resolve(state,&provider_id,Some(&model))?;
        provider_id=selected.id;
        model=selected.model;
    }
    Ok((agent_id,provider_id,model))
}
