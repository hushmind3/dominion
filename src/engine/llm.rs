//! Non-streaming model calls for planner, reviewer, reflection and learning.
//! Provider resolution, capability ordering and local admission live in
//! `engine::gateway`; this module only builds prompts and parses responses.
use crate::engine::gateway;
mod receipts;
pub(crate) use receipts::summarize as summarize_auxiliary_model_calls;
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

async fn try_complete(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
    session_id: Option<&str>,
    execution: Option<(&str, &str)>,
) -> Result<String, String> {
    let route = gateway::resolve(state, provider_id, Some(model))?;
    let invocation = uuid::Uuid::new_v4().to_string();
    // An auxiliary request is not part of the participant's main prompt
    // history. Give it an independent but stable relay affinity: this avoids
    // evicting the hot 4k main persona prefix with one-off helper prompts.
    // Out-of-turn calls still need a header (otherwise OpenCode Go returns
    // MissingSessionID), so key those by the execution or current invocation.
    let opencode_session=state.store.get("provider",&route.id)
        .filter(|provider|provider["integration"]=="opencode-go")
        .map(|_|{
            let scope=session_id
                .or_else(||execution.map(|(id,_)|id))
                .unwrap_or(invocation.as_str());
            format!("dominion-aux-{}",crate::engine::memory::stable_id(&[
                "auxiliary-opencode-session-v1",scope,&route.id,&route.model,
            ]))
        });
    if route.kind=="cli"{
        let source=route.base_url.strip_prefix("cli://").ok_or("CLI 제공자 주소 오류")?;
        let (budget_call,_)=if let Some(session)=session_id{
            crate::engine::execution_budget::prepare_call(state,session,&route.id,&route.model,route.local,
                crate::engine::chat::estimate_text_tokens(&format!("{system}{prompt}")),8192)?
        }else{(None,8192)};
        let receipt=receipts::begin(state,execution,&invocation,0,&route.id,&route.model)?;
        let generated=crate::engine::cli_model::generate(source,&route.model,&[
            json!({"role":"system","content":system}),
            json!({"role":"user","content":prompt})
        ]).await;
        let text=match generated{Ok(text)=>text,Err(error)=>{
            receipts::finish(state,receipt.as_deref(),"failed",gateway::Usage::default(),
                false,Some(&error));
            crate::engine::execution_budget::finish_call(budget_call,false,Some(&error))?;return Err(error);
        }};
        let prompt_tokens=crate::engine::chat::estimate_text_tokens(&format!("{system}{prompt}"));
        let output_tokens=text.len().div_ceil(4) as u64;
        gateway::record_usage_tokens(state,&route.id,&route.model,false,"helper",session_id,
            prompt_tokens,output_tokens,prompt_tokens+output_tokens,true);
        receipts::finish(state,receipt.as_deref(),"completed",gateway::Usage{
            prompt_tokens,completion_tokens:output_tokens,total_tokens:prompt_tokens+output_tokens,
        },true,None);
        crate::engine::execution_budget::finish_call(budget_call,true,None)?;
        return Ok(text);
    }
    // Thinking models can consume the entire completion budget on reasoning
    // and return empty content. Start with a practical budget and retry once
    // with a larger one before reporting an empty response.
    let mut max_tokens = 8192u64;
    let client = reqwest::Client::new();
    let mut last = String::new();
    for attempt in 0..2 {
        let mut body = json!({
            "model": route.model,
            "stream": false,
            "max_tokens": max_tokens,
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": prompt }
            ]
        });
        if route.disable_thinking {
            body["think"] = json!(false);
            body["chat_template_kwargs"] = json!({"enable_thinking": false});
        }
        let (budget_call,output)=if let Some(session)=session_id {
            crate::engine::execution_budget::prepare_call(state,session,&route.id,&route.model,route.local,
                crate::engine::chat::estimate_text_tokens(&format!("{system}{prompt}")),max_tokens)?
        }else{(None,max_tokens)};
        body["max_tokens"]=json!(output);
        let receipt=receipts::begin(state,execution,&invocation,attempt,&route.id,&route.model)?;
        let sent=gateway::send_with_affinity(&route,&client,&body,
            Duration::from_secs(240),opencode_session.as_deref()).await;
        let response=match sent {Ok(response)=>response,Err(error)=>{
            receipts::finish(state,receipt.as_deref(),"failed",gateway::Usage::default(),
                false,Some(&error));
            crate::engine::execution_budget::finish_call(budget_call,false,Some(&error))?;return Err(error);
        }};
        let response = response.response;
        if !response.status().is_success() {
            let error=format!("모델 호출 실패: HTTP {}",response.status());
            receipts::finish(state,receipt.as_deref(),"failed",gateway::Usage::default(),
                false,Some(&error));
            crate::engine::execution_budget::finish_call(budget_call,false,Some(&error))?;
            return Err(error);
        }
        let value:Value=match response.json().await {
            Ok(value)=>value,Err(error)=>{
                receipts::finish(state,receipt.as_deref(),"failed",gateway::Usage::default(),
                    false,Some(&error.to_string()));
                if let Some(call)=budget_call{call.completed_without_usage()?;}
                return Err(error.to_string());
            }
        };
        let usage=gateway::usage_from(&value);
        if usage.total_tokens==0&&usage.prompt_tokens==0&&usage.completion_tokens==0 {
            let input=crate::engine::chat::estimate_text_tokens(&format!("{system}{prompt}"));
            let output=value["choices"][0]["message"]["content"].as_str().unwrap_or("").len().div_ceil(4) as u64;
            gateway::record_usage_tokens(state,&route.id,&route.model,route.local,"helper",session_id,input,output,input+output,true);
            receipts::finish(state,receipt.as_deref(),"completed",gateway::Usage{
                prompt_tokens:input,completion_tokens:output,total_tokens:input+output,
            },true,None);
        }else{
            gateway::record_usage(state,&route,usage,"helper",session_id);
            receipts::finish(state,receipt.as_deref(),"completed",usage,false,None);
        }
        crate::engine::execution_budget::finish_call(budget_call,true,None)?;
        let choice = value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let content = choice["message"]["content"].as_str().unwrap_or("");
        if !content.trim().is_empty() {
            return Ok(content.to_string());
        }
        let finish = choice["finish_reason"].as_str().unwrap_or("unknown");
        let reasoning = choice["message"]["reasoning_content"]
            .as_str()
            .or_else(|| choice["message"]["reasoning"].as_str())
            .unwrap_or("");
        last = format!(
            "모델이 빈 응답을 반환했습니다 (finish={finish}, reasoning={}자, attempt={})",
            reasoning.chars().count(),
            attempt + 1
        );
        if finish != "length" {
            break;
        }
        max_tokens = (max_tokens * 2).min(16384);
    }
    Err(last)
}

