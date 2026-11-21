//! H3 ModelGateway: one route, one admission policy and one usage record for
//! every model call in the engine.
//!
//! Callers keep their own prompt/response handling (chat streaming, planner,
//! compactor, learner), but they share provider resolution, capability
//! ordering, local concurrency admission, auth, timeout/retry and usage
//! recording here. This is deliberately a thin gateway: it does not rebuild the
//! existing execution loop.
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Explicit per-model/per-endpoint feature profile. Unknown is not "false",
/// and a provider declaration never becomes proof of runtime behavior.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub tool_calling:Option<bool>,
    pub parallel_tool_calls:Option<bool>,
    pub reasoning:Option<bool>,
    pub prompt_cache:Option<bool>,
    pub vision:Option<bool>,
    pub streaming:Option<bool>,
    pub context_window:Option<u64>,
}
impl ModelCapabilities {
    fn field(model:&Value,provider:&Value,names:&[&str])->Option<bool>{
        for key in names {
            if let Some(value)=model.get(*key).and_then(Value::as_bool){return Some(value)}
            if let Some(value)=provider.get(*key).and_then(Value::as_bool){return Some(value)}
        }
        None
    }
    pub fn from_provider(provider:&Value,model:&str,tool_mode:&str)->Self {
        let base=&provider["capabilities"];
        let model_profile=&provider["modelCapabilities"][model];
        let mut value=Self{
            tool_calling:Self::field(model_profile,base,&["toolCalling","tools"]),
            parallel_tool_calls:Self::field(model_profile,base,&["parallelToolCalls"]),
            reasoning:Self::field(model_profile,base,&["reasoningSupported","thinking"]),
            prompt_cache:Self::field(model_profile,base,&["promptCache"]),
            vision:Self::field(model_profile,base,&["vision"]),
            streaming:Self::field(model_profile,base,&["streaming"]),
            context_window:model_profile["contextTokens"].as_u64()
                .or_else(||base["contextTokens"].as_u64()),
        };
        // The configured wire adapter cannot deliver native tool calls in
        // compatibility mode, regardless of a model marketing capability.
        if tool_mode=="compat" {value.tool_calling=Some(false);}
        value
    }
    /// Unknown is allowed only for legacy native-capable endpoints; an
    /// explicit negative is a hard rejection before any billable request.
    /// Streaming UI must never dispatch an explicitly non-streaming endpoint.
    /// Unknown capabilities retain compatibility with unprobed local servers.
    pub fn require_streaming(&self)->Result<(),String>{
        if self.streaming==Some(false){
            Err("선택한 모델·엔드포인트는 스트리밍 응답을 지원하지 않습니다.".into())
        }else{Ok(())}
    }
    pub fn require_native_tools(&self)->Result<(),String>{
        if self.tool_calling==Some(false){
            Err("이 모델·엔드포인트 조합은 네이티브 도구 호출을 지원하지 않습니다.".into())
        }else{Ok(())}
    }
}

/// A resolved provider + model pair. \`base_url\` never has a trailing slash.
#[derive(Clone, Debug)]
pub struct Route {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub kind: String,
    pub tool_mode: String,
    pub reasoning_protocol: String,
    pub local: bool,
    /// True when this route must disable provider-side thinking for helper
    /// calls (declared capability first, local legacy default second).
    pub disable_thinking: bool,
    pub context_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub requests_per_minute: Option<u64>,
    pub account_id: Option<String>,
    pub capabilities: ModelCapabilities,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

impl Route {
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }
}

/// A provider that runs on this machine. It is eligible for any room or chat
/// route; the gateway uses this flag for local concurrency admission.
pub fn is_local_provider(provider: &Value) -> bool {
    let base = provider["baseUrl"].as_str().unwrap_or("");
    provider["local"].as_bool() == Some(true)
        || base.contains("127.0.0.1")
        || base.contains("localhost")
}

/// A provider may explicitly declare `capabilities.text=false`; absence means
/// unknown, not unqualified.
pub fn provider_eligible(provider: &Value) -> bool {
    provider["enabled"].as_bool() != Some(false)
        && provider["capabilities"]["text"].as_bool() != Some(false)
}

