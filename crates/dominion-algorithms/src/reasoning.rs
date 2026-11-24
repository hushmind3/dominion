//! Shared effort contract. Model catalogs, not UI tiers, decide availability.
use serde_json::{json, Value};

pub fn valid_effort(value: &str) -> bool {
    value.is_empty() || (value.len() <= 32
        && value.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'))
}

pub fn validate(value: &Value) -> Result<(), String> {
    match value.get("reasoningEffort") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(effort)) if valid_effort(effort) => Ok(()),
        _ => Err("추론 강도 값이 올바르지 않습니다.".into()),
    }
}

/// Never drop `none` or silently downgrade a requested tier. A missing value
/// leaves the provider's advertised default in charge.
pub fn codex_options(catalog: Option<&Value>, model: &str, effort: Option<&str>) -> Result<Option<Value>, String> {
    let Some(effort) = effort.filter(|value| !value.is_empty()) else { return Ok(None) };
    if !valid_effort(effort) { return Err("추론 강도 값이 올바르지 않습니다.".into()); }
    let row = catalog.and_then(|catalog| catalog["models"].as_array())
        .and_then(|rows| rows.iter().find(|row| row["id"] == model));
    if let Some(supported) = row.and_then(|row| row["reasoningEfforts"].as_array()).filter(|items| !items.is_empty()) {
        if !supported.iter().any(|value| value.as_str() == Some(effort)) {
            let levels = supported.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
            return Err(format!("{model}은 추론 강도 '{effort}'를 지원하지 않습니다. 지원 값: {levels}"));
        }
    }
    Ok(Some(json!({"effort":effort})))
}

/// The UI reads the same resolved model route as the chat transport. This
/// describes controls implemented by DOMINION, not a live provider probe.
/// Unknown generic endpoint capabilities must not invent selectable tiers.
pub fn inference_controls(kind: &str, protocol: &str, declared: &[Value], reasoning: Option<bool>) -> Value {
    let (toggle, efforts): (bool, Vec<&str>) = if reasoning == Some(false) || kind == "cli" {
        (false, vec![])
    } else if kind == "codex" {
        let efforts: Vec<_> = declared.iter().filter_map(Value::as_str)
            .filter(|effort| !effort.is_empty() && valid_effort(effort)).collect();
        (efforts.contains(&"none"), efforts)
    } else {
        match protocol {
            // The existing transport forwards only enable_thinking.
            "chat_template" | "qwen" => (true, vec![]),
            // These transports implement precisely these distinct choices.
            "deepseek" => (true, vec!["low", "high"]),
            "anthropic" => (true, vec!["low", "medium", "high"]),
            // Generic compatible transport forwards effort, not a boolean.
            // Its off value is not yet preserved by the native turn resolver.
            _ => (false, declared.iter().filter_map(Value::as_str)
                .filter(|effort| !effort.is_empty() && *effort != "none" && valid_effort(effort)).collect()),
        }
    };
    json!({"thinkingToggle":toggle,"reasoningEfforts":efforts})
}


/// Automatic defaults may only use fields implemented by this route.
/// Explicit legacy SDK settings retain their existing public contract.
pub fn automatic_room_reasoning(support:&Value, requested:&str)->(Option<String>,Option<bool>) {
    let toggle=support["thinkingToggle"].as_bool()==Some(true);
    let supported=support["reasoningEfforts"].as_array().is_some_and(|rows|rows.iter().any(|row|row.as_str()==Some(requested)));
    if requested=="none" {
        return (supported.then(||requested.to_string()),toggle.then_some(false));
    }
    (supported.then(||requested.to_string()),toggle.then_some(true))
}

/// A model-pool override is more specific than the room or message default.
pub fn room_reasoning_choice(rotated:Option<String>, input:&Value, room:&Value, agent:&Value, provider:Option<&Value>, automatic:(Option<String>,Option<bool>))->(Option<String>,Option<bool>){
    if let Some(effort)=rotated {
        let enabled=effort!="none";
        return (Some(effort),Some(enabled));
    }
    let effort=[Some(input),Some(room),Some(agent),provider].into_iter().flatten()
        .find_map(|row|row["reasoningEffort"].as_str().filter(|v|!v.is_empty()).map(str::to_string));
    let explicit=input["thinking"].as_bool().or_else(||room["thinking"].as_bool());
    if let Some(effort)=effort {
        let enabled=effort!="none";
        return (Some(effort),explicit.or(Some(enabled)));
    }
    if let Some(enabled)=explicit.or_else(||agent["thinking"].as_bool()) {
        // A stored boolean is an explicit decision, not a heuristic.
        return (None,Some(enabled));
    }
    automatic
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_reasoning_respects_per_model_override_and_native_support() {
        let room=json!({"reasoningEffort":"high"});
        let input=json!({"thinking":false});
        let agent=json!({"thinking":false});
        assert_eq!(room_reasoning_choice(Some("low".into()),&input,&room,&agent,None,(None,None)),(Some("low".into()),Some(true)));
        assert_eq!(room_reasoning_choice(Some("none".into()),&json!({"thinking":true}),&room,&agent,None,(None,None)),(Some("none".into()),Some(false)));
        assert_eq!(room_reasoning_choice(None,&json!({}),&room,&agent,None,(None,None)),(Some("high".into()),Some(true)));
        assert_eq!(automatic_room_reasoning(&inference_controls("openai","openai",&[],None),"medium"),(None,None));
        assert_eq!(automatic_room_reasoning(&inference_controls("ollama","chat_template",&[],None),"none"),(None,Some(false)));
        assert_eq!(automatic_room_reasoning(&inference_controls("ollama","chat_template",&[],None),"medium"),(None,Some(true)));
    }

    #[test]
    fn advertised_efforts_are_preserved_and_unsupported_choices_are_rejected() {
        let catalog = json!({"models":[{"id":"dynamic-model","reasoningEfforts":["none","low","xhigh","max","ultra","future_tier"]}]});
        for effort in ["none", "low", "xhigh", "max", "ultra", "future_tier"] {
            assert_eq!(codex_options(Some(&catalog), "dynamic-model", Some(effort)).unwrap(), Some(json!({"effort":effort})));
        }
        assert!(codex_options(Some(&catalog), "dynamic-model", Some("high")).is_err());
        assert_eq!(codex_options(Some(&catalog), "dynamic-model", None).unwrap(), None);
        assert!(validate(&json!({"reasoningEffort":false})).is_err());
        assert!(!valid_effort("ultra\n"));
    }
}
