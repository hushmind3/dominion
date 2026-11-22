//! Owner connection commands. HTTP handlers do not mutate records or secrets.
use crate::{
    db::{ProviderSecretChange, ProviderWriteError},
    state::AppState,
};
use serde_json::{json, Value};

pub enum ConnectionError {
    Invalid(String),
    Missing,
    Changed,
    Storage(String),
}
impl From<ProviderWriteError> for ConnectionError {
    fn from(error: ProviderWriteError) -> Self {
        match error {
            ProviderWriteError::Changed => Self::Changed,
            ProviderWriteError::Storage(message) => Self::Storage(message),
        }
    }
}
fn invalid(message: impl Into<String>) -> ConnectionError {
    ConnectionError::Invalid(message.into())
}
fn optional_text<'a>(
    value: &'a Value,
    key: &str,
    max: usize,
) -> Result<Option<&'a str>, ConnectionError> {
    match value.get(key) {
        None => Ok(None),
        Some(Value::String(text)) if text.chars().count() <= max => Ok(Some(text.trim())),
        _ => Err(invalid(format!(
            "{key}는 {max}자 이내 문자열이어야 합니다."
        ))),
    }
}

pub fn normalize_endpoint(raw: &str) -> String {
    let value = raw.trim().trim_end_matches('/');
    if value.starts_with("https://opencode.ai/zen/go")
        || matches!(
            value,
            "https://opencode.ai/inference/go" | "https://opencode.ai/inference/go/v1"
        )
    {
        "https://opencode.ai/inference/go/openai/v1".into()
    } else {
        value.into()
    }
}

pub fn public_provider(state: &AppState, mut provider: Value) -> Value {
    let local = super::gateway::is_local_provider(&provider);
    let id = provider["id"].as_str().unwrap_or_default().to_string();
    let stored = state.store.has_secret(&format!("provider:{id}"));
    let readable = state
        .read_secret(&format!("provider:{id}"))
        .is_some_and(|key| !key.is_empty());
    if let Some(fields) = provider.as_object_mut() {
        let reflection = fields
            .get("reflection")
            .and_then(Value::as_bool)
            .or_else(|| fields.get("learning").and_then(Value::as_bool))
            .unwrap_or(false);
        fields.remove("apiKey");
        fields.insert("reflection".into(), json!(reflection));
        fields.insert("hasKey".into(), json!(readable));
        fields.insert("local".into(), json!(local));
        fields.insert(
            "credentialStatus".into(),
            json!(if readable {
                "saved"
            } else if stored {
                "unreadable"
            } else {
                "missing"
            }),
        );
    }
    provider
}

pub struct ConnectionDraft {
    pub record: Value,
    previous: Option<Value>,
    expected_secret: Option<String>,
    key: Option<String>,
}