/// A declared capability profile always outranks the legacy model-name digit
/// heuristic. Declared scores live in 1..2, heuristic scores in 0..1.
pub fn capability_score(provider: &Value) -> f64 {
    let capabilities = &provider["capabilities"];
    if let Some(quality) = capabilities["quality"].as_f64() {
        return 1.0 + quality.clamp(0.0, 1.0);
    }
    if let Some(rank) = capabilities["reasoning"].as_f64() {
        return 1.0 + rank.clamp(0.0, 1.0);
    }
    let rank = model_rank(provider["model"].as_str().unwrap_or(""));
    1.0 - 1.0 / (1.0 + rank)
}

fn model_rank(model: &str) -> f64 {
    model
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .filter_map(|part| part.parse::<f64>().ok())
        .fold(0.0, f64::max)
}

/// Capability profile first, legacy local heuristic second.
pub fn disable_thinking(provider: &Value, local: bool) -> bool {
    match provider["capabilities"]["thinking"].as_bool() {
        Some(supported) => !supported,
        None => local,
    }
}

fn default_tool_mode(provider: &Value, kind: &str) -> String {
    provider["toolMode"]
        .as_str()
        .filter(|value| matches!(*value, "native" | "compat" | "auto"))
        .map(str::to_string)
        .unwrap_or_else(|| {
            if kind == "anthropic" {
                "native".into()
            } else if kind == "ollama" || provider["local"].as_bool() == Some(true) {
                "compat".into()
            } else {
                "auto".into()
            }
        })
}

fn default_reasoning_protocol(provider: &Value, kind: &str, model: &str, id: &str) -> String {
    provider["reasoningProtocol"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            if kind == "anthropic" {
                "anthropic".into()
            } else if model.to_ascii_lowercase().contains("deepseek") {
                "deepseek".into()
            } else if kind == "ollama" || id == "dominion-native" {
                "chat_template".into()
            } else {
                "openai".into()
            }
        })
}

/// Resolves placeholders in a provider URL. Cloudflare (and similar APIs)
/// put the account in the path, so the owner supplies `accountId` once and
/// every route/model call fills it in.
pub fn resolved_base_url(provider: &Value) -> String {
    let raw = provider["baseUrl"].as_str().unwrap_or("");
    let account = provider["accountId"].as_str().unwrap_or("").trim();
    let resolved = if account.is_empty() {
        raw.trim_end_matches('/').to_string()
    } else {
        raw.replace("ACCOUNT_ID", account)
            .replace("{accountId}", account)
            .trim_end_matches('/')
            .to_string()
    };
    // OpenCode Go publishes its OpenAI-compatible base under
    // /inference/go/openai/v1. Older records still carry /zen/go/v1, so
    // every route resolves to the documented URL.
    if provider["integration"].as_str() == Some("opencode-go") {
        if resolved.starts_with("https://opencode.ai/zen/go")
            || resolved == "https://opencode.ai/inference/go"
            || resolved == "https://opencode.ai/inference/go/v1"
        {
            return "https://opencode.ai/inference/go/openai/v1".to_string();
        }
    }
    resolved
}

