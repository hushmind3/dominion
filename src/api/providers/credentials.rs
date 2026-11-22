use super::{oauth::save_codex_provider, records::public_provider};
use crate::api::{cli_oauth, response::error};
use crate::{engine::codex_oauth, state::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

fn home_file(relative: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(relative))
}

pub(super) fn codex_home_file(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| home_file(".codex"))
        .map(|home| home.join(name))
}

/// Codex stores a ChatGPT OAuth session in ~/.codex/auth.json. Only the
/// presence and account are exposed to the UI; tokens never leave the vault.
pub(super) fn codex_auth() -> Result<Value, String> {
    let path =
        codex_home_file("auth.json").ok_or("CODEX_HOME/HOME 환경변수를 찾을 수 없습니다.")?;
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "Codex 로그인 정보를 찾을 수 없습니다. Codex 앱에서 먼저 로그인하세요.")?;
    let value: Value =
        serde_json::from_str(&text).map_err(|_| "Codex 로그인 정보 형식을 읽을 수 없습니다.")?;
    let access = value["tokens"]["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("Codex OAuth 토큰이 없습니다. Codex 앱에서 ChatGPT로 로그인하세요.")?;
    if codex_oauth::account_id(&value["tokens"]).is_none() {
        return Err("Codex 계정 ID가 없습니다. Codex 앱에서 다시 로그인하세요.".into());
    }
    let _ = access;
    Ok(value)
}

/// OpenCode keeps reusable credentials in a per-provider auth map. Plain API
/// keys can be imported; OAuth access tokens stay with the issuing client.
fn opencode_auth() -> Option<Value> {
    [
        ".local/share/opencode/auth.json",
        ".config/opencode/auth.json",
    ]
    .iter()
    .filter_map(|relative| home_file(relative))
    .find_map(|path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    })
}

pub(super) fn opencode_go_key_in_auth(value: &Value) -> Option<&str> {
    ["opencode-go", "opencode_go"].iter().find_map(|name| {
        let entry = value.get(*name)?;
        if !matches!(entry["type"].as_str(), Some("api" | "wellknown")) {
            return None;
        }
        entry["key"]
            .as_str()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    })
}

/// One-time explicit import; subsequent model requests use the DOMINION vault.
fn local_opencode_go_key() -> Option<(String, &'static str)> {
    if let Ok(value) = std::env::var("OPENCODE_API_KEY") {
        let value = value.trim();
        if !value.is_empty() {
            return Some((value.to_string(), "environment"));
        }
    }
    if let Some(auth) = opencode_auth() {
        if let Some(key) = opencode_go_key_in_auth(&auth) {
            return Some((key.to_string(), "opencode-cli"));
        }
    }
    None
}

pub(super) fn official_opencode_go_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("https://opencode.ai/zen/go")
        || trimmed == "https://opencode.ai/inference/go"
        || trimmed == "https://opencode.ai/inference/go/v1"
    {
        "https://opencode.ai/inference/go/openai/v1".to_string()
    } else {
        trimmed.to_string()
    }
}

fn direct_opencode_go(state: &AppState) -> Option<Value> {
    state.store.all("provider").into_iter().find(|provider| {
        provider["integration"] == "opencode-go"
            || provider["baseUrl"].as_str().is_some_and(|url| {
                url.starts_with("https://opencode.ai/inference/go/openai/v1")
                    || url.starts_with("https://opencode.ai/zen/go/v1")
            })
    })
}

fn import_opencode_go(state: &AppState) -> Result<Value, String> {
    let (key, source) = local_opencode_go_key()
        .ok_or("OpenCode Go 키가 로컬에 없습니다. API 키 직접 등록을 이용하세요.")?;
    let previous = direct_opencode_go(state).unwrap_or(Value::Null);
    let id = previous["id"].as_str().unwrap_or("opencode-go").to_string();
    let record = json!({
        "id": id,
        "name": previous["name"].as_str().unwrap_or("OpenCode Go"),
        "baseUrl": previous["baseUrl"].as_str().unwrap_or("https://opencode.ai/inference/go/openai/v1"),
        "kind": "openai",
        "model": previous["model"].as_str().unwrap_or(""),
        "enabled": true,
        "local": false,
        "integration": "opencode-go",
        "toolMode": previous["toolMode"].as_str().unwrap_or("auto"),
        "reflection": previous["reflection"].as_bool().unwrap_or(false),
        "requestsPerMinute": previous["requestsPerMinute"].as_u64().unwrap_or(0)
    });
    state.save_secret(&format!("provider:{id}"), &key);
    let saved = state
        .store
        .put("provider", &record)
        .ok_or("OpenCode Go 연결 정보를 저장하지 못했습니다.")?;
    state.store.audit(
        "provider.credential_imported",
        json!({"providerId":id,"source":source,"credentialCopied":true}),
    );
    state.bus.record("provider", &saved);
    let mut result = public_provider(state, saved);
    result["importedCount"] = json!(1);
    result["credentialSource"] = json!(source);
    result["verification"] = json!("not_tested");
    Ok(result)
}

