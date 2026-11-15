use crate::{engine::codex_oauth, state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use super::response::error;

// Each provider family owns a focused implementation. The public API names
// remain stable; handlers still use the same AppState, vault and Rust router.
#[path = "providers/credentials.rs"]
mod credentials;
use credentials::*;
#[path = "providers/catalog.rs"]
mod catalog;
pub use catalog::{provider_catalog,refresh_free_catalog,free_catalog,free_catalog_refresh};
#[path = "providers/cli.rs"]
mod cli;
pub use cli::{set_cli_model,cli_credentials,import_cli_credential,
    codex_login_start,codex_device_start,codex_login_status};

fn needs_account_discovery(provider: &Value) -> bool {
    provider["accountId"].as_str().unwrap_or("").trim().is_empty()
        && provider["baseUrl"].as_str().is_some_and(|url| {
            url.contains("ACCOUNT_ID") || url.contains("{accountId}")
        })
}

/// Cloudflare lets a token list every account it can see, so the owner never
/// has to hunt for the account ID by hand. Any other provider with a
/// placeholder URL is left untouched.
async fn discover_cloudflare_account(api_key: &str) -> Option<String> {
    if api_key.is_empty() {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .ok()?;
    let response = client
        .get("https://api.cloudflare.com/client/v4/accounts")
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: Value = response.json().await.ok()?;
    value["result"].as_array()?.iter()
        .find_map(|account| account["id"].as_str().map(str::to_string))
}

async fn fill_missing_account(state: &AppState, provider: &Value) -> Option<String> {
    if !needs_account_discovery(provider) {
        return None;
    }
    let id = provider["id"].as_str()?;
    let key = state.read_secret(&format!("provider:{id}"))?;
    discover_cloudflare_account(&key).await
}

pub async fn list_providers(State(state): State<AppState>) -> Json<Value> {
    let mut items = Vec::new();
    for mut provider in state.store.all("provider") {
        // Lazily fill `ACCOUNT_ID` from the key the owner already saved, then
        // persist it so chat/model routes use the real URL.
        if let Some(account) = fill_missing_account(&state, &provider).await {
            provider["accountId"] = json!(account);
            if let Some(saved) = state.store.put("provider", &provider) {
                state.bus.record("provider", &saved);
            }
        }
        items.push(public_provider(&state, provider));
    }
    Json(json!(items))
}

fn public_provider(state: &AppState, mut provider: Value) -> Value {
    let id = provider
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    if let Value::Object(ref mut map) = provider {
        let reflection = map
            .get("reflection")
            .and_then(Value::as_bool)
            .or_else(|| map.get("learning").and_then(Value::as_bool))
            .unwrap_or(false);
        map.insert("reflection".into(), json!(reflection));
        map.remove("apiKey");
        map.insert(
            "hasKey".into(),
            json!(state.store.has_secret(&format!("provider:{id}"))),
        );
    }
    provider
}

pub async fn save_provider(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if let Err(message) = crate::engine::reasoning::validate(&body) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let name = body
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let base_url = body
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_string();
    let base_url = official_opencode_go_url(&base_url);
    if base_url.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "서버 주소를 입력하세요.",
        );
    }
    let parsed = match reqwest::Url::parse(&base_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return error(StatusCode::BAD_REQUEST, "HTTP(S) 주소만 허용됩니다."),
    };

    let name = if name.is_empty() { parsed.host_str().unwrap_or("API").to_owned() } else { name };

    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let kind = match body.get("kind").and_then(|value| value.as_str()) {
        Some("ollama") => "ollama",
        Some("anthropic") => "anthropic",
        Some("codex") => "codex",
        _ => "openai",
    };
    let existing = state.store.get("provider", &id);
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| existing.as_ref().and_then(|provider| provider["model"].as_str()))
        .unwrap_or("")
        .trim()
        .to_string();
    if model.len() > 256 {
        return error(StatusCode::BAD_REQUEST, "모델 ID가 너무 깁니다.");
    }
    // `reflection` is the public role. Older records and clients used
    // `learning`, so accept it as a compatibility alias.
    let reflection = body
        .get("reflection")
        .or_else(|| body.get("learning"))
        .and_then(|value| value.as_bool())
        .or_else(|| existing.as_ref().and_then(|provider| {
            provider["reflection"].as_bool().or_else(|| provider["learning"].as_bool())
        }))
        .unwrap_or(false);
    let review_model = body["reflectionModel"].as_str()
        .or_else(|| existing.as_ref().and_then(|provider| provider["reflectionModel"].as_str()))
        .unwrap_or(&model).trim();
    if reflection && (review_model.is_empty() || review_model.len() > 256) {
        return error(
            StatusCode::BAD_REQUEST,
            "실행 검토에 사용할 모델 ID를 입력하세요.",
        );
    }

    if body.get("accountId").and_then(Value::as_str).is_some_and(|value| value.len() > 128) {
        return error(StatusCode::BAD_REQUEST, "Account ID가 너무 깁니다.");
    }
    let mut record = body.clone();
    record["id"] = json!(id);
    record["name"] = json!(name);
    record["baseUrl"] = json!(parsed.to_string().trim_end_matches('/'));
    record["kind"] = json!(kind);
    record["reflection"] = json!(reflection);
    // Keep the legacy key in sync so existing readers keep working.
    record["learning"] = json!(reflection);
    if reflection {
        record["reflectionModel"] = json!(review_model);
    } else if let Some(fields) = record.as_object_mut() {
        fields.remove("reflectionModel");
    }
    if model.is_empty() {
        if let Value::Object(ref mut map) = record {
            map.remove("model");
        }
    } else {
        record["model"] = json!(model);
    }
    record["enabled"] = json!(body
        .get("enabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(true));
    record["toolMode"] = json!(
        match body.get("toolMode").and_then(|value| value.as_str()) {
            Some("native") => "native",
            Some("compat") => "compat",
            Some("auto") => "auto",
            _ =>
                if kind == "ollama" || kind == "codex" {
                    "compat"
                } else if kind == "anthropic" {
                    "native"
                } else {
                    "auto"
                },
        }
    );
    if let Value::Object(ref mut map) = record {
        map.remove("apiKey");
        map.remove("hasKey");
    }

    let mut fresh_key: Option<String> = None;
    if let Some(api_key) = body.get("apiKey").and_then(|value| value.as_str()) {
        if api_key.starts_with("••") {
            // Masked placeholder sent back by the UI: keep the stored secret.
        } else if api_key.is_empty() {
            state.store.remove_secret(&format!("provider:{id}"));
        } else {
            state.save_secret(&format!("provider:{id}"), api_key);
            fresh_key = Some(api_key.to_string());
        }
    }
    // Save the account ID immediately when the key can reveal it, so the next
    // model list or chat does not need a second trip to the provider.
    if needs_account_discovery(&record) {
        let key = fresh_key
            .clone()
            .or_else(|| state.read_secret(&format!("provider:{id}")))
            .unwrap_or_default();
        if let Some(account) = discover_cloudflare_account(&key).await {
            record["accountId"] = json!(account);
        }
    }

    let Some(saved) = state.store.put("provider", &record) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "API 연결을 저장할 수 없습니다.");
    };
    // Only one provider can own the reflection route. Selecting a new one
    // clears both the public and legacy flags on every other provider.
    if reflection {
        for mut other in state.store.all("provider") {
            if other["id"].as_str() != Some(id.as_str())
                && (other["reflection"].as_bool() == Some(true)
                    || other["learning"].as_bool() == Some(true))
            {
                other["reflection"] = json!(false);
                other["learning"] = json!(false);
                if let Some(saved) = state.store.put("provider", &other) {
                    state.bus.record("provider", &saved);
                }
            }
        }
    }
    state
        .store
        .audit("provider.saved", json!({ "providerId": saved.get("id") }));
    state.bus.record("provider", &saved);
    Json(public_provider(&state, saved)).into_response()
}

