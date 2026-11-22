//! Validated owner room configuration; worker state is never part of the patch.
use crate::{db::ConditionalPatch, state::AppState};
use serde_json::{json, Value};
use std::collections::HashSet;
fn bad(message: impl Into<String>) -> String {
    message.into()
}

fn text_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: &str,
    limit: usize,
) -> Result<String, String> {
    let value = body.get(key).or_else(|| previous.get(key));
    let text = match value {
        Some(value) => value
            .as_str()
            .ok_or_else(|| bad(format!("{key}는 문자열이어야 합니다.")))?,
        None => default,
    }
    .trim();
    if text.chars().count() > limit {
        return Err(bad(format!("{key}는 {limit}자 이하여야 합니다.")));
    }
    Ok(text.into())
}
fn bool_field(body: &Value, previous: &Value, key: &str, default: bool) -> Result<bool, String> {
    body.get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| bad(format!("{key}는 true/false여야 합니다.")))
        })
        .unwrap_or(Ok(default))
}
fn number_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: u64,
    max: u64,
) -> Result<u64, String> {
    let number = body
        .get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| bad(format!("{key}는 양의 정수여야 합니다.")))
        })
        .unwrap_or(Ok(default))?;
    if number == 0 || number > max {
        return Err(bad(format!("{key} 범위는 1~{max}입니다.")));
    }
    Ok(number)
}
fn number_field_allow_zero(
    body: &Value,
    previous: &Value,
    key: &str,
    max: u64,
) -> Result<u64, String> {
    let number = body
        .get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| bad(format!("{key}는 0 이상의 정수여야 합니다.")))
        })
        .unwrap_or(Ok(0))?;
    if number > max {
        return Err(bad(format!("{key} 범위는 0~{max}입니다.")));
    }
    Ok(number)
}
pub fn validated_config(
    state: &AppState,
    id: &str,
    previous: &Value,
    body: &Value,
) -> Result<Value, String> {
    if !body.is_object() {
        return Err(bad("회의실 객체가 필요합니다."));
    }
    if let Some(binding) = body.get("sessionId") {
        if binding != &previous["sessionId"] {
            return Err(bad("회의실의 대화 세션은 변경할 수 없습니다."));
        }
    }
    for reserved in [
        "roomState",
        "activeAgentId",
        "status",
        "generation",
        "cancelRequested",
        "ownerId",
        "partitionKey",
    ] {
        if body.get(reserved).is_some() {
            return Err(bad(format!("{reserved}는 서버가 관리하는 상태입니다.")));
        }
    }
    let name = text_field(body, previous, "name", "새 회의실", 160)?;
    if name.is_empty() {
        return Err(bad("회의실 이름이 필요합니다."));
    }
    // Omitted members retain legacy quick-create support. An explicit empty
    // or malformed selection is never replaced with arbitrary assistants.
    if let Some(ids) = body.get("agentIds") {
        if ids.as_array().is_none_or(|ids| ids.is_empty()) {
            return Err(bad("참여할 AI를 한 명 이상 선택하세요."));
        }
    }
    let requested = body.get("agentIds").or_else(|| previous.get("agentIds"));
    let filled: Vec<Value>;
    let source = match requested
        .and_then(Value::as_array)
        .filter(|ids| !ids.is_empty())
    {
        Some(ids) => ids,
        None => {
            filled = state
                .store
                .all("agent")
                .into_iter()
                .filter(|agent| agent["enabled"] != false)
                .take(3)
                .map(|agent| agent["id"].clone())
                .collect();
            if filled.is_empty() {
                return Err(bad("먼저 도우미를 한 명 만들어 주세요."));
            }
            &filled
        }
    };
    if source.len() > 32 {
        return Err(bad("참가자는 32명까지 가능합니다."));
    }
    let mut known = HashSet::new();
    let mut agent_ids = Vec::new();
    for value in source {
        let id = value
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| bad("유효한 에이전트 ID가 필요합니다."))?;
        if state.store.get("agent", id).is_none() {
            return Err(bad(format!("참가 에이전트를 찾을 수 없습니다: {id}")));
        }
        if known.insert(id.to_string()) {
            agent_ids.push(id.to_string());
        }
    }
    let routing = text_field(body, previous, "routingMode", "discussion", 32)?;
    if !matches!(routing.as_str(), "discussion" | "round_robin" | "mentioned") {
        return Err(bad("지원하지 않는 발언 방식입니다."));
    }
    // Owner-picked model pool. FreeLLMAPI exposes dozens of models; the owner
    // checks the ones they trust once and every turn rotates inside that list.
    if body.get("models").is_some_and(|models| !models.is_array()) {
        return Err(bad("그룹 모델은 목록으로 선택하세요."));
    }
    let models_source = body
        .get("models")
        .or_else(|| previous.get("models"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if models_source.len() > 64 {
        return Err(bad("방 모델은 최대 64개까지 선택할 수 있습니다."));
    }
    let mut models = Vec::new();
    let mut seen_models = HashSet::new();
    for entry in &models_source {
        let provider_id = entry
            .get("providerId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad("방 모델에 providerId가 필요합니다."))?;
        let model = entry
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 200)
            .ok_or_else(|| bad("방 모델에 model 이름이 필요합니다."))?;
        state
            .store
            .get("provider", provider_id)
            .ok_or_else(|| bad(format!("제공자를 찾을 수 없습니다: {provider_id}")))?;
        let effort = entry
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !valid_effort(effort) {
            return Err(bad("추론 강도 값이 올바르지 않습니다."));
        }
        if seen_models.insert(format!("{provider_id}|{model}")) {
            let mut picked = json!({"providerId":provider_id,"model":model});
            if !effort.is_empty() {
                picked["reasoningEffort"] = json!(effort);
            }
            models.push(picked);
        }
    }
    let max_turns = number_field_allow_zero(body, previous, "maxTurns", 100_000)?;
    let reasoning_effort = text_field(body, previous, "reasoningEffort", "", 16)?;
    if !valid_effort(&reasoning_effort) {
        return Err(bad("추론 강도 값이 올바르지 않습니다."));
    }
    Ok(json!({
        "id":id,"name":name,"agentIds":agent_ids,"routingMode":routing,
        "description":text_field(body,previous,"description","",4000)?,
        "topic":text_field(body,previous,"topic","",16000)?,
        "ownerName":text_field(body,previous,"ownerName","나",100)?,
        "rounds":number_field(body,previous,"rounds",1,100_000)?,
        "maxTurns":max_turns,
        "tokenBudget":number_field_allow_zero(body,previous,"tokenBudget",10_000_000)?,
        "contextBudgetTokens":number_field(body,previous,"contextBudgetTokens",2_000,1_000_000)?,
        "toolsEnabled":bool_field(body,previous,"toolsEnabled",false)?,
        "autoLearning":bool_field(body,previous,"autoLearning",false)?,
        "autoModels":bool_field(body,previous,"autoModels",true)?,
        "reasoningEffort":reasoning_effort,
        "models":models,
    }))
}
fn valid_effort(value: &str) -> bool {
    crate::engine::reasoning::valid_effort(value)
}

pub enum ConfigError {
    Invalid(String),
    Missing,
    Changed,
    Storage(String),
}
pub fn save(
    state: &AppState,
    id_override: Option<&str>,
    body: &Value,
) -> Result<Value, ConfigError> {
    if !body.is_object() {
        return Err(ConfigError::Invalid("그룹 설정 객체가 필요합니다.".into()));
    }
    if body.get("id").is_some_and(|value| !value.is_string()) {
        return Err(ConfigError::Invalid(
            "그룹 ID는 문자열이어야 합니다.".into(),
        ));
    }
    let id = id_override
        .or_else(|| body["id"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
    {
        return Err(ConfigError::Invalid("유효하지 않은 그룹 ID입니다.".into()));
    }
    if body["id"].as_str().is_some_and(|value| value != id) {
        return Err(ConfigError::Invalid(
            "경로와 본문의 그룹 ID가 다릅니다.".into(),
        ));
    }
    if body.get("configRevision").is_some() {
        return Err(ConfigError::Invalid(
            "설정 버전은 서버가 관리합니다.".into(),
        ));
    }
    let key = format!("room-submit:{id}");
    if !state.begin_session(&key) {
        return Err(ConfigError::Changed);
    }
    let _guard = state.session_guard(&key);
    let previous = state.store.get("room", &id).unwrap_or(Value::Null);
    if id_override.is_some() && previous.is_null() {
        return Err(ConfigError::Missing);
    }
    let revision = previous["configRevision"].as_u64().unwrap_or(0);
    if let Some(expected) = body.get("expectedConfigRevision") {
        if expected.as_u64().is_none() {
            return Err(ConfigError::Invalid(
                "설정 버전은 양의 정수여야 합니다.".into(),
            ));
        }
        if expected.as_u64() != Some(revision) {
            return Err(ConfigError::Changed);
        }
    }
    let mut config = validated_config(state, &id, &previous, body).map_err(ConfigError::Invalid)?;
    config["configRevision"] = json!(revision.checked_add(1).ok_or(ConfigError::Changed)?);
    let saved = if previous.is_null() {
        state
            .store
            .put_if_absent("room", &config)
            .ok_or(ConfigError::Changed)?
    } else {
        config.as_object_mut().unwrap().remove("id");
        let expected = config
            .as_object()
            .unwrap()
            .keys()
            .map(|key| (key.clone(), previous[key].clone()))
            .collect();
        state
            .store
            .patch_records_if_all(&[ConditionalPatch {
                kind: "room".into(),
                id: id.clone(),
                expected,
                patch: config,
            }])
            .map_err(ConfigError::Storage)?
            .ok_or(ConfigError::Changed)?
            .into_iter()
            .next()
            .ok_or(ConfigError::Changed)?
    };
    state.bus.record("room", &saved);
    super::rooms::ensure_room_session(state, &id).map_err(ConfigError::Storage)?;
    state.store.get("room", &id).ok_or(ConfigError::Missing)
}