pub(super) fn opencode_api_entries(auth: &Value) -> Vec<(String, String)> {
    auth.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(provider, credential)| {
            let kind = credential["type"].as_str().unwrap_or("");
            if !matches!(kind, "api" | "wellknown") {
                return None;
            }
            let key = credential["key"]
                .as_str()
                .filter(|value| !value.is_empty())?;
            Some((provider.clone(), key.to_string()))
        })
        .collect()
}

pub(super) fn opencode_target(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match id {
        "openai" => Some(("OpenAI", "https://api.openai.com/v1", "openai")),
        "anthropic" => Some(("Anthropic", "https://api.anthropic.com/v1", "anthropic")),
        "google" | "gemini" => Some((
            "Google Gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "openai",
        )),
        "openrouter" => Some(("OpenRouter", "https://openrouter.ai/api/v1", "openai")),
        "deepseek" => Some(("DeepSeek", "https://api.deepseek.com/v1", "openai")),
        "groq" => Some(("Groq", "https://api.groq.com/openai/v1", "openai")),
        "xai" => Some(("xAI", "https://api.x.ai/v1", "openai")),
        "mistral" => Some(("Mistral", "https://api.mistral.ai/v1", "openai")),
        "together" => Some(("Together", "https://api.together.xyz/v1", "openai")),
        "cerebras" => Some(("Cerebras", "https://api.cerebras.ai/v1", "openai")),
        "fireworks" => Some((
            "Fireworks",
            "https://api.fireworks.ai/inference/v1",
            "openai",
        )),
        "zai" | "z-ai" => Some(("z.ai", "https://api.z.ai/api/paas/v4", "openai")),
        "moonshotai" => Some(("Moonshot", "https://api.moonshot.cn/v1", "openai")),
        "alibaba" => Some((
            "Alibaba DashScope",
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            "openai",
        )),
        _ => None,
    }
}

pub(super) fn provider_slug(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn import_opencode(state: &AppState) -> Result<Value, String> {
    let auth = opencode_auth()
        .ok_or("OpenCode 인증 파일을 찾을 수 없습니다. OpenCode에서 먼저 로그인하세요.")?;
    let entries = opencode_api_entries(&auth);
    if entries.is_empty() {
        return Err(
            "OpenCode 인증 파일에 가져올 API 키가 없습니다. OAuth 자격 증명은 다른 앱으로 복사하지 않습니다."
                .into(),
        );
    }
    let mut first: Option<Value> = None;
    let mut imported = 0usize;
    let mut skipped = Vec::new();
    for (provider, key) in entries {
        let Some((name, base_url, kind)) = opencode_target(&provider) else {
            skipped.push(provider);
            continue;
        };
        let id = format!("opencode-{}", provider_slug(&provider));
        let previous = state.store.get("provider", &id).unwrap_or(Value::Null);
        let record = json!({
            "id": id,
            "name": format!("OpenCode · {name}"),
            "baseUrl": base_url,
            "kind": kind,
            "model": previous["model"].as_str().unwrap_or(""),
            "enabled": true,
            "local": false,
            "integration": "opencode",
            "reflection": previous["reflection"].as_bool().unwrap_or(false),
            "learning": previous["reflection"].as_bool().unwrap_or(false),
            "requestsPerMinute": previous["requestsPerMinute"].as_u64().unwrap_or(0),
        });
        state.save_secret(&format!("provider:{id}"), &key);
        if let Some(saved) = state.store.put("provider", &record) {
            state.bus.record("provider", &saved);
            if first.is_none() {
                first = Some(public_provider(state, saved));
            }
            imported += 1;
        }
    }
    let mut response = first.ok_or_else(|| {
        format!(
            "가져올 수 있는 OpenCode 제공자가 없습니다: {}",
            skipped.join(", ")
        )
    })?;
    response["importedCount"] = json!(imported);
    response["skipped"] = json!(skipped);
    state.store.audit(
        "provider.saved",
        json!({"source":"opencode-auth","imported":imported}),
    );
    Ok(response)
}

pub(super) fn codex_model() -> String {
    if let Some(path) = codex_home_file("config.toml") {
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let line = line.trim();
                if let Some(value) = line.strip_prefix("model = ") {
                    let model = value.trim().trim_matches('"');
                    if !model.is_empty() {
                        return model.to_string();
                    }
                }
            }
        }
    }
    "gpt-6.1-sol".to_string()
}