/// Builds a route from a provider record. Disabled providers and providers
/// without a base URL are rejected here for every caller.
pub fn route_from_provider(
    state: &AppState,
    provider: &Value,
    model_override: Option<&str>,
) -> Result<Route, String> {
    if provider["enabled"].as_bool() == Some(false) {
        return Err("선택한 제공자가 비활성 상태입니다.".into());
    }
    if provider["capabilities"]["text"].as_bool() == Some(false) {
        return Err("이 제공자는 텍스트 생성을 지원하지 않는다고 선언했습니다.".into());
    }
    let id = provider["id"].as_str().unwrap_or_default().to_string();
    if id.is_empty() {
        return Err("제공자 id 가 없습니다.".into());
    }
    let base_url = resolved_base_url(provider);
    if base_url.is_empty() {
        return Err("제공자에 baseUrl 이 없습니다.".into());
    }
    let model = model_override
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            provider["model"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .ok_or("이 API에서 사용할 모델을 선택하거나 모델 ID를 입력하세요.")?;
    let kind = provider["kind"]
        .as_str()
        .unwrap_or("openai")
        .to_string();
    let api_key = state
        .read_secret(&format!("provider:{id}"))
        .or_else(|| {
            provider["apiKey"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_default();
    let local = is_local_provider(provider);
    // Model + endpoint overrides provider-wide marketing context limits.
    let context_tokens=provider["modelCapabilities"][model.as_str()]["contextTokens"]
        .as_u64()
        .or_else(||provider["capabilities"]["contextTokens"].as_u64())
        .or_else(||provider["contextTokens"].as_u64());
    let max_output_tokens = provider["modelCapabilities"][model.as_str()]["maxOutputTokens"]
        .as_u64()
        .or_else(||provider["capabilities"]["maxOutputTokens"].as_u64())
        .or_else(|| provider["maxOutputTokens"].as_u64());
    let tool_mode=default_tool_mode(provider,&kind);
    let capabilities=ModelCapabilities::from_provider(provider,&model,&tool_mode);
    let disable_thinking=capabilities.reasoning
        .map(|supported|!supported)
        .unwrap_or_else(||disable_thinking(provider,local));
    Ok(Route {
        capabilities,
        name: provider["name"]
            .as_str()
            .unwrap_or("Provider")
            .to_string(),
        tool_mode,
        reasoning_protocol: default_reasoning_protocol(provider, &kind, &model, &id),
        local,
        disable_thinking,
        context_tokens,
        max_output_tokens,
        requests_per_minute: provider["requestsPerMinute"]
            .as_u64()
            .filter(|value| *value > 0),
        account_id: provider["accountId"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        id,
        base_url,
        api_key,
        model,
        kind,
    })
}

/// Resolve an owner-pinned provider, or a stable enabled default when no
/// provider was requested. Database updated_at has only second-level precision
/// and must not silently change the selected model across platforms.
pub fn resolve(
    state: &AppState,
    provider_id: &str,
    model_override: Option<&str>,
) -> Result<Route, String> {
    let provider = if provider_id.is_empty() {
        let mut providers = state.store.all("provider").into_iter()
            .filter(provider_eligible).collect::<Vec<_>>();
        // Respect explicitly marked defaults, then select deterministically.
        // Editing a provider or SQLite's tie order must not silently reroute
        // an unpinned model call to a different (possibly paid) account.
        let marked = |provider: &Value| {
            provider["isDefault"].as_bool() == Some(true)
                || provider["default"].as_bool() == Some(true)
        };
        providers.sort_by(|a,b| marked(b).cmp(&marked(a))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str())));
        providers.into_iter().next()
    } else {
        state.store.get("provider", provider_id)
    }
    .ok_or("제공자를 찾을 수 없습니다.")?;
    route_from_provider(state, &provider, model_override)
}

/// Capability-ordered alternatives for helper calls. The current provider is
/// tried first by the caller; this list deliberately excludes it.
pub fn ordered_alternatives(state: &AppState, exclude_id: &str) -> Vec<Route> {
    let mut candidates = state
        .store
        .all("provider")
        .into_iter()
        .filter(provider_eligible)
        .filter(|provider| provider["id"].as_str() != Some(exclude_id))
        .filter_map(|provider| {
            let route = route_from_provider(state, &provider, None).ok()?;
            let learning = provider["learning"].as_bool() == Some(true);
            Some((route, capability_score(&provider), learning))
        })
        .collect::<Vec<_>>();
    let tight_cap = |route: &Route| {
        route
            .requests_per_minute
            .is_some_and(|limit| limit > 0 && limit <= 2)
    };
    candidates.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| tight_cap(&a.0).cmp(&tight_cap(&b.0)))
            .then_with(|| b.0.local.cmp(&a.0.local))
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    candidates.into_iter().map(|(route, _, _)| route).collect()
}

// ------------------------------------------------------------------ admission

fn semaphores() -> &'static Mutex<HashMap<String, Arc<Semaphore>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Semaphore>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Local model servers usually serve one generation at a time; cloud providers
/// get a small bound so a burst cannot exhaust sockets. The permit is held for
/// the whole response body, including SSE streams.
#[derive(Default)]
struct LimitWindow {
    stamps: std::collections::VecDeque<std::time::Instant>,
    blocked_until: Option<std::time::Instant>,
    /// Set when a route proved unusably slow, so the room rotation stops
    /// picking it for a while instead of paying the latency every round.
    slow_until: Option<std::time::Instant>,
}

fn rate_limits() -> &'static Mutex<HashMap<String, LimitWindow>> {
    static LIMITS: OnceLock<Mutex<HashMap<String, LimitWindow>>> = OnceLock::new();
    LIMITS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Reserve one generation slot for routes with a documented anonymous limit.
/// OVHcloud allows two requests per minute per model and IP. A 429 from any
/// provider also sets a short cooldown so a retry loop cannot hammer it.
pub fn reserve_generation(
    base_url: &str,
    model: &str,
    requests_per_minute: Option<u64>,
) -> Result<(), String> {
    let key = format!("{base_url}|{model}");
    let now = std::time::Instant::now();
    let mut limits = rate_limits().lock().unwrap();
    let window = limits.entry(key).or_default();
    if window.blocked_until.is_some_and(|until| until > now) {
        return Err("무료 제공자 호출 제한에 걸렸습니다. 잠시 후 다시 시도하거나 다른 모델을 사용하세요.".into());
    }
    if window.slow_until.is_some_and(|until| until > now) {
        return Err("최근 응답이 너무 느렸던 모델이라 잠시 건너뜁니다.".into());
    }
    // No built-in per-host limit: the provider record owns the value so any
    // free or metered service can be configured without a code change.
    let Some(limit) = requests_per_minute.filter(|value| *value > 0) else {
        return Ok(());
    };
    while window
        .stamps
        .front()
        .is_some_and(|stamp| now.duration_since(*stamp).as_secs() >= 60)
    {
        window.stamps.pop_front();
    }
    if window.stamps.len() as u64 >= limit {
        return Err("무료 제공자 호출 한도(분당 2회)에 도달했습니다. 잠시 후 다시 시도하거나 다른 모델을 사용하세요.".into());
    }
    window.stamps.push_back(now);
    Ok(())
}

/// Remembers that a route took too long, so rotation and failover skip it.
pub fn mark_slow(base_url: &str, model: &str) {
    let key = format!("{base_url}|{model}");
    let until = std::time::Instant::now() + Duration::from_secs(900);
    if let Ok(mut limits) = rate_limits().lock() {
        limits.entry(key).or_default().slow_until = Some(until);
    }
}

pub fn mark_rate_limited(base_url: &str, model: &str) {
    let key = format!("{base_url}|{model}");
    let until = std::time::Instant::now() + Duration::from_secs(60);
    if let Ok(mut limits) = rate_limits().lock() {
        limits.entry(key).or_default().blocked_until = Some(until);
    }
}

pub async fn acquire(base_url: &str, local: bool) -> OwnedSemaphorePermit {
    let capacity = if local { 1 } else { 4 };
    let semaphore = {
        let mut registry = semaphores().lock().unwrap();
        registry
            .entry(base_url.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(capacity)))
            .clone()
    };
    semaphore
        .acquire_owned()
        .await
        .expect("model admission semaphore closed")
}

// -------------------------------------------------------------------- transport

/// A response plus the admission permit that must stay alive while the body is
/// consumed. Dropping this struct releases the model slot.
pub struct GatewayResponse {
    pub response: reqwest::Response,
    /// Held until the body is consumed; the name marks it as a guard.
    pub _permit: OwnedSemaphorePermit,
}

pub fn benchmark_request(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    if benchmark_expected_route().is_some() {
        request.header("X-FreeLLM-Cache", "off").header("X-FreeLLM-Compress", "off")
    } else {
        request
    }
}

fn benchmark_expected_route() -> Option<String> {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return None;
    }
    std::env::var("DOMINION_BENCHMARK_EXPECTED_ROUTE").ok().filter(|route| !route.is_empty())
}

