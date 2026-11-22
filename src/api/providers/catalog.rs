use crate::state::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

// models.dev API is one large JSON document, not a per-provider endpoint.
static MODELS_DEV_DOCUMENT: OnceLock<Mutex<Option<(Instant, Arc<Value>)>>> = OnceLock::new();
static MODELS_DEV_FETCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn models_dev_document_cache() -> &'static Mutex<Option<(Instant, Arc<Value>)>> {
    MODELS_DEV_DOCUMENT.get_or_init(|| Mutex::new(None))
}
pub(super) fn fresh_models_dev_document(
    cache: &Option<(Instant, Arc<Value>)>,
) -> Option<Arc<Value>> {
    cache.as_ref().and_then(|(fetched, document)| {
        (fetched.elapsed() < Duration::from_secs(86_400)).then(|| Arc::clone(document))
    })
}
fn cached_models_dev_document() -> Option<Arc<Value>> {
    let cache = models_dev_document_cache().lock().ok()?;
    fresh_models_dev_document(&cache)
}
pub(super) async fn load_models_dev_document() -> Result<Arc<Value>, String> {
    if let Some(document) = cached_models_dev_document() {
        return Ok(document);
    }
    let _fetch_guard = MODELS_DEV_FETCH.lock().await;
    // Another request may have populated the cache while we awaited the lock.
    if let Some(document) = cached_models_dev_document() {
        return Ok(document);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get("https://models.dev/api.json")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("공개 카탈로그 HTTP {}", response.status()));
    }
    let document = Arc::new(
        response
            .json::<Value>()
            .await
            .map_err(|error| error.to_string())?,
    );
    if let Ok(mut cache) = models_dev_document_cache().lock() {
        *cache = Some((Instant::now(), Arc::clone(&document)));
    }
    Ok(document)
}