// Share one 24-hour snapshot across public provider and model browsing.
// The async fetch lock also coalesces simultaneous cold-cache requests: the
fn cli_provider_id(source: &str) -> String {
    format!("cli-{source}")
}

fn ensure_cli_provider(state: &AppState, source: &str, authenticated: bool) -> bool {
    if !authenticated {
        return false;
    }
    let id = cli_provider_id(source);
    if state.store.get("provider", &id).is_none() {
        let (name, default_model) = match source {
            "claude" => ("Claude Code (CLI 계정)", "default"),
            "gemini" => ("Gemini CLI (Google 계정)", "auto"),
            "opencode" => ("OpenCode (CLI 계정)", "default"),
            _ => return false,
        };
        let record = json!({"id":id,"name":name,"baseUrl":format!("cli://{source}"),
      "kind":"cli","model":default_model,"enabled":true,"local":false,
      "toolMode":"compat","reasoningProtocol":"openai"});
        if let Some(saved) = state.store.put("provider", &record) {
            state.bus.record("provider", &saved);
        }
    }
    state.store.get("provider", &id).is_some()
}

pub async fn set_cli_model(
    State(state): State<AppState>,
    Path(source): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !matches!(source.as_str(), "claude" | "gemini" | "opencode") {
        return error(StatusCode::BAD_REQUEST, "지원하지 않는 CLI입니다.");
    }
    let id = cli_provider_id(&source);
    let Some(mut record) = state.store.get("provider", &id) else {
        return error(StatusCode::NOT_FOUND, "먼저 CLI 계정을 연결하세요.");
    };
    let Some(model) = body["model"]
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.len() < 160 && !v.chars().any(char::is_whitespace))
    else {
        return error(StatusCode::BAD_REQUEST, "모델 ID가 올바르지 않습니다.");
    };
    record["model"] = json!(model);
    match state.store.put("provider", &record) {
        Some(saved) => {
            state.bus.record("provider", &saved);
            Json(public_provider(&state, saved)).into_response()
        }
        None => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "모델 선택을 저장하지 못했습니다.",
        ),
    }
}