pub async fn complete(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    try_complete(state, provider_id, model, system, prompt, None, None).await
}

/// Session-aware helper call used by room learning, reflection and planning so
/// their cost is attributed to the conversation that caused it.
pub async fn complete_for_session(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
    session_id: &str,
) -> Result<String, String> {
    try_complete(
        state,
        provider_id,
        model,
        system,
        prompt,
        Some(session_id),
        None,
    )
    .await
}

/// Optional-session variant for turn helpers that can run without a durable
/// session id. The call is still recorded; only the attribution differs.
pub async fn complete_for_turn(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
    session_id: Option<&str>,
) -> Result<String, String> {
    try_complete(state, provider_id, model, system, prompt, session_id, None).await
}

/// Measure an auxiliary phase against the native execution shared by chat, SDK and jobs.
#[allow(clippy::too_many_arguments)]
pub async fn complete_for_execution(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
    session_id: Option<&str>,
    execution_id: &str,
    phase: &str,
) -> Result<String, String> {
    try_complete(state, provider_id, model, system, prompt, session_id,
        Some((execution_id, phase))).await
}

/// A provider explicitly marked for learning is used as the teacher model,
/// even when the conversation itself runs on a smaller local model.
pub fn learning_route(state: &AppState) -> Option<(String, String)> {
    state
        .store
        .all("provider")
        .into_iter()
        .find(|provider| {
            // Accept the legacy `learning` key for providers created before
            // the role was renamed to `reflection`.
            (provider["reflection"].as_bool() == Some(true)
                || provider["learning"].as_bool() == Some(true))
                && provider["enabled"].as_bool() != Some(false)
        })
        .and_then(|provider| {
            Some((
                provider["id"].as_str()?.to_string(),
                provider["reflectionModel"].as_str()
                    .or_else(|| provider["model"].as_str())
                    .filter(|model| !model.is_empty())?
                    .to_string(),
            ))
        })
}