/// Read-only public model directory. Neither endpoint nor cache depends on
/// a connected provider or its API key; authorization remains a separate action.
pub(super) fn public_models_from_metadata(provider: &Value) -> Vec<Value> {
    let mut result = provider["models"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, model)| {
            let id = model["id"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(id.as_str());
            if id.is_empty() {
                return None;
            }
            Some(json!({
                "id": id,
                "name": model["name"].as_str().filter(|name| !name.is_empty()).unwrap_or(id),
                "context": model["limit"]["context"].as_u64(),
                "reasoning": model["reasoning"].as_bool(),
                "toolCall": model["tool_call"].as_bool()
            }))
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    result
}

// A saved cloud provider with no credential can still display read-only
// public metadata. Match its exact API endpoint, not an arbitrary name or host.
pub(super) fn public_models_for_endpoint(document: &Value, base: &str) -> Option<Vec<Value>> {
    let target = base.trim().trim_end_matches('/');
    if target.is_empty() {
        return None;
    }
    let provider = document
        .as_object()?
        .values()
        .filter(|entry| {
            entry["api"].as_str().is_some_and(|api| {
                api.trim()
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case(target)
            })
        })
        .max_by_key(|entry| entry["models"].as_object().map_or(0, |models| models.len()))?;
    let mut models = public_models_from_metadata(provider);
    if models.is_empty() {
        return None;
    }
    for model in &mut models {
        model["free"] = Value::Null;
        model["freeStatus"] = json!("unknown");
        model["catalogSource"] = json!("models.dev");
    }
    Some(models)
}

pub async fn public_catalog_models(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Json<Value> {
    if provider_id.is_empty()
        || provider_id.len() > 128
        || !provider_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Json(json!({"models":[],"error":"업체 ID가 올바르지 않습니다."}));
    }
    let id = provider_id.to_ascii_lowercase();
    let cached = state.store.get("provider_catalog_models", &id);
    if let Some(record) = &cached {
        let fresh = record["fetchedAt"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|time| {
                (chrono::Utc::now() - time.with_timezone(&chrono::Utc)).num_seconds() < 86_400
            });
        if fresh {
            return Json(json!({"providerId":id,"models":record["models"],"cached":true}));
        }
    }
    let result = async {
        let data = load_models_dev_document().await?;
        let provider = data
            .get(&id)
            .ok_or("공개 카탈로그에서 해당 업체를 찾지 못했습니다.")?;
        Ok::<Vec<Value>, String>(public_models_from_metadata(provider))
    }
    .await;
    match result {
        Ok(models) => {
            let record =
                json!({"id":id,"fetchedAt":chrono::Utc::now().to_rfc3339(),"models":models});
            state.store.put("provider_catalog_models", &record);
            Json(json!({"providerId":id,"models":models,"cached":false}))
        }
        Err(error) => Json(json!({
            "providerId":id,
            "models":cached.as_ref().map(|value|value["models"].clone()).unwrap_or_else(||json!([])),
            "cached":cached.is_some(),
            "error":format!("공개 모델 목록을 새로 가져오지 못했습니다: {error}")
        })),
    }
}

/// OpenRouter publishes a public catalog. Showing free-model names never
/// needs the user's OpenRouter API key; actual chat calls still require auth.
pub(super) fn public_openrouter_free_models(value: &Value) -> Vec<Value> {
    let mut models = value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model["id"].as_str()?;
            if !id.ends_with(":free") {
                return None;
            }
            let zero = |field: &str| {
                model["pricing"][field]
                    .as_str()
                    .and_then(|price| price.parse::<f64>().ok())
                    .is_some_and(|price| price == 0.0)
            };
            if !zero("prompt") || !zero("completion") {
                return None;
            }
            if model["architecture"]["output_modalities"]
                .as_array()
                .is_some_and(|modalities| {
                    !modalities.iter().any(|mode| mode.as_str() == Some("text"))
                })
            {
                return None;
            }
            Some(json!({
                "id":id,
                "name":model["name"].as_str().unwrap_or(id),
                "context":model["context_length"].as_u64(),
            }))
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    models
}
pub async fn public_openrouter_free_catalog(State(state): State<AppState>) -> Json<Value> {
    let cached = state.store.get("public_free_models", "openrouter");
    if let Some(record) = &cached {
        let fresh = record["fetchedAt"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|time| {
                (chrono::Utc::now() - time.with_timezone(&chrono::Utc)).num_seconds() < 21_600
            });
        if fresh {
            return Json(json!({"models":record["models"],"cached":true}));
        }
    }
    let result = async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| error.to_string())?;
        let response = client
            .get("https://openrouter.ai/api/v1/models")
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("OpenRouter HTTP {}", response.status()));
        }
        let body = response
            .json::<Value>()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<Vec<Value>, String>(public_openrouter_free_models(&body))
    }
    .await;
    match result {
        Ok(models) => {
            let record = json!({"id":"openrouter","fetchedAt":chrono::Utc::now().to_rfc3339(),"models":models});
            state.store.put("public_free_models", &record);
            Json(json!({"models":models,"cached":false}))
        }
        Err(error) => Json(json!({
            "models":cached.as_ref().map(|value|value["models"].clone()).unwrap_or_else(||json!([])),
            "cached":cached.is_some(),
            "error":format!("무료 모델 목록 조회 실패: {error}")
        })),
    }
}

/// Public provider catalog backed by models.dev. The cache keeps the Models
/// tab usable offline and avoids hammering the upstream on every visit.
pub async fn provider_catalog(State(state): State<AppState>) -> Json<Value> {
    let cached = state.store.get("provider_catalog", "models.dev");
    if let Some(cached) = &cached {
        let fresh = cached["fetchedAt"]
            .as_str()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .is_some_and(|time| {
                (chrono::Utc::now() - time.with_timezone(&chrono::Utc)).num_seconds() < 86_400
            });
        if fresh {
            return Json(json!({
                "providers":cached["providers"],
                "cached":true
            }));
        }
    }
    let fetched = load_models_dev_document().await;
    let data = match fetched {
        Ok(data) => data,
        Err(error) => {
            if let Some(cached) = cached {
                return Json(json!({
                    "providers":cached["providers"],
                    "cached":true,
                    "error":format!("최신 카탈로그를 가져오지 못했습니다: {error}")
                }));
            }
            return Json(
                json!({"providers":[],"error":format!("공개 카탈로그를 가져오지 못했습니다: {error}")}),
            );
        }
    };
    let mut providers = Vec::new();
    for (id, item) in data.as_object().into_iter().flatten() {
        let Some(api) = item["api"].as_str().filter(|value| !value.is_empty()) else {
            continue;
        };
        let models = item["models"]
            .as_object()
            .map(|models| models.len())
            .unwrap_or(0);
        if models == 0 {
            continue;
        }
        providers.push(json!({
            "id":id,
            "name":item["name"].as_str().unwrap_or(id),
            "api":api,
            "models":models,
            "env":item["env"],
            "doc":item["doc"],
        }));
    }
    providers.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    providers.truncate(300);
    let record = json!({
        "id":"models.dev",
        "fetchedAt":chrono::Utc::now().to_rfc3339(),
        "providers":providers
    });
    state.store.put("provider_catalog", &record);
    Json(json!({"providers":record["providers"],"cached":false}))
}