pub async fn delete_provider(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if id == "dominion-native" {
        return error(StatusCode::CONFLICT, "실행 중인 로컬 모델은 모델 관리에서 중지하세요.");
    }
    if state.store.get("provider", &id).is_none() {
        return error(StatusCode::NOT_FOUND, "API 연결을 찾을 수 없습니다.");
    }
    if !state.store.remove("provider", &id) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "API 연결을 해제할 수 없습니다.");
    }
    state.store.remove_secret(&format!("provider:{id}"));
    state.bus.emit(json!({ "type": "record", "kind": "provider", "removed": id }));
    Json(json!({ "ok": true })).into_response()
}

/// Returns (free?, source). `None` means the provider did not publish enough
/// information to decide; the UI must show "확인 필요", never a fake FREE.
fn model_free_status(
    _provider: &Value,
    kind: &str,
    base: &str,
    item: &Value,
    id: &str,
) -> (Option<bool>, &'static str) {
    // Local runtimes on their standard ports are free. Any other localhost
    // proxy may front a paid provider, so it is not automatically free.
    if kind == "ollama" || base.contains(":11434") || base.contains(":1234") {
        return (Some(true), "local-runtime");
    }
    if base.contains("z.ai") {
        return (
            Some(matches!(id, "glm-4.7-flash" | "glm-4.5-flash" | "glm-4.6v-flash")),
            "z.ai-free-list",
        );
    }
    if base.contains("oai.endpoints.kepler.ai.cloud.ovh.net") {
        return (Some(true), "ovh-free-endpoint");
    }
    // Groq's Free Plan has per-model rate limits. /models lists the models
    // accessible to the key, not a "free-only" catalog.
    // Source: https://console.groq.com/docs/rate-limits
    if base.contains("api.groq.com") {
        return (
            if matches!(
                id,
                "openai/gpt-oss-120b"
                    | "openai/gpt-oss-20b"
                    | "openai/gpt-oss-safeguard-20b"
                    | "qwen/qwen3.8-27b"
            ) {
                Some(true)
            } else {
                None
            },
            "groq-free-list",
        );
    }
    // Cloudflare Workers Free includes 10,000 Neurons per day shared across
    // supported models. The catalogue does not publish per-model free status,
    // so only models confirmed by the official pricing page are FREE; the
    // rest are "unknown", never a guessed FREE.
    if base.contains("api.cloudflare.com") && base.contains("/ai/") {
        if !id.starts_with("@cf/") {
            return (Some(false), "not-workers-ai");
        }
        if matches!(
            id,
            "@cf/moonshotai/kimi-k2.6"
                | "@cf/moonshotai/kimi-k2.7-code"
                | "@cf/zai-org/glm-5.2"
                | "@cf/zai-org/glm-5.3"
                | "@cf/zai-org/glm-5.3-flash"
                | "@cf/deepseek-ai/deepseek-v4-flash-0731"
                | "@cf/deepseek-ai/deepseek-v4-pro-0813"
        ) {
            return (Some(false), "cloudflare-paid-only");
        }
        if matches!(
            id,
            "@cf/zai-org/glm-4.7-flash"
                | "@cf/google/gemma-4-26b-a4b-it"
                | "@cf/nvidia/nemotron-3-120b-a12b"
                | "@cf/deepseek-ai/deepseek-v4.1-flash-1017"
        ) {
            return (Some(true), "cloudflare-confirmed-free");
        }
        return (None, "cloudflare-no-free-metadata");
    }
    if base.contains("127.0.0.1:31415") || base.contains("127.0.0.1:3001") {
        return (Some(true), "freellmapi-hub");
    }
    if item["free"].as_bool() == Some(true) || item["is_free"].as_bool() == Some(true) {
        return (Some(true), "provider-metadata");
    }
    if id.ends_with(":free") || id.contains(":free") {
        return (Some(true), "openrouter-free-suffix");
    }
    if let Some(pricing) = item.get("pricing") {
        let zero = |key: &str| {
            pricing
                .get(key)
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|text| text.parse::<f64>().ok())
                        .or_else(|| value.as_f64())
                })
                .is_some_and(|value| value == 0.0)
        };
        return (Some(zero("prompt") && zero("completion")), "provider-pricing");
    }
    (None, "no-pricing-metadata")
}