pub async fn cli_credentials(State(state): State<AppState>) -> Json<Value> {
    let codex = codex_auth().ok();
    let connected = state.store.get("provider", "codex-oauth");
    let claude = cli_oauth::credential_verified("claude").await;
    let gemini = cli_oauth::credential_verified("gemini").await;
    let opencode_auth = opencode_auth();
    let opencode = cli_oauth::credential_verified("opencode").await
        || opencode_auth
            .as_ref()
            .and_then(Value::as_object)
            .is_some_and(|entries| !entries.is_empty());
    let go_source = local_opencode_go_key().map(|(_, source)| source);
    let go_provider = direct_opencode_go(&state);
    let go_connected = go_provider
        .as_ref()
        .and_then(|p| p["id"].as_str())
        .is_some_and(|id| state.store.has_secret(&format!("provider:{id}")));
    let claude_connected = ensure_cli_provider(&state, "claude", claude);
    let gemini_connected = ensure_cli_provider(&state, "gemini", gemini);
    let opencode_connected = ensure_cli_provider(&state, "opencode", opencode);
    let opencode_importable = opencode_auth
        .as_ref()
        .map(|auth| {
            opencode_api_entries(auth)
                .into_iter()
                .filter(|(provider, _)| opencode_target(provider).is_some())
                .count()
        })
        .unwrap_or(0);
    Json(json!([
        {
            "id":"codex",
            "name":"Codex (ChatGPT OAuth)",
            "loginSupported":true,
            "loginMethod":"browser",
            "available":codex.is_some() || connected.is_some(),
            "importAvailable":codex.is_some(),
            "connected":connected.is_some(),
            "supported":true,
            "accountId":connected.as_ref().and_then(|value| value["accountId"].as_str()).or_else(||codex.as_ref().and_then(|value| value["tokens"]["account_id"].as_str())),
            "model":connected.as_ref().and_then(|value| value["model"].as_str()).unwrap_or_else(||""),
            "hint":if connected.is_some() { "DOMINION에 ChatGPT 계정이 연결되어 있습니다. 사용 가능한 모델은 실제 계정 목록에서 조회합니다." } else if codex.is_some() { "Codex 앱 로그인이 감지되었습니다. 기존 인증 가져오기를 누르면 재로그인 없이 연결됩니다." } else { "브라우저에서 ChatGPT로 로그인하거나 Codex 앱의 기존 인증을 가져오세요." }
        },
        {
            "id":"claude",
            "name":"Claude Code (OAuth)",
            "connected":claude_connected,
            "model":state.store.get("provider","cli-claude").and_then(|p|p["model"].as_str().map(str::to_string)),
            "loginSupported":true,
            "loginMethod":"browser-cli",
            "available":claude,
            "supported":false,
            "hint":if claude { "Claude Code 인증 확인됨. DOMINION에서 계정 모델을 선택해 바로 대화할 수 있습니다." } else { "브라우저 로그인을 완료하면 모델이 DOMINION에 연결됩니다." }
        },
        {
            "id":"gemini",
            "name":"Gemini CLI (OAuth)",
            "connected":gemini_connected,
            "model":state.store.get("provider","cli-gemini").and_then(|p|p["model"].as_str().map(str::to_string)),
            "loginSupported":true,
            "loginMethod":"browser-cli",
            "available":gemini,
            "supported":false,
            "hint":if gemini { "Gemini CLI 로그인 확인됨. 기본 모델을 선택해 DOMINION에서 대화할 수 있습니다." } else { "브라우저 로그인 후 Gemini CLI 모델이 DOMINION에 연결됩니다." }
        },
        {
            "id":"opencode-go",
            "name":"OpenCode Go (API 키)",
            "loginSupported":false,
            "loginMethod":"api-key",
            "available":go_source.is_some() || go_connected,
            "importAvailable":go_source.is_some(),
            "supported":true,
            "connected":go_connected,
            "model":go_provider.as_ref().and_then(|p| p["model"].as_str()),
            "hint":if go_connected {
                "DOMINION에 키가 저장돼 있습니다. 실제 인증은 모델 조회로 검사하세요."
            } else if go_source.is_some() {
                "이 컴퓨터에 기존 OpenCode Go 키가 있습니다. 가져오면 DOMINION 금고에 별도 저장합니다."
            } else {
                "기존 로컬 키가 없습니다. OpenCode Go 콘솔에서 발급한 API 키를 직접 등록하세요."
            }
        },
        {
            "id":"opencode",
            "name":"OpenCode (OAuth/CLI)",
            "connected":opencode_connected,
            "model":state.store.get("provider","cli-opencode").and_then(|p|p["model"].as_str().map(str::to_string)),
            "loginSupported":true,
            "loginMethod":"browser-cli",
            "available":opencode,
            "importAvailable":opencode_importable > 0,
            "supported":opencode_importable > 0,
            "hint":if opencode { "OpenCode 인증을 확인했습니다. 사용 가능한 모델을 선택하면 DOMINION에서 실행됩니다.".to_string() } else if opencode_importable > 0 { format!("기존 OpenCode API 키 {opencode_importable}개를 가져올 수 있습니다.") } else { "브라우저 로그인 후 모델을 선택하세요.".to_string() }
        }
    ]))
}

pub async fn import_cli_credential(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let source = body["source"].as_str().unwrap_or("");
    let result = match source {
        "codex" => (|| {
            let auth = codex_auth()?;
            let saved = save_codex_provider(&state, &auth["tokens"], true)?;
            Ok(public_provider(&state, saved))
        })(),
        "opencode" => import_opencode(&state),
        "opencode-go" => import_opencode_go(&state),
        _ => Err("지원하지 않는 OAuth/CLI 소스입니다.".to_string()),
    };
    match result {
        Ok(saved) => Json(saved).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}