/// Refreshes every enabled provider's model catalogue and stores the
/// free/paid/unknown classification with a timestamp. Invoked explicitly
/// from the Models page; idle sessions never call provider model endpoints.
pub async fn refresh_free_catalog(state: &AppState) -> Value {
    let lock = "free-catalog-refresh";
    if !state.begin_session(lock) {
        return state
            .store
            .get("free_catalog", "catalog")
            .unwrap_or_else(|| json!({"id":"catalog","status":"running","providers":[]}));
    }
    let _guard = state.session_guard(lock);
    // Read through the same handler boundary as the UI. A loopback HTTP call
    // loses owner authentication behind proxies and needlessly serializes the
    // whole directory behind an unavailable service.
    use futures_util::{stream, StreamExt};
    let enabled = state
        .store
        .all("provider")
        .into_iter()
        .filter(|provider| provider["enabled"].as_bool() != Some(false))
        .filter_map(|provider| {
            Some((
                provider["id"].as_str()?.to_string(),
                provider["name"].as_str().unwrap_or("AI").to_string(),
            ))
        })
        .enumerate();
    let mut requests = stream::iter(enabled.map(|(index, (id, name))| {
        let state = state.clone();
        async move {
            let result = tokio::time::timeout(Duration::from_secs(20),
                super::models::provider_models(State(state), Path(id.clone()))
            ).await;
            let mut row = json!({"providerId":id,"providerName":name,"reachable":false,"models":[]});
            match result {
                Ok(response) => {
                    let status = response.status();
                    match axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024).await {
                        Ok(body) => match serde_json::from_slice::<Value>(&body) {
                            Ok(Value::Array(models)) if status.is_success() => {
                                row["reachable"] = json!(true);
                                row["models"] = json!(models.into_iter().map(|model| json!({
                                    "id":model["id"],"free":model["free"],
                                    "freeStatus":model["freeStatus"],"freeSource":model["freeSource"],
                                })).collect::<Vec<_>>());
                            }
                            Ok(value) => row["error"] = json!(value["error"].as_str().unwrap_or("AI 목록을 불러오지 못했습니다.")),
                            Err(_) => row["error"] = json!("AI 목록 응답을 읽지 못했습니다."),
                        },
                        Err(_) => row["error"] = json!("AI 목록 응답을 읽지 못했습니다."),
                    }
                }
                Err(_) => row["error"] = json!("응답이 오래 걸리고 있습니다. 잠시 후 다시 시도하세요."),
            }
            (index, row)
        }
    })).buffer_unordered(4);
    let mut results = Vec::new();
    while let Some(result) = requests.next().await {
        results.push(result);
    }
    results.sort_by_key(|(index, _)| *index);
    let providers = results.into_iter().map(|(_, row)| row).collect::<Vec<_>>();
    let (mut free, mut paid, mut unknown) = (0u64, 0u64, 0u64);
    for row in &providers {
        for model in row["models"].as_array().into_iter().flatten() {
            match model["freeStatus"].as_str() {
                Some("free") => free += 1,
                Some("paid") => paid += 1,
                _ => unknown += 1,
            }
        }
    }
    let record = json!({
        "id":"catalog",
        "status":"ready",
        "updatedAt":chrono::Utc::now().to_rfc3339(),
        "providers":providers,
        "counts":{"free":free,"paid":paid,"unknown":unknown},
    });
    state.store.put("free_catalog", &record);
    record
}

pub async fn free_catalog(State(state): State<AppState>) -> Json<Value> {
    Json(
        state
            .store
            .get("free_catalog", "catalog")
            .unwrap_or_else(|| {
                json!({
                    "id":"catalog","status":"empty","providers":[],
                    "counts":{"free":0,"paid":0,"unknown":0}
                })
            }),
    )
}

pub async fn free_catalog_refresh(State(state): State<AppState>) -> Json<Value> {
    Json(refresh_free_catalog(&state).await)
}