// Legacy helper is exercised by compatibility tests only; production uses
// the richer model_free_status function and its explicit evidence reason.
#[cfg(test)]
fn model_is_free(provider:&Value,kind:&str,base:&str,item:&Value,id:&str)->bool{
    model_free_status(provider,kind,base,item,id).0==Some(true)
}

/// Verifies a provider without saving anything. The UI can test the key the
/// owner just typed, so an invalid token is reported immediately instead of
/// silently failing later during a room conversation.
pub async fn test_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    // `new` (or any unsaved id) tests the draft in the dialog. Stored
    // providers are tested against their saved record plus the typed fields.
    let mut provider = state
        .store
        .get("provider", &id)
        .unwrap_or_else(|| json!({"id":id,"enabled":true}));
    for field in ["baseUrl", "accountId", "kind", "integration"] {
        if let Some(value) = body.get(field).and_then(Value::as_str) {
            provider[field] = json!(value);
        }
    }
    if provider["kind"]=="cli"{
        let Some(source)=provider["baseUrl"].as_str().and_then(|s|s.strip_prefix("cli://")) else{
            return Json(json!({"ok":false,"message":"CLI 주소 오류"})).into_response();
        };
        let valid=super::cli_oauth::credential_verified(source).await;
        return Json(json!({"ok":valid,"message":if valid{"CLI 계정 인증을 확인했습니다."}else{"CLI 로그인이 필요합니다."}})).into_response();
    }
    let key = body
        .get("apiKey")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.starts_with("••"))
        .map(str::to_string)
        .or_else(|| state.read_secret(&format!("provider:{id}")))
        .unwrap_or_default();
    if key.is_empty() {
        return Json(json!({"ok":false,"message":"API 키가 저장되어 있지 않습니다."})).into_response();
    }
    let key = key.trim();
    let base = crate::engine::gateway::resolved_base_url(&provider);
    if base.contains("ACCOUNT_ID") || base.contains("{accountId}") {
        if let Some(account) = discover_cloudflare_account(key).await {
            provider["accountId"] = json!(account);
        }
    }
    let base = crate::engine::gateway::resolved_base_url(&provider);
    let kind = provider["kind"].as_str().unwrap_or("openai");
    let key = if kind == "codex" {
        let client = reqwest::Client::new();
        match codex_oauth::fresh_access(&state, &client, &id, key).await {
            Ok(token) => token,
            Err(message) => return Json(json!({"ok":false,"message":message})).into_response(),
        }
    } else {
        key.to_string()
    };
    let model = body["model"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| provider["model"].as_str())
        .unwrap_or("")
        .to_string();
    let cloudflare = base.contains("/client/v4/accounts/") && base.contains("/ai/v1");
    let opencode_go = provider["integration"] == "opencode-go";
    let tested_model = if cloudflare && model.is_empty() {
        "@cf/zai-org/glm-4.7-flash".to_string()
    } else if kind == "codex" && model.is_empty() {
        codex_model()
    } else {
        model.clone()
    };
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(20)).build() {
        Ok(client) => client,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    if kind == "codex" {
        // Verify account credentials and model visibility without spending a
        // generation or calling unsupported non-streaming Codex responses.
        let account = provider["accountId"].as_str().unwrap_or("");
        match codex_models_live(&state, &client, &id, account, &key).await {
            Ok(models) => {
                let listed = models.iter().any(|entry|entry["id"].as_str()==Some(tested_model.as_str()));
                return Json(json!({
                    "ok":listed,
                    "status":200,
                    "model":tested_model,
                    "accountId":account,
                    "modelCount":models.len(),
                    "message":if listed {
                        format!("Codex OAuth 인증 성공. 계정에서 {}개 모델을 조회했고 선택된 모델도 목록에 있습니다.",models.len())
                    } else {
                        format!("Codex OAuth 인증 성공. 계정에서 {}개 모델을 조회했지만 설정된 모델 '{}'은 목록에 없습니다. 계정 모델을 다시 선택하세요.",models.len(),tested_model)
                    }
                })).into_response();
            }
            Err(message) => return Json(json!({
                "ok":false,
                "message":format!("Codex OAuth 모델 조회 실패: {message}"),
                "model":tested_model
            })).into_response(),
        }
    }
    let request = if kind == "codex" {
        let account = provider["accountId"].as_str().unwrap_or("");
        if account.is_empty() {
            return Json(json!({"ok":false,"message":"Codex 계정 ID가 없습니다. Codex를 다시 가져오세요."})).into_response();
        }
        client
            .post(format!("{base}/responses"))
            .header("Authorization", format!("Bearer {key}"))
            .header("chatgpt-account-id", account)
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "codex_cli_rs")
            .header("User-Agent", "codex_cli_rs/0.0.0")
            .json(&json!({
                "model":tested_model,
                "input":[{"role":"user","content":[{"type":"input_text","text":"ping"}]}],
                "stream":false,
                "store":false,
                "max_output_tokens":16
            }))
    } else if opencode_go {
        // No model generation is charged for this credential check.
        client.get("https://opencode.ai/inference/go/v1/models")
            .header("Authorization", format!("Bearer {key}"))
    } else if cloudflare {
        client
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {key}"))
            .json(&json!({
                "model":tested_model,
                "messages":[{"role":"user","content":"ping"}],
                "max_tokens":4,
                "stream":false
            }))
    } else if kind == "ollama" {
        client
            .get(format!("{}/models", base.trim_end_matches("/v1").trim_end_matches('/')))
            .header("Authorization", format!("Bearer {key}"))
    } else if kind == "openai" && !model.is_empty() {
        client
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {key}"))
            .json(&json!({
                "model":model,
                "messages":[{"role":"user","content":"ping"}],
                "max_tokens":4,
                "stream":false
            }))
    } else if kind == "anthropic" {
        client
            .get(format!("{base}/models"))
            .header("x-api-key", key.as_str())
            .header("anthropic-version", "2023-06-01")
    } else {
        client
            .get(format!("{base}/models"))
            .header("Authorization", format!("Bearer {key}"))
    };
    match request.send().await {
        Ok(response) if response.status().is_success() => {
            Json(json!({
                "ok":true,
                "status":response.status().as_u16(),
                "accountId":provider["accountId"],
                "model":tested_model
            })).into_response()
        }
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let detail = text.chars().take(300).collect::<String>();
            let overloaded = detail.contains("1305")
                || detail.to_ascii_lowercase().contains("temporarily overloaded");
            let cloudflare_key_shape = cloudflare && key.len() != 40 && !key.starts_with("cfk_");
            let message = if cloudflare_key_shape {
                "Cloudflare API 토큰 형식이 아닙니다. 40자 토큰 또는 cfk_ 토큰을 붙여넣으세요."
            } else if overloaded {
                "제공자가 혼잡합니다(1305). 다른 무료 모델을 선택하거나 잠시 후 다시 시도하세요."
            } else {
                match status.as_u16() {
                    401 | 403 => "키가 거부되었습니다. API 토큰 권한과 계정을 확인하세요.",
                    404 | 405 => "주소는 응답했지만 이 경로를 지원하지 않습니다. 서버 주소를 확인하세요.",
                    429 => "호출 한도에 걸렸습니다. 잠시 후 다시 시도하세요.",
                    _ => "연결 테스트에 실패했습니다.",
                }
            };
            Json(json!({
                "ok":false,
                "status":status.as_u16(),
                "keyLength":key.chars().count(),
                "model":tested_model,
                "message":message,
                "detail":detail
            })).into_response()
        }
        Err(err) => Json(json!({"ok":false,"message":format!("연결할 수 없습니다: {err}")})).into_response(),
    }
}