pub fn verify_benchmark_route(response: &reqwest::Response) -> Result<(), String> {
    if let Some(expected) = benchmark_expected_route() {
        if response.status().is_success()
            && response.headers().get("X-Routed-Via").and_then(|value| value.to_str().ok())
                != Some(expected.as_str())
        {
            return Err("Benchmark response did not come from the pinned upstream route".into());
        }
    }
    Ok(())
}

/// Only one provider dispatch for a durable native agent model call.
/// A POST disconnected before its first delta may still have incurred tokens.
/// The Rust kernel handles model_in_flight; never retry here.
pub async fn send_once(
    route:&Route,client:&reqwest::Client,body:&Value,timeout:Duration,
)->Result<GatewayResponse,String> {
    reserve_generation(&route.base_url,&route.model,route.requests_per_minute)?;
    let permit=acquire(&route.base_url,route.local).await;
    let response=benchmark_request(client.post(route.endpoint("/chat/completions")))
        .header("Content-Type","application/json")
        .header("Authorization",format!("Bearer {}",route.api_key))
        .json(body)
        .timeout(timeout)
        .send().await.map_err(|error|format!(
            "{} native model delivery may be uncertain (not replayed): {error}",
            route.name
        ))?;
    if response.status()==reqwest::StatusCode::TOO_MANY_REQUESTS {
        mark_rate_limited(&route.base_url,&route.model);
    }
    verify_benchmark_route(&response)?;
    Ok(GatewayResponse{response,_permit:permit})
}