/// Learning and reflection should not fail just because the conversation used
/// a very small thinking model. Try the same model first, then up to two
/// capability-ordered alternatives.
/// Session-aware fallback helper for learning and reflection jobs.
pub async fn complete_with_fallback_for_session(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
    session_id: &str,
) -> Result<(String, String, String), String> {
    complete_with_fallback_inner(
        state,
        provider_id,
        model,
        system,
        prompt,
        Some(session_id),
    )
    .await
}

async fn complete_with_fallback_inner(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
    session_id: Option<&str>,
) -> Result<(String, String, String), String> {
    let alternatives = gateway::ordered_alternatives(state, provider_id);
    let mut last_error = String::new();
    for route in std::iter::once(None)
        .chain(alternatives.into_iter().take(2).map(Some))
    {
        let (candidate_id, candidate_model) = match &route {
            Some(route) => (route.id.clone(), route.model.clone()),
            None => (provider_id.to_string(), model.to_string()),
        };
        match try_complete(
            state,
            &candidate_id,
            &candidate_model,
            system,
            prompt,
            session_id,
            None,
        )
        .await
        {
            Ok(content) => return Ok((content, candidate_id, candidate_model)),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    #[tokio::test]
    async fn opencode_go_auxiliary_requests_keep_affinity_across_calls_and_headless_jobs() {
        use axum::{extract::State,http::HeaderMap,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        let seen=Arc::new(Mutex::new(Vec::<(String,String)>::new()));
        let app=Router::new().route("/v1/chat/completions",
            post(|State(seen):State<Arc<Mutex<Vec<(String,String)>>>>,
                  headers:HeaderMap,Json(body):Json<Value>| async move {
                let session=headers.get("x-opencode-session")
                    .and_then(|value|value.to_str().ok()).unwrap_or("").to_string();
                let model=body["model"].as_str().unwrap_or("").to_string();
                seen.lock().unwrap().push((model,session.clone()));
                if session.is_empty() && model=="deepseek-v4.1-flash" {
                    return (axum::http::StatusCode::BAD_REQUEST,
                        Json(json!({"error":{"type":"MissingSessionID"}})));
                }
                (axum::http::StatusCode::OK,Json(json!({
                    "choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":100,"completion_tokens":1,"total_tokens":101}
                })))
            })
        ).with_state(seen.clone());
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let state=state();
        for (id,model,integration) in [
            ("go","deepseek-v4.1-flash","opencode-go"),
            ("plain","generic-model",""),
        ] {
            state.store.put("provider",&json!({"id":id,"name":id,
                "baseUrl":format!("http://{address}/v1"),"model":model,
                "integration":integration,"enabled":true})).unwrap();
        }
        for session in [Some("room:alpha"),Some("room:alpha"),Some("room:beta"),None,None] {
            let result=complete_for_turn(&state,"go","deepseek-v4.1-flash",
                "unchanged owner policy","helper question",session).await.unwrap();
            assert_eq!(result,"ok");
        }
        let plain=complete_for_turn(&state,"plain","generic-model",
            "system","question",Some("room:alpha")).await.unwrap();
        assert_eq!(plain,"ok");
        let rows=seen.lock().unwrap();
        assert_eq!(rows.len(),6);
        assert!(!rows[0].1.is_empty());
        assert!(rows[0].1.starts_with("dominion-aux-"));
        assert_eq!(rows[0].1,rows[1].1,"same room helper calls need stable cache routing");
        assert_ne!(rows[0].1,rows[2].1,"distinct rooms must not share helper affinity");
        assert!(!rows[3].1.is_empty(),"out-of-turn helpers must never omit the Go header");
        assert_ne!(rows[3].1,rows[4].1,"unscoped one-shots cannot share a synthetic conversation");
        assert_eq!(rows[5],("generic-model".to_string(),String::new()),
            "generic providers must never receive OpenCode-specific headers");
        drop(rows);
        server.abort();
    }

    #[tokio::test]
    async fn helper_retry_charges_both_attempts_to_the_same_native_execution() {
        use axum::{extract::State, routing::post, Json, Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let invocations=Arc::new(AtomicUsize::new(0));
        let api=Router::new().route("/v1/chat/completions",
            post(|State(counter):State<Arc<AtomicUsize>>,Json(_):Json<Value>| async move {
                let nth=counter.fetch_add(1,Ordering::SeqCst);
                let content=if nth==0 { "" } else { "Plan: inspect, modify, verify" };
                Json(json!({
                    "choices":[{"message":{"content":content},
                        "finish_reason":if nth==0{"length"}else{"stop"}}],
                    "usage":{"prompt_tokens":10+nth,"completion_tokens":5+nth,
                        "total_tokens":15+2*nth}
                }))
            })
        ).with_state(invocations.clone());
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {axum::serve(listener,api).await.unwrap()});
        let state=state();
        state.store.put("provider",&json!({
            "id":"meter-provider","name":"Meter","baseUrl":format!("http://{addr}/v1"),
            "model":"test-model","enabled":true
        })).unwrap();
        let output=complete_for_execution(&state,"meter-provider","test-model",
            "You are a planner","Make a plan",None,"run-1","planning")
            .await.unwrap();
        assert_eq!(output,"Plan: inspect, modify, verify");
        assert_eq!(invocations.load(Ordering::SeqCst),2);
        let rows=state.store.auxiliary_model_calls("run-1").unwrap();
        assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["logicalId"],rows[1]["logicalId"]);
        let accounting=summarize_auxiliary_model_calls(&rows,1,1,50,false);
        assert_eq!(accounting["auxiliaryUpstreamAttempts"],2);
        assert_eq!(accounting["auxiliaryLogicalCalls"],1);
        assert_eq!(accounting["auxiliaryObservedTokens"],32);
        assert_eq!(accounting["observedTotalTokens"],82);
        assert_eq!(accounting["usageComplete"],true);
        assert_eq!(accounting["usageEstimated"],false);
        server.abort();
    }

    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-llm-{}", uuid::Uuid::new_v4()));
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
    fn learning_route_prefers_the_declared_teacher() {
        let state = state();
        state.store.put(
            "provider",
            &json!({"id":"local","name":"local","baseUrl":"http://127.0.0.1/v1","model":"small"}),
        );
        state.store.put(
            "provider",
            &json!({"id":"teacher","name":"teacher","baseUrl":"https://api/v1","model":"large","reflection":true}),
        );
        assert_eq!(
            learning_route(&state),
            Some(("teacher".into(), "large".into()))
        );
        // Older records used `learning`; keep them working during migration.
        state.store.put(
            "provider",
            &json!({"id":"teacher","name":"teacher","baseUrl":"https://api/v1","model":"large","learning":true}),
        );
        assert_eq!(
            learning_route(&state),
            Some(("teacher".into(), "large".into()))
        );
    }

    #[test]
    fn an_explicit_text_disabled_provider_is_not_a_fallback() {
        let state = state();
        state.store.put(
            "provider",
            &json!({"id":"bad","name":"bad","baseUrl":"http://bad/v1","model":"m","capabilities":{"text":false}}),
        );
        assert!(gateway::ordered_alternatives(&state, "primary").is_empty());
    }
}