/// Decode the account-specific Codex catalog, never manufacture model IDs.
fn parse_codex_models(value: &Value) -> Result<Vec<Value>, String> {
    let rows = value["models"].as_array()
        .or_else(||value["data"].as_array())
        .ok_or("Codex 응답에 models 배열이 없습니다.")?;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();
    for row in rows {
        if row["hidden"] == true
            || row["show_in_picker"] == false
            || matches!(row["visibility"].as_str(), Some("hide"|"hidden"|"none"))
        { continue; }
        let Some(id) = row["slug"].as_str()
            .or_else(||row["model"].as_str())
            .or_else(||row["id"].as_str())
            .filter(|id|!id.is_empty()) else { continue };
        if !seen.insert(id.to_string()) { continue; }
        let efforts = row["supported_reasoning_levels"].as_array()
            .or_else(||row["supported_reasoning_efforts"].as_array())
            .or_else(||row["supportedReasoningEfforts"].as_array())
            .cloned().unwrap_or_default();
        models.push(json!({
            "id":id,
            "name":row["display_name"].as_str()
                .or_else(||row["displayName"].as_str()).unwrap_or(id),
            "free":null,
            "freeStatus":"unknown",
            "freeSource":"chatgpt-subscription",
            "reasoningEfforts":efforts.iter().filter_map(|value|
                value.as_str().or_else(||value["effort"].as_str()).or_else(||value["reasoningEffort"].as_str())
            ).collect::<Vec<_>>(),
            "defaultReasoningEffort":row["default_reasoning_level"].as_str()
                .or_else(||row["default_reasoning_effort"].as_str())
                .or_else(||row["defaultReasoningEffort"].as_str()),
            "isDefault":row["is_default"].as_bool()
                .or_else(||row["isDefault"].as_bool()).unwrap_or(false),
        }));
    }
    if models.is_empty() {
        return Err("Codex 계정에서 선택 가능한 모델이 반환되지 않았습니다.".into());
    }
    Ok(models)
}