/// Sends a helper completion through shared admission and authentication.
pub async fn send(
    route: &Route,
    client: &reqwest::Client,
    body: &Value,
    timeout: Duration,
) -> Result<GatewayResponse, String> {
    reserve_generation(
        &route.base_url,
        &route.model,
        route.requests_per_minute,
    )?;
    let permit = acquire(&route.base_url, route.local).await;
    let mut last_error = String::new();
    for attempt in 0..2 {
        match benchmark_request(client.post(route.endpoint("/chat/completions")))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", route.api_key))
            .json(body)
            .timeout(timeout)
            .send()
            .await
        {
            Ok(response) => {
                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    mark_rate_limited(&route.base_url, &route.model);
                }
                verify_benchmark_route(&response)?;
                return Ok(GatewayResponse { response, _permit: permit });
            }
            Err(error) => {
                last_error = error.to_string();
                if attempt == 0 {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                }
            }
        }
    }
    Err(format!("{} 연결 실패: {last_error}", route.name))
}

pub fn usage_from(value: &Value) -> Usage {
    let usage = &value["usage"];
    Usage {
        prompt_tokens: usage["prompt_tokens"]
            .as_u64()
            .or_else(|| usage["promptTokens"].as_u64())
            .unwrap_or(0),
        completion_tokens: usage["completion_tokens"]
            .as_u64()
            .or_else(|| usage["completionTokens"].as_u64())
            .unwrap_or(0),
        total_tokens: usage["total_tokens"]
            .as_u64()
            .or_else(|| usage["totalTokens"].as_u64())
            .unwrap_or(0),
    }
}

/// Records per-call usage with enough identity to attribute cost later. Audit
/// records are append-only and do not mutate provider configuration.
pub fn record_usage(
    state: &AppState,
    route: &Route,
    usage: Usage,
    purpose: &str,
    session_id: Option<&str>,
) {
    if usage.total_tokens == 0 && usage.prompt_tokens == 0 && usage.completion_tokens == 0 {
        return;
    }
    state.store.audit(
        "model.usage",
        json!({
            "providerId": route.id,
            "model": route.model,
            "kind": route.kind,
            "toolMode": route.tool_mode,
            "reasoningProtocol": route.reasoning_protocol,
            "local": route.local,
            "purpose": purpose,
            "sessionId": session_id,
            "promptTokens": usage.prompt_tokens,
            "completionTokens": usage.completion_tokens,
            "totalTokens": usage.total_tokens,
        }),
    );
}

/// Usage for a streaming call where the breakdown may be estimated.
#[allow(clippy::too_many_arguments)]
pub fn record_usage_tokens(
    state: &AppState,
    provider_id: &str,
    model: &str,
    local: bool,
    purpose: &str,
    session_id: Option<&str>,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    estimated: bool,
) {
    record_usage_tokens_with_cache(
        state, provider_id, model, local, purpose, session_id,
        prompt_tokens, completion_tokens, total_tokens, estimated, None,
    );
}

