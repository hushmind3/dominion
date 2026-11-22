//! Scoped prompt commands preserve unrelated owner settings and worker state.
use crate::{db::ConditionalPatch, state::AppState};
use serde_json::{json, Value};

pub enum PromptError {
    Invalid(String),
    Missing,
    Changed,
    Storage(String),
}
fn invalid(message: &str) -> PromptError {
    PromptError::Invalid(message.into())
}
fn text(value: &Value, nullable: bool) -> Result<Value, PromptError> {
    match value {
        Value::Null if nullable=>Ok(Value::Null),
        Value::String(text) if text.chars().count()<=32_000=>Ok(json!(text)),
        _=>Err(invalid("프롬프트는 32,000자 이내 문자열이어야 합니다. 직접 지정을 해제할 때는 null을 사용하세요.")),
    }
}
fn persona_change(
    state: &AppState,
    id: &str,
    body: &Value,
) -> Result<ConditionalPatch, PromptError> {
    let agent = state.store.get("agent", id).ok_or(PromptError::Missing)?;
    let prompt = text(
        body.get("prompt")
            .ok_or_else(|| invalid("페르소나 내용이 필요합니다."))?,
        false,
    )?;
    let expected = match body.get("expectedPrompt") {
        Some(value) => text(value, false)?,
        None => agent["systemPrompt"].clone(),
    };
    // Legacy personas without a systemPrompt field are represented as empty text.
    let expected = if expected == "" && agent["systemPrompt"].is_null() {
        Value::Null
    } else {
        expected
    };
    Ok(ConditionalPatch {
        kind: "agent".into(),
        id: id.into(),
        expected: vec![("systemPrompt".into(), expected)],
        patch: json!({"systemPrompt":prompt}),
    })
}
fn commit(state: &AppState, changes: &[ConditionalPatch]) -> Result<Vec<Value>, PromptError> {
    let rows = state
        .store
        .patch_records_if_all(changes)
        .map_err(PromptError::Storage)?
        .ok_or(PromptError::Changed)?;
    for (change, row) in changes.iter().zip(&rows) {
        if change
            .patch
            .as_object()
            .is_some_and(|fields| !fields.is_empty())
        {
            state.bus.record(&change.kind, row);
        }
    }
    Ok(rows)
}
pub fn save_persona(state: &AppState, id: &str, body: &Value) -> Result<Value, PromptError> {
    let change = persona_change(state, id, body)?;
    commit(state, &[change])?
        .into_iter()
        .next()
        .ok_or_else(|| PromptError::Storage("Persona receipt missing".into()))
}

/// The unified persona form edits identity and prompt together. Omitted
/// fields (such as a full-system override) are preserved, and an editor's
/// captured baseline prevents an old form overwriting a newer change.
pub fn save_identity(state: &AppState, body: &Value) -> Result<Value, PromptError> {
    let fields = body
        .as_object()
        .ok_or_else(|| invalid("AI 설정 객체가 필요합니다."))?;
    let name = body["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| invalid("AI 이름을 입력하세요."))?;
    crate::engine::reasoning::validate(body).map_err(PromptError::Invalid)?;
    if let Some(prompt) = body.get("systemPrompt") {
        text(prompt, false)?;
    }
    if let Some(prompt) = body.get("systemPromptOverride") {
        text(prompt, true)?;
    }
    if body
        .get("expectedIdentity")
        .is_some_and(|value| !value.is_object())
    {
        return Err(invalid("AI 설정 원본을 확인할 수 없습니다."));
    }
    let id = body["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let current = state.store.get("agent", &id);
    let mut patch = json!({});
    for (field, value) in fields {
        if !matches!(
            field.as_str(),
            "id" | "expectedIdentity" | "createdAt" | "updatedAt"
        ) {
            patch[field] = value.clone();
        }
    }
    patch["name"] = json!(name.trim());
    if let Some(current) = current {
        let baseline = body.get("expectedIdentity").unwrap_or(&current);
        let expected = patch
            .as_object()
            .unwrap()
            .keys()
            .map(|field| (field.clone(), baseline[field].clone()))
            .collect();
        let change = ConditionalPatch {
            kind: "agent".into(),
            id,
            expected,
            patch,
        };
        commit(state, &[change])?
            .into_iter()
            .next()
            .ok_or(PromptError::Changed)
    } else {
        if body.get("expectedIdentity").is_some() {
            return Err(PromptError::Changed);
        }
        patch["id"] = json!(id);
        let saved = state
            .store
            .put_if_absent("agent", &patch)
            .ok_or(PromptError::Changed)?;
        state.bus.record("agent", &saved);
        Ok(saved)
    }
}
pub fn save_room(state: &AppState, id: &str, body: &Value) -> Result<Value, PromptError> {
    if !body.is_object() {
        return Err(invalid("프롬프트 설정 객체가 필요합니다."));
    }
    let room = state.store.get("room", id).ok_or(PromptError::Missing)?;
    let mut patch = json!({});
    let mut expected = Vec::new();
    for (key, field, expected_key) in [
        ("common", "systemPromptCommon", "expectedCommon"),
        ("final", "systemPromptOverride", "expectedFinal"),
    ] {
        if let Some(value) = body.get(key) {
            patch[field] = text(value, true)?;
            let original = match body.get(expected_key) {
                Some(value) => text(value, true)?,
                None => room[field].clone(),
            };
            expected.push((field.into(), original));
        }
    }
    let mut changes = Vec::new();
    if let Some(persona) = body.get("persona") {
        let agent_id = persona["agentId"]
            .as_str()
            .ok_or_else(|| invalid("수정할 AI를 선택하세요."))?;
        if !room["agentIds"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(agent_id)))
        {
            return Err(invalid("페르소나는 이 그룹의 참가자 중에서 선택하세요."));
        }
        expected.push(("agentIds".into(), room["agentIds"].clone()));
        changes.push(persona_change(state, agent_id, persona)?);
    }
    if expected.is_empty() {
        return Err(invalid("저장할 프롬프트 변경이 없습니다."));
    }
    changes.insert(
        0,
        ConditionalPatch {
            kind: "room".into(),
            id: id.into(),
            expected,
            patch,
        },
    );
    commit(state, &changes)?
        .into_iter()
        .next()
        .ok_or_else(|| PromptError::Storage("Room prompt receipt missing".into()))
}