/// The same authenticated /models endpoint used by Codex's model manager.
/// Client version comes from the installed CLI or its cache, not a fake list.
async fn codex_models_live(
    state: &AppState, client: &reqwest::Client,
    provider_id: &str, account: &str, saved_token: &str,
) -> Result<Vec<Value>, String> {
    if account.is_empty() { return Err("Codex 계정 ID가 없습니다.".into()); }
    let access = codex_oauth::fresh_access(state, client, provider_id, saved_token).await?;
    if access.is_empty() { return Err("Codex OAuth 연결 토큰이 없습니다.".into()); }
    // Codex requires client_version. An absent query gives HTTP 400; using
    // only a stale installed CLI's version hides newer account models.
    // Query the full account catalog first with a generic compatibility
    // version, then retry with the installed CLI's version.
    let mut versions = vec!["99.0.0".to_string()];
    let cli_version = tokio::time::timeout(Duration::from_secs(3),
        tokio::process::Command::new("codex").arg("--version").output()
    ).await.ok().and_then(Result::ok)
        .and_then(|output|String::from_utf8(output.stdout).ok())
        .and_then(|stdout|stdout.split_whitespace()
            .find(|part|part.chars().next().is_some_and(|c|c.is_ascii_digit()))
            .map(str::to_string));
    let cache_version = codex_home_file("models_cache.json")
        .and_then(|path|std::fs::read_to_string(path).ok())
        .and_then(|text|serde_json::from_str::<Value>(&text).ok())
        .and_then(|value|value["client_version"].as_str().map(str::to_string));
    if let Some(version) = cli_version.or(cache_version) {
        if !versions.contains(&version) { versions.push(version); }
    }
    versions.push("0.0.0".to_string());
    let mut failure = String::from("Codex 계정 모델을 조회하지 못했습니다.");
    for version in versions {
        let mut url = reqwest::Url::parse("https://chatgpt.com/backend-api/codex/models")
            .map_err(|e|e.to_string())?;
        url.query_pairs_mut().append_pair("client_version", &version);
        let response = match client.get(url)
            .bearer_auth(&access)
            .header("chatgpt-account-id",account)
            .header("originator","codex_cli_rs")
            .header("Accept","application/json")
            .send().await {
                Ok(response) => response,
                Err(e) => {failure = format!("Codex 모델 조회 연결 실패: {e}");break;}
            };
        if matches!(response.status().as_u16(),401|403) {
            return Err(format!("Codex 계정 인증이 거부되었습니다 (HTTP {}). 재로그인하세요.",response.status()));
        }
        if !response.status().is_success() {
            failure = format!("Codex 모델 조회 HTTP {} (client_version={version})",response.status());
            continue;
        }
        let data:Value = response.json().await
            .map_err(|_|"Codex 모델 응답 JSON 오류")?;
        match parse_codex_models(&data) {
            Ok(models) => return Ok(models),
            Err(message) => failure = message,
        }
    }
    Err(failure)
}

