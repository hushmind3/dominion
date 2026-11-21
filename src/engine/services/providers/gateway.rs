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

/// Provider endpoints receive account credentials and private prompts. Never
/// forward either across an unsolicited HTTP redirect.
pub fn provider_http_client()->&'static reqwest::Client{
    static CLIENT:OnceLock<reqwest::Client>=OnceLock::new();
    CLIENT.get_or_init(||reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build().expect("provider HTTP client configuration is valid"))
}

/// Read a remote response without allowing an unknown or dishonest length
/// header to grow the body buffer beyond the caller's limit.
pub async fn read_http_response_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(format!("원격 응답이 {limit} 바이트 한도를 초과했습니다."));
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or(0)
            .min(limit as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| error.to_string())?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(format!("원격 응답이 {limit} 바이트 한도를 초과했습니다."));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn read_http_response_json(
    response: reqwest::Response,
    limit: usize,
) -> Result<Value, String> {
    let bytes = read_http_response_limited(response, limit).await?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

/// A resolved provider + model pair. `base_url` never has a trailing slash.
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


/// Stable capability view for one model + endpoint. A model override can have
/// different limits from the provider's default model; absent declarations
/// remain unknown and must never be guessed from a model name.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct ModelRequirements{
    pub tools:bool,
    pub vision:bool,
    pub reasoning:bool,
}
/// The selected *endpoint* may have different abilities than the model's
/// default route. Only explicitly declared false values deny a feature:
/// older custom local adapters can still run with unknown capability data.
pub fn supports_requirements(
    provider:&Value,model_id:&str,required:ModelRequirements,
)->bool{
    supports_requirements_for_endpoint(provider,model_id,
        provider["baseUrl"].as_str().unwrap_or(""),required)
}
pub fn supports_requirements_for_endpoint(
    provider:&Value,model_id:&str,endpoint:&str,required:ModelRequirements,
)->bool{
    let layers=[
        &provider["endpointCapabilities"][endpoint][model_id],
        &provider["modelCapabilities"][model_id],
        &provider["capabilities"],
    ];
    let feature=|keys:&[&str]|->bool{
        for layer in layers{
            for key in keys{
                if let Some(value)=layer[*key].as_bool(){return value;}
            }
        }
        true
    };
    feature(&["text"])
        && (!required.tools||feature(&["toolCalling","tools"]))
        && (!required.vision||feature(&["vision","images"]))
        && (!required.reasoning||feature(&["reasoningSupported","thinking"]))
}
/// Versioned, source-addressed endpoint negotiation. No model name heuristics
/// or guessed price/cache savings. This reports actual selected endpoint
/// features but does not imply successful live API/provider negotiation.
pub fn negotiate_capabilities_for_endpoint(
    provider:&Value,model_id:&str,endpoint:&str,required:ModelRequirements,
)->Value{
    let layers=[
        ("endpoint",&provider["endpointCapabilities"][endpoint][model_id]),
        ("model",&provider["modelCapabilities"][model_id]),
        ("provider",&provider["capabilities"]),
    ];
    let bool_feature=|keys:&[&str]|->Value{
        for (source,layer) in layers{
            for key in keys{
                if let Some(value)=layer[*key].as_bool(){
                    return json!({"supported":value,"source":source,"key":key});
                }
            }
        }
        json!({"supported":null,"source":"unknown"})
    };
    let numeric=|keys:&[&str]|->Value{
        for (source,layer) in layers{
            for key in keys{
                if let Some(value)=layer[*key].as_u64(){
                    return json!({"value":value,"source":source,"key":key});
                }
            }
        }
        json!({"value":null,"source":"unknown"})
    };
    json!({
        "schemaVersion":2,
        "providerId":provider["id"],
        "modelId":model_id,
        "endpointId":endpoint,
        "protocol":provider["kind"],
        "local":is_local_provider(provider),
        "features":{
            "text":bool_feature(&["text"]),
            "tools":bool_feature(&["toolCalling","tools"]),
            "vision":bool_feature(&["vision","images"]),
            "reasoning":bool_feature(&["reasoningSupported","thinking"]),
            "parallelToolCalls":bool_feature(&["parallelToolCalls","parallel_tool_calls"]),
            "promptCache":bool_feature(&["promptCache","promptCaching"]),
            "streaming":bool_feature(&["streaming"]),
        },
        "limits":{
            "contextWindow":numeric(&["contextWindow","contextTokens"]),
            "maxOutputTokens":numeric(&["maxOutputTokens"]),
        },
        "required":{"tools":required.tools,"vision":required.vision,
                    "reasoning":required.reasoning},
        "admitted":supports_requirements_for_endpoint(provider,model_id,endpoint,required),
    })
}
/// Compatibility entrypoint for older clients, but endpoint-specific
/// capabilities always take precedence when the provider declares them.
pub fn negotiate_capabilities(
    provider:&Value,model_id:&str,required:ModelRequirements,
)->Value{
    negotiate_capabilities_for_endpoint(provider,model_id,
        provider["baseUrl"].as_str().unwrap_or(""),required)
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
    let context_tokens = provider["capabilities"]["contextTokens"]
        .as_u64()
        .or_else(|| provider["contextTokens"].as_u64());
    let max_output_tokens = provider["capabilities"]["maxOutputTokens"]
        .as_u64()
        .or_else(|| provider["maxOutputTokens"].as_u64());
    Ok(Route {
        name: provider["name"]
            .as_str()
            .unwrap_or("Provider")
            .to_string(),
        tool_mode: default_tool_mode(provider, &kind),
        reasoning_protocol: default_reasoning_protocol(provider, &kind, &model, &id),
        local,
        disable_thinking: disable_thinking(provider, local),
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

/// Resolves an explicit provider, or the first enabled provider when the id is
/// empty. This is the single entry point for non-chat model callers.
pub fn resolve(
    state: &AppState,
    provider_id: &str,
    model_override: Option<&str>,
) -> Result<Route, String> {
    let provider = if provider_id.is_empty() {
        state
            .store
            .all("provider")
            .into_iter()
            .find(provider_eligible)
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

/// Sends a helper completion through shared admission and authentication.
pub async fn send(
    route: &Route,
    client: &reqwest::Client,
    body: &Value,
    timeout: Duration,
) -> Result<GatewayResponse, String> {
    send_with_session(route,client,body,timeout,None).await
}

/// Exact official OpenCode Go origins. Keep unrelated proxies and Zen
/// paid/free routes out of the Go-specific conversation affinity contract.
pub fn opencode_go_base_url(base_url:&str)->bool{
    let Ok(url)=reqwest::Url::parse(base_url) else{return false};
    url.scheme()=="https" && url.host_str()==Some("opencode.ai")
        && ["/zen/go","/inference/go"].iter().any(|prefix|
            url.path().strip_prefix(*prefix)
                .is_some_and(|suffix|suffix.is_empty() || suffix.starts_with('/')))
}

/// A helper planner/reviewer/compactor is a DIFFERENT model conversation
/// from the owner's native tool loop. Its routing key is derived from the
/// owning session and immutable helper system text, never raw user messages
/// or credentials. Without a session, matching helper identities still
/// receive a stable cache affinity.
pub fn helper_go_affinity(route:&Route,body:&Value,session_id:Option<&str>)->Option<String>{
    if !opencode_go_base_url(&route.base_url){return None;}
    let system=body["messages"].as_array()
        .and_then(|messages|messages.iter().find(|m|m["role"]=="system"))
        .and_then(|message|message["content"].as_str()).unwrap_or("");
    Some(crate::engine::memory::stable_id(&[
        "go-helper-v1",&route.id,&route.base_url,&route.model,
        session_id.unwrap_or("sessionless"),&crate::engine::memory::stable_id(&[system]),
    ]))
}

/// Bounded request-builder seam shared by retries and tests. Header values
/// contain hashes only; the helper's user prompt and API key are not exposed.
fn with_go_session_header(
    builder:reqwest::RequestBuilder,route:&Route,body:&Value,
    session_id:Option<&str>,
)->reqwest::RequestBuilder{
    if let Some(affinity)=helper_go_affinity(route,body,session_id){
        builder.header("x-opencode-session",affinity)
    }else{builder}
}

/// Planner, reviewer and compaction calls must carry the same stable
/// OpenCode Go session-routing header contract as the native chat path.
pub async fn send_with_session(
    route: &Route,
    client: &reqwest::Client,
    body: &Value,
    timeout: Duration,
    session_id: Option<&str>,
) -> Result<GatewayResponse, String> {
    reserve_generation(
        &route.base_url,
        &route.model,
        route.requests_per_minute,
    )?;
    let permit = acquire(&route.base_url, route.local).await;
    let mut last_error = String::new();
    for attempt in 0..2 {
        let builder=benchmark_request(client.post(route.endpoint("/chat/completions")))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", route.api_key));
        let builder=with_go_session_header(builder,route,body,session_id);
        match builder
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
    let quoted=state.store.get("provider",&route.id).filter(|p|
        p["model"].as_str()==Some(route.model.as_str()))
        .map(|p|crate::engine::model_cost::estimate(&p,
            usage.prompt_tokens,usage.completion_tokens,None,false))
        .unwrap_or(Value::Null);
    state.store.audit(
        "model.usage",
        json!({
            "configuredCost":quoted,
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
    record_usage_tokens_cached(state, provider_id, model, local, purpose, session_id,
        prompt_tokens, completion_tokens, total_tokens, estimated, None);
}

/// Only provider-reported cache hits qualify as measured savings. The total
/// prompt token count already INCLUDES those cached tokens; never subtract
/// them from overall usage or guess a monetary discount.
#[allow(clippy::too_many_arguments)]
pub fn record_usage_tokens_cached(
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
    if total_tokens == 0 && prompt_tokens == 0 && completion_tokens == 0 {
        return;
    }
    let cache_hits=if estimated {None} else {
        cached_prompt_tokens.map(|count|count.min(prompt_tokens))
    };
    // Only the selected configured model may contribute owner-entered rates;
    // an unknown or overridden model has no reliable pricing.
    let quoted=state.store.get("provider",provider_id)
        .filter(|p|p["model"].as_str()==Some(model))
        .map(|p|crate::engine::model_cost::estimate(&p,
            prompt_tokens,completion_tokens,cache_hits,estimated))
        .unwrap_or(Value::Null);
    state.store.audit(
        "model.usage",
        json!({
            "providerId": provider_id,
            "model": model,
            "local": local,
            "purpose": purpose,
            "sessionId": session_id,
            "promptTokens": prompt_tokens,
            "completionTokens": completion_tokens,
            "totalTokens": total_tokens,
            "estimated": estimated,
            "cachedPromptTokens": cache_hits,
            "configuredCost":quoted,
            "uncachedPromptTokens": cache_hits.map(|hits|prompt_tokens.saturating_sub(hits)),
            "cacheEvidence": if cache_hits.is_some() {"provider_reported"} else {"unavailable"}
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
mod sub2_capability_tests {
    use super::*;
    #[test]
    fn model_level_negative_capabilities_override_provider_level_positive(){
        let provider=json!({
            "model":"basic","capabilities":{
                "text":true,"toolCalling":true,"vision":true,
                "reasoningSupported":true
            },
            "modelCapabilities":{
                "basic":{"toolCalling":false,"vision":false},
                "advanced":{"toolCalling":true,"vision":true}
            }
        });
        assert!(!supports_requirements(&provider,"basic",ModelRequirements{
            tools:true,..Default::default()
        }));
        assert!(!supports_requirements(&provider,"basic",ModelRequirements{
            vision:true,..Default::default()
        }));
        assert!(supports_requirements(&provider,"advanced",ModelRequirements{
            tools:true,vision:true,reasoning:true,
        }));
        assert!(supports_requirements(&provider,"unknown",ModelRequirements{
            tools:true,..Default::default()
        }));
    }
    #[test]
    fn endpoint_overrides_model_catalog_without_granting_unknown_features(){
        let provider=json!({
            "id":"local","kind":"openai","baseUrl":"http://127.0.0.1:8080/v1",
            "capabilities":{"toolCalling":true,"streaming":true,"promptCache":false},
            "modelCapabilities":{"qwen":{"toolCalling":true,"contextWindow":32768}},
            "endpointCapabilities":{
                "http://127.0.0.1:8080/v1":{
                    "qwen":{"toolCalling":false,"contextWindow":8192,"streaming":false}
                },
                "http://127.0.0.1:8090/v1":{
                    "qwen":{"toolCalling":true,"contextWindow":65536,"parallelToolCalls":true}
                }
            }
        });
        let required=ModelRequirements{tools:true,..Default::default()};
        let local=negotiate_capabilities_for_endpoint(&provider,"qwen",
            "http://127.0.0.1:8080/v1",required);
        assert_eq!(local["admitted"],false);
        assert_eq!(local["features"]["tools"]["source"],"endpoint");
        assert_eq!(local["limits"]["contextWindow"]["value"],8192);
        assert_eq!(local["features"]["streaming"]["supported"],false);
        let alternate=negotiate_capabilities_for_endpoint(&provider,"qwen",
            "http://127.0.0.1:8090/v1",required);
        assert_eq!(alternate["admitted"],true);
        assert_eq!(alternate["limits"]["contextWindow"]["value"],65536);
        assert_eq!(alternate["features"]["parallelToolCalls"]["supported"],true);
        assert_eq!(alternate["features"]["promptCache"]["source"],"provider");
        assert!(alternate["features"]["vision"]["supported"].is_null());
    }
    #[test]
    fn negotiation_records_model_overrides_and_unknown_capabilities_without_fake_grants(){
        let provider=json!({"id":"p","kind":"openai","baseUrl":"http://127.0.0.1:4321/v1",
            "capabilities":{"toolCalling":true,"vision":false},
            "modelCapabilities":{"a":{"toolCalling":false},"b":{"toolCalling":true}}});
        let needed=ModelRequirements{tools:true,vision:false,reasoning:false};
        let denied=negotiate_capabilities(&provider,"a",needed);
        assert_eq!(denied["admitted"],false);
        assert_eq!(denied["features"]["tools"]["source"],"model");
        let allowed=negotiate_capabilities(&provider,"b",needed);
        assert_eq!(allowed["admitted"],true);
        assert_eq!(allowed["features"]["vision"]["source"],"provider");
        assert_eq!(allowed["features"]["reasoning"]["source"],"unknown");
        assert!(allowed["features"]["reasoning"]["supported"].is_null());
        let unknown=negotiate_capabilities(&provider,"not-listed",needed);
        assert_eq!(unknown["features"]["tools"]["source"],"provider");
    }
    #[test]
    fn explicitly_unsupported_text_or_tools_never_receives_fallback_work(){
        let provider=json!({"capabilities":{"text":false,"tools":true}});
        assert!(!supports_requirements(&provider,"any",ModelRequirements::default()));
        let provider=json!({"capabilities":{"toolCalling":false}});
        assert!(!supports_requirements(&provider,"model",ModelRequirements{
            tools:true,..Default::default()
        }));
        assert!(supports_requirements(&provider,"model",ModelRequirements::default()));
    }
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
    fn helper_go_requests_have_stable_isolated_session_affinity(){
        let route=|base_url:&str|Route{
            id:"p".into(),name:"provider".into(),base_url:base_url.into(),
            api_key:"SECRET".into(),model:"deepseek-v4.1-flash".into(),
            kind:"openai".into(),tool_mode:"native".into(),
            reasoning_protocol:"deepseek".into(),local:false,
            disable_thinking:false,context_tokens:None,max_output_tokens:None,
            requests_per_minute:None,account_id:None
        };
        let go=route("https://opencode.ai/inference/go/openai/v1");
        let messages=json!({"messages":[
            {"role":"system","content":"HELPER_STATIC_RULES"},
            {"role":"user","content":"first task"}
        ]});
        let first=helper_go_affinity(&go,&messages,Some("owner-session-1")).unwrap();
        let request=with_go_session_header(
            reqwest::Client::new().post("http://127.0.0.1:1/test"),
            &go,&messages,Some("owner-session-1")).build().unwrap();
        assert_eq!(request.headers().get("x-opencode-session")
            .and_then(|value|value.to_str().ok()),Some(first.as_str()),
            "helper calls must really set Go's transport header, not only compute a key");

        let second=helper_go_affinity(&go,&json!({"messages":[
            {"role":"system","content":"HELPER_STATIC_RULES"},
            {"role":"user","content":"different task"}
        ]}),Some("owner-session-1")).unwrap();
        assert_eq!(first,second);
        assert_ne!(first,helper_go_affinity(&go,&messages,Some("owner-session-2")).unwrap());
        assert_ne!(first,helper_go_affinity(&go,&json!({"messages":[
            {"role":"system","content":"DIFFERENT_HELPER"},
            {"role":"user","content":"first task"}
        ]}),Some("owner-session-1")).unwrap());
        assert!(helper_go_affinity(&go,&messages,None).is_some(),
            "sessionless helper calls still need a valid Go header");
        assert!(!first.contains("SECRET"));
        for url in [
            "https://opencode.ai/zen/go/v1",
            "https://opencode.ai/inference/go/v1"
        ]{assert!(opencode_go_base_url(url));}
        for url in [
            "https://opencode.ai/zen/v1",
            "https://opencode.ai/inference/good/v1",
            "https://opencode.ai.evil.example/zen/go/v1",
            "http://opencode.ai/zen/go/v1"
        ]{assert!(!opencode_go_base_url(url));
            assert!(helper_go_affinity(&route(url),&messages,Some("s")).is_none());}
    }

    #[test]
    fn provider_rate_card_flows_to_real_usage_without_inventing_unpriced_costs(){
        let state=state();
        state.store.put("provider",&json!({"id":"p","model":"m","pricing":{
            "inputUsdPerMillion":2.0,"cachedInputUsdPerMillion":0.5,
            "outputUsdPerMillion":8.0}}));
        record_usage_tokens_cached(&state,"p","m",false,"chat",Some("priced"),
            1000,200,1200,false,Some(800));
        record_usage_tokens_cached(&state,"unknown","m",false,"chat",Some("priced"),
            200,10,210,false,None);
        let usage=state.store.session_usage("priced");
        assert_eq!(usage["pricedCalls"],1);
        assert_eq!(usage["unpricedCalls"],1);
        assert!((usage["pricedSubtotalUsd"].as_f64().unwrap()-0.0024).abs()<1e-10);
        assert!(usage["configuredCostUsd"].is_null(),
            "incomplete pricing must not be presented as a full bill");
    }
    
    #[test]
    fn cache_usage_reports_verified_tokens_without_double_counting() {
        let state=state();
        record_usage_tokens_cached(&state,"p","m",false,"chat",Some("s"),1000,60,1060,false,Some(800));
        // Estimated usage must not claim savings even when the input includes
        // a purported hit. Unmeasured calls are not cache misses.
        record_usage_tokens_cached(&state,"p","m",false,"chat",Some("s"),1000,60,1060,true,Some(800));
        let total=state.store.usage_summary();
        assert_eq!(total["calls"],2);
        assert_eq!(total["promptTokens"],2000);
        assert_eq!(total["totalTokens"],2120);
        assert_eq!(total["cachedPromptTokens"],800);
        assert_eq!(total["cacheObservedCalls"],1);
        let session=state.store.session_usage("s");
        assert_eq!(session["cachedPromptTokens"],800);
        assert_eq!(session["cacheObservedCalls"],1);
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