impl ConnectionDraft {
    pub fn parse(state: &AppState, body: &Value) -> Result<Self, ConnectionError> {
        let fields = body
            .as_object()
            .ok_or_else(|| invalid("연결 설정 객체가 필요합니다."))?;
        let id = optional_text(body, "id", 160)?
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let previous = state.store.get("provider", &id);
        let mut record = previous.clone().unwrap_or_else(|| json!({"id":id}));
        for (key, value) in fields {
            if !matches!(
                key.as_str(),
                "apiKey" | "hasKey" | "credentialStatus" | "createdAt" | "updatedAt"
            ) {
                record[key] = value.clone();
            }
        }
        record["id"] = json!(id);
        for key in [
            "enabled",
            "local",
            "reflection",
            "learning",
            "requiresKey",
            "isDefault",
            "default",
        ] {
            if fields.get(key).is_some_and(|value| !value.is_boolean()) {
                return Err(invalid(format!("{key}는 true/false여야 합니다.")));
            }
        }
        for key in ["capabilities", "modelCapabilities"] {
            if fields.get(key).is_some_and(|value| !value.is_object()) {
                return Err(invalid(format!("{key}는 객체여야 합니다.")));
            }
        }
        super::reasoning::validate(&record).map_err(invalid)?;
        if let Some(profile) = record.get("nativeRouting") {
            super::native_routing::validate_profile(profile).map_err(invalid)?;
        }
        let kind = optional_text(&record, "kind", 32)?
            .unwrap_or("openai")
            .to_string();
        if !matches!(
            kind.as_str(),
            "openai" | "ollama" | "anthropic" | "codex" | "cli"
        ) {
            return Err(invalid("지원하지 않는 연결 방식입니다."));
        }
        if kind == "cli" && previous.as_ref().is_none_or(|row| row["kind"] != "cli") {
            return Err(invalid("CLI 연결은 계정 연결 화면에서 등록하세요."));
        }
        let base = normalize_endpoint(optional_text(&record, "baseUrl", 2048)?.unwrap_or(""));
        let parsed = reqwest::Url::parse(&base).map_err(|_| invalid("서버 주소를 확인하세요."))?;
        if kind == "cli" {
            if parsed.scheme() != "cli" {
                return Err(invalid("CLI 연결 주소를 확인하세요."));
            }
        } else if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid(
                "계정 정보나 조각 식별자가 없는 HTTP(S) 서버 주소를 입력하세요.",
            ));
        }
        let name = optional_text(&record, "name", 160)?
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| parsed.host_str().unwrap_or("API"))
            .to_string();
        record["name"] = json!(name);
        record["kind"] = json!(kind);
        record["baseUrl"] = json!(parsed.to_string().trim_end_matches('/'));
        let model = optional_text(&record, "model", 256)?
            .unwrap_or("")
            .to_string();
        if model.is_empty() {
            record.as_object_mut().unwrap().remove("model");
        } else {
            record["model"] = json!(model);
        }
        optional_text(&record, "accountId", 128)?;
        if fields
            .get("requestsPerMinute")
            .is_some_and(|v| v.as_u64().is_none_or(|n| n > 10000))
        {
            return Err(invalid("분당 요청 수는 0~10000 사이의 정수여야 합니다."));
        }
        let reflection = body
            .get("reflection")
            .or_else(|| body.get("learning"))
            .and_then(Value::as_bool)
            .or_else(|| {
                previous.as_ref().and_then(|row| {
                    row["reflection"]
                        .as_bool()
                        .or_else(|| row["learning"].as_bool())
                })
            })
            .unwrap_or(false);
        record["reflection"] = json!(reflection);
        record["learning"] = json!(reflection);
        if reflection {
            let review = optional_text(&record, "reflectionModel", 256)?
                .filter(|text| !text.is_empty())
                .or_else(|| record["model"].as_str())
                .ok_or_else(|| invalid("실행 검토에 사용할 모델을 선택하세요."))?
                .to_string();
            record["reflectionModel"] = json!(review);
        } else {
            record.as_object_mut().unwrap().remove("reflectionModel");
        }
        if record.get("enabled").is_none() {
            record["enabled"] = json!(true);
        }
        let tool_mode = optional_text(&record, "toolMode", 16)?
            .map(str::to_string)
            .unwrap_or_else(|| match kind.as_str() {
                "ollama" | "codex" => "compat".into(),
                "anthropic" => "native".into(),
                _ => "auto".into(),
            });
        if !matches!(tool_mode.as_str(), "auto" | "compat" | "native") {
            return Err(invalid("도구 연결 방식이 올바르지 않습니다."));
        }
        record["toolMode"] = json!(tool_mode);
        let key = optional_text(body, "apiKey", 65536)?
            .filter(|text| !text.starts_with("••"))
            .map(str::to_string);
        let expected_secret = state.store.get_secret(&format!("provider:{id}"));
        record.as_object_mut().unwrap().remove("apiKey");
        for key in ["hasKey", "credentialStatus"] {
            record.as_object_mut().unwrap().remove(key);
        }
        Ok(Self {
            record,
            previous,
            expected_secret,
            key,
        })
    }

    pub fn discovery_key(&self, state: &AppState) -> String {
        self.key.clone().unwrap_or_else(|| {
            state
                .read_secret(&format!(
                    "provider:{}",
                    self.record["id"].as_str().unwrap_or_default()
                ))
                .unwrap_or_default()
        })
    }

    pub fn commit(self, state: &AppState) -> Result<Value, ConnectionError> {
        let secret = match self.key {
            None => ProviderSecretChange::Keep,
            Some(key) if key.is_empty() => ProviderSecretChange::Clear,
            Some(key) => ProviderSecretChange::Replace(state.vault.seal(&key)),
        };
        let result = state.store.save_provider_connection(
            &self.record,
            self.previous.as_ref(),
            secret,
            self.expected_secret.as_deref(),
        )?;
        for provider in result.displaced {
            state.bus.record("provider", &provider);
        }
        state.bus.record("provider", &result.provider);
        state.store.audit(
            "provider.saved",
            json!({"providerId":result.provider["id"]}),
        );
        Ok(public_provider(state, result.provider))
    }
}

pub fn remove(state: &AppState, id: &str) -> Result<(), ConnectionError> {
    if id == "dominion-native" {
        return Err(ConnectionError::Invalid(
            "실행 중인 로컬 모델은 모델 관리에서 중지하세요.".into(),
        ));
    }
    let previous = state
        .store
        .get("provider", id)
        .ok_or(ConnectionError::Missing)?;
    state.store.remove_provider_connection(id, &previous)?;
    state
        .bus
        .emit(json!({"type":"record","kind":"provider","removed":id}));
    Ok(())
}