pub async fn provider_models(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(mut provider) = state.store.get("provider", &id) else {
        return error(StatusCode::NOT_FOUND, "제공자를 찾을 수 없습니다.");
    };
    if provider["kind"]=="cli"{
        let Some(source)=provider["baseUrl"].as_str().and_then(|s|s.strip_prefix("cli://")) else{
            return error(StatusCode::BAD_REQUEST,"CLI 제공자 주소 오류");
        };
        return match crate::engine::cli_model::models(source).await{
            Ok(models)=>Json(json!(models)).into_response(),
            Err(message)=>error(StatusCode::BAD_GATEWAY,&message)
        };
    }
    let key = state.read_secret(&format!("provider:{id}")).unwrap_or_default();
    if needs_account_discovery(&provider) {
        if let Some(account) = discover_cloudflare_account(&key).await {
            provider["accountId"] = json!(account);
            if let Some(saved) = state.store.put("provider", &provider) {
                state.bus.record("provider", &saved);
            }
        }
    }
    let base = crate::engine::gateway::resolved_base_url(&provider);
    let kind = provider.get("kind").and_then(|value| value.as_str()).unwrap_or("openai");
    if kind == "codex" {
        // The owner's actual Codex catalog replaces the historical four-name stub.
        // Subscription models are not metered as ordinary API-key purchases.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let account = provider["accountId"].as_str().unwrap_or("");
        let cached = state.store.get("codex_model_catalog", &id)
            .filter(|row| row["accountId"].as_str() == Some(account));
        match codex_models_live(&state, &client, &id, account, &key).await {
            Ok(models) => {
                state.store.put("codex_model_catalog", &json!({
                    "id":id,"accountId":account,"models":models,
                    "fetchedAt":chrono::Utc::now().to_rfc3339()
                }));
                return Json(json!(models)).into_response();
            }
            Err(reason) => {
                if let Some(models) = cached.as_ref().and_then(|row| row["models"].as_array()) {
                    return Json(json!(models.iter().map(|model| {
                        let mut item = model.clone();
                        item["catalogStale"] = json!(true);
                        item
                    }).collect::<Vec<_>>())).into_response();
                }
                // A matching Codex CLI account may already have a local catalog.
                // Never expose the models of another logged-in account.
                if codex_auth().ok().and_then(|auth|
                    auth["tokens"]["account_id"].as_str().map(str::to_string)
                ).as_deref() == Some(account) {
                    if let Some(path) = codex_home_file("models_cache.json") {
                        if let Ok(contents) = std::fs::read_to_string(path) {
                            if let Ok(value) = serde_json::from_str::<Value>(&contents) {
                                if let Ok(mut rows) = parse_codex_models(&value) {
                                    for row in &mut rows {
                                        row["catalogStale"] = json!(true);
                                    }
                                    return Json(json!(rows)).into_response();
                                }
                            }
                        }
                    }
                }
                return error(StatusCode::BAD_GATEWAY,
                    &format!("Codex 계정 모델 목록을 조회하지 못했습니다: {reason}. 계정 재연결 또는 Codex CLI 모델 동기화를 확인하세요."));
            }
        }
    }
    // Cloudflare's OpenAI-compatible surface has no /models route. Its native
    // model search returns the catalogue instead.
    let cloudflare = base.contains("/client/v4/accounts/") && base.contains("/ai/v1");
    let url = if provider["integration"] == "opencode-go" {
        "https://opencode.ai/inference/go/v1/models".to_string()
    } else if kind == "ollama" {
        let native = base.trim_end_matches("/v1").trim_end_matches('/');
        format!("{native}/api/tags")
    } else if cloudflare {
        format!(
            "{}/ai/models/search?per_page=100",
            base.trim_end_matches("/ai/v1").trim_end_matches('/')
        )
    } else if kind == "anthropic" {
        format!("{base}/models")
    } else {
        format!(
            "{base}/models{}",
            if provider["integration"] == "freellmapi" {
                "?available=true"
            } else {
                ""
            }
        )
    };

    let client = reqwest::Client::new();
    let mut request = client.get(&url).timeout(Duration::from_secs(12));
    if kind == "anthropic" {
        request = request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
    } else if !key.is_empty() {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    match request.send().await {
        Ok(response) if response.status().is_success() => {
            let Ok(value) = response.json::<Value>().await else {
                return error(StatusCode::BAD_GATEWAY, "서버가 모델 목록 JSON을 반환하지 않았습니다. 서버 주소를 확인하세요.");
            };
            let key_name = if kind == "ollama" {
                "models"
            } else if cloudflare {
                "result"
            } else {
                "data"
            };
            let Some(items) = value[key_name].as_array()
                .or_else(|| if provider["integration"] == "opencode-go" { value["models"].as_array() } else { None }) else {
                return error(StatusCode::BAD_GATEWAY, "모델 목록 형식을 확인할 수 없습니다. 서버 주소와 API 방식을 확인하세요.");
            };
            let mut models: Vec<Value> = items.iter().filter_map(|item| {
                let id = if kind == "ollama" || cloudflare {
                    item["name"].as_str().or_else(|| item["id"].as_str())?
                } else {
                    item["id"].as_str()?
                };
                let (free, source) = model_free_status(&provider, kind, &base, item, id);
                Some(json!({
                    "id": id,
                    "name": item["display_name"].as_str().unwrap_or(id),
                    "free": free,
                    "freeStatus": match free {
                        Some(true) => "free",
                        Some(false) => "paid",
                        None => "unknown",
                    },
                    "freeSource": source,
                }))
            }).collect();
            // Z.ai's model endpoint omits the free flash models even though
            // they are callable. Add them so the picker can show free options.
            if base.contains("z.ai") {
                for id in ["glm-4.7-flash", "glm-4.5-flash", "glm-4.6v-flash"] {
                    if let Some(row) = models.iter_mut().find(|row| row["id"] == id) {
                        row["free"] = json!(true);
                        row["freeStatus"] = json!("free");
                        row["freeSource"] = json!("z.ai-free-list");
                    } else {
                        models.push(json!({
                            "id":id,"name":id,"free":true,
                            "freeStatus":"free","freeSource":"z.ai-free-list"
                        }));
                    }
                }
            }
            Json(json!(models)).into_response()
        }
        Ok(response) => {
            let status = response.status();
            let hint = match status.as_u16() {
                401 | 403 => "API 키와 모델 조회 권한을 확인하세요.",
                404 | 405 => "서버 주소를 확인하세요. 모델 목록을 제공하지 않는 API는 모델 ID를 직접 입력할 수 있습니다.",
                _ => "잠시 후 다시 조회하거나 API 서비스 상태를 확인하세요.",
            };
            error(StatusCode::BAD_GATEWAY, &format!("모델 목록 조회 실패 (HTTP {status}). {hint}"))
        }
        Err(err) => error(StatusCode::BAD_GATEWAY, &err.to_string()),
    }
}

#[cfg(test)]
#[path = "providers/tests.rs"]
mod tests;