/// Durable per-call cache accounting for the session total shown in Chat.
/// A missing provider cache field remains unknown, never a fabricated miss.
#[allow(clippy::too_many_arguments)]
pub fn record_usage_tokens_with_cache(
    state: &AppState,
    provider_id: &str,
    model: &str,
    local: bool,
    purpose: &str,
    session_id: Option<&str>,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    estimated: bool,
    cached_prompt_tokens: Option<u64>,
) {
    if total_tokens == 0 {
        return;
    }
    let cached = if estimated { None } else {
        cached_prompt_tokens.map(|tokens|tokens.min(prompt_tokens))
    };
    state.store.audit(
        "model.usage",
        json!({
            "providerId": provider_id,
            "model": model,
            "local": local,
            "purpose": purpose,
            "sessionId": session_id,
            "promptTokens": prompt_tokens,
            "cachedPromptTokens": cached,
            "completionTokens": completion_tokens,
            "totalTokens": total_tokens,
            "estimated": estimated,
        }),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn record_usage_total(
    state: &AppState,
    provider_id: &str,
    model: &str,
    local: bool,
    purpose: &str,
    session_id: Option<&str>,
    total_tokens: u64,
) {
    record_usage_tokens(
        state,
        provider_id,
        model,
        local,
        purpose,
        session_id,
        0,
        0,
        total_tokens,
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-gateway-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn a_route_marked_slow_is_skipped_without_waiting() {
        mark_slow("http://slow.invalid/v1", "slow-model");
        let error = reserve_generation("http://slow.invalid/v1", "slow-model", None).unwrap_err();
        assert!(error.contains("느렸던"));
        assert!(reserve_generation("http://fast.invalid/v1", "fast-model", None).is_ok());
    }

    #[test]
    fn ovh_anonymous_limit_is_two_per_model_and_cooldowns_on_429() {
        let base = "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1";
        let model = "Qwen3.8-27B-test";
        assert!(reserve_generation(base, model, Some(2)).is_ok());
        assert!(reserve_generation(base, model, Some(2)).is_ok());
        assert!(reserve_generation(base, model, Some(2)).is_err());
        assert!(reserve_generation(base, "Qwen3.6-27B-test", Some(2)).is_ok());
        mark_rate_limited(base, "Qwen3.6-27B-test");
        assert!(reserve_generation(base, "Qwen3.6-27B-test", Some(2)).is_err());
        // Keyed routes are not subject to the anonymous limit.
        assert!(reserve_generation(base, "Qwen3.8-27B-test", None).is_ok());
    }

    #[test]
    fn disabled_or_text_incapable_providers_are_not_resolved() {
        let state = state();
        state.store.put(
            "provider",
            &json!({"id":"off","name":"off","baseUrl":"http://x/v1","model":"m","enabled":false}),
        );
        assert!(resolve(&state, "off", None).is_err());
        state.store.put(
            "provider",
            &json!({"id":"no-text","name":"x","baseUrl":"http://x/v1","model":"m","capabilities":{"text":false}}),
        );
        assert!(resolve(&state, "no-text", None).is_err());
    }

    #[test]
    fn default_selection_is_stable_and_respects_a_declared_default() {
        let state = state();
        for id in ["z-last", "a-first", "m-middle"] {
            state.store.put("provider", &json!({
                "id":id,"name":id,"baseUrl":format!("http://{id}/v1"),
                "model":"m","enabled":true
            })).unwrap();
        }
        assert_eq!(resolve(&state, "", None).unwrap().id, "a-first");
        let mut marked=state.store.get("provider","z-last").unwrap();
        marked["isDefault"]=json!(true);
        state.store.put("provider",&marked).unwrap();
        assert_eq!(resolve(&state, "", None).unwrap().id, "z-last");
        marked["enabled"]=json!(false);
        state.store.put("provider",&marked).unwrap();
        assert_eq!(resolve(&state, "", None).unwrap().id, "a-first");
    }
    #[test]
    fn capability_order_is_deterministic_and_declared_wins() {
        let state = state();
        for (id, model, quality) in [
            ("a", "gpt-4o", Value::Null),
            ("b", "tiny", json!(0.9)),
            ("c", "tiny", json!(0.9)),
        ] {
            let mut provider = json!({"id":id,"name":id,"baseUrl":format!("http://{id}/v1"),"model":model});
            if !quality.is_null() {
                provider["capabilities"] = json!({"quality":quality});
            }
            state.store.put("provider", &provider);
        }
        let ordered = ordered_alternatives(&state, "missing");
        assert_eq!(ordered[0].id, "b");
        assert_eq!(ordered[1].id, "c");
        assert_eq!(ordered[2].id, "a");
        assert_eq!(ordered[0].model, "tiny");
    }

    #[test]
    fn model_specific_capabilities_override_provider_defaults_and_wire_restrictions() {
        let profile=json!({"capabilities":{"toolCalling":true,"vision":false},
            "modelCapabilities":{"no-tools":{"toolCalling":false,"vision":true}}});
        let denied=ModelCapabilities::from_provider(&profile,"no-tools","native");
        assert_eq!(denied.tool_calling,Some(false));
        assert_eq!(denied.vision,Some(true));
        assert!(denied.require_native_tools().is_err());
        let parent=ModelCapabilities::from_provider(&profile,"other","native");
        assert_eq!(parent.tool_calling,Some(true));
        assert_eq!(parent.vision,Some(false));
        let compat=ModelCapabilities::from_provider(&profile,"other","compat");
        assert!(compat.require_native_tools().is_err());
    }

    #[test]
    fn exact_endpoint_streaming_output_and_reasoning_capabilities_guard_dispatch(){
        let state=state();
        state.store.put("provider",&json!({"id":"caps-local","baseUrl":"http://127.0.0.1:14444/v1",
            "name":"local","model":"capable","local":true,"toolMode":"native",
            "capabilities":{"streaming":true,"thinking":true,"maxOutputTokens":8192},
            "modelCapabilities":{"limited":{"streaming":false,
                "reasoningSupported":false,"maxOutputTokens":512,
                "toolCalling":false},"capable":{"streaming":true,
                "reasoningSupported":true,"maxOutputTokens":2048,
                "toolCalling":true}}})).unwrap();
        let rejected=resolve(&state,"caps-local",Some("limited")).unwrap();
        assert!(rejected.capabilities.require_streaming().is_err());
        assert!(rejected.capabilities.require_native_tools().is_err());
        assert!(rejected.disable_thinking);
        assert_eq!(rejected.max_output_tokens,Some(512));
        let supported=resolve(&state,"caps-local",Some("capable")).unwrap();
        assert!(supported.capabilities.require_streaming().is_ok());
        assert!(supported.capabilities.require_native_tools().is_ok());
        assert!(!supported.disable_thinking);
        assert_eq!(supported.max_output_tokens,Some(2048));
    }
    #[test]
    fn exact_model_context_and_native_tool_support_override_provider_defaults(){
        let state=state();
        state.store.put("provider",&json!({"id":"endpoint-test",
            "baseUrl":"http://127.0.0.1:4444/v1","name":"local","model":"large",
            "capabilities":{"contextTokens":65536},
            "modelCapabilities":{
                "small":{"contextTokens":4096,"toolCalling":false},
                "large":{"contextTokens":32768,"toolCalling":true}
            }})).unwrap();
        let small=resolve(&state,"endpoint-test",Some("small")).unwrap();
        let large=resolve(&state,"endpoint-test",None).unwrap();
        assert_eq!(small.context_tokens,Some(4096));
        assert_eq!(large.context_tokens,Some(32768));
        assert!(small.capabilities.require_native_tools().is_err());
        assert!(large.capabilities.require_native_tools().is_ok());
    }
    #[test]
    fn provider_route_rejects_explicit_no_tool_model_before_generation() {
        let state=state();
        state.store.put("provider",&json!({"id":"test-model","name":"test",
            "baseUrl":"http://127.0.0.1:1111/v1","model":"known",
            "toolMode":"native","modelCapabilities":{"known":{"toolCalling":false}}})).unwrap();
        let route=resolve(&state,"test-model",None).unwrap();
        assert!(route.capabilities.require_native_tools().is_err());
        let other=resolve(&state,"test-model",Some("other")).unwrap();
        assert_eq!(other.capabilities.tool_calling,None);
    }
    #[test]
    fn usage_parses_snake_and_camel_case() {
        assert_eq!(
            usage_from(&json!({"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}})),
            Usage { prompt_tokens: 3, completion_tokens: 5, total_tokens: 8 }
        );
        assert_eq!(
            usage_from(&json!({"usage":{"promptTokens":1,"completionTokens":2,"totalTokens":3}})),
            Usage { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3 }
        );
        assert_eq!(usage_from(&json!({})), Usage::default());
    }

    #[tokio::test]
    async fn local_admission_serializes_generations() {
        let first = acquire("http://127.0.0.1:9999/v1", true).await;
        let started = std::time::Instant::now();
        let waiter = tokio::spawn(async move {
            let _second = acquire("http://127.0.0.1:9999/v1", true).await;
            started.elapsed()
        });
        tokio::time::sleep(Duration::from_millis(80)).await;
        drop(first);
        let waited = waiter.await.unwrap();
        assert!(waited >= Duration::from_millis(70), "local calls overlapped");
    }
}
