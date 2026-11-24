use crate::engine;
use crate::state::AppState;
use axum::{extract::{Query,State},http::StatusCode,response::{IntoResponse,Response,sse::{Event,KeepAlive,Sse}},Json};
use serde_json::{json,Value};
use super::response::error;
use std::convert::Infallible;
use tokio::sync::mpsc;

pub async fn chat(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    // Snapshot admission before spawning: STOP may arrive while scheduling
    // this turn, before the native run has acquired its session lease.
    // Passing the epoch prevents that older turn clearing a newer STOP.
    let admitted_epoch = body["sessionId"].as_str()
        .map(|session| state.cancellation_generation(session));
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let task_state = state.clone();
    tokio::spawn(async move {
        engine::chat::run_with_http_epoch(task_state, body, sender, admitted_epoch).await;
    });

    let stream = async_stream::stream! {
        while let Some(event) = receiver.recv().await {
            yield Ok::<_, Infallible>(Event::default().data(event.to_string()));
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// The UI reads the same resolved model route as the chat transport. This
/// describes controls implemented by DOMINION, not a live provider probe.
/// Unknown generic endpoint capabilities must not invent selectable tiers.
fn inference_controls(kind: &str, protocol: &str, declared: &[Value], reasoning: Option<bool>) -> Value {
    let (toggle, efforts): (bool, Vec<&str>) = if reasoning == Some(false) || kind == "cli" {
        (false, vec![])
    } else if kind == "codex" {
        let efforts: Vec<_> = declared.iter().filter_map(Value::as_str)
            .filter(|effort| !effort.is_empty() && engine::reasoning::valid_effort(effort)).collect();
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
                .filter(|effort| !effort.is_empty() && *effort != "none" && engine::reasoning::valid_effort(effort)).collect()),
        }
    };
    json!({"thinkingToggle":toggle,"reasoningEfforts":efforts})
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceQuery {
    provider_id: String,
    model: String,
}

fn support_for_route(state: &AppState, query: &InferenceQuery) -> Result<Value, String> {
    let Some(provider) = state.store.get("provider", &query.provider_id) else {
        return Err("제공자를 찾을 수 없습니다.".into());
    };
    let route = engine::gateway::route_from_provider(state, &provider, Some(&query.model))?;
    let caps = engine::gateway::ModelCapabilities::for_route(&provider, &route.model);
    let catalog = if route.kind == "codex" {
        state.store.get("codex_model_catalog", &route.id).filter(|catalog| {
            route.account_id.as_deref().is_some_and(|account| catalog["accountId"].as_str() == Some(account))
        })
    } else { None };
    let endpoint = provider["endpointId"].as_str().filter(|id| !id.is_empty())
        .or_else(|| provider["baseUrl"].as_str()).unwrap_or("");
    let declared = if route.kind == "codex" {
        catalog.as_ref().and_then(|row| row["models"].as_array())
            .and_then(|rows| rows.iter().find(|row| row["id"].as_str() == Some(route.model.as_str())))
            .and_then(|row| row["reasoningEfforts"].as_array())
    } else {
        [&provider["endpointModelCapabilities"][endpoint][route.model.as_str()],
         &provider["endpointCapabilities"][endpoint],
         &provider["modelCapabilities"][route.model.as_str()], &provider["capabilities"]]
            .into_iter().find_map(|layer| layer["reasoningEfforts"].as_array())
    };
    let mut result = inference_controls(&route.kind, &route.reasoning_protocol,
        declared.map(Vec::as_slice).unwrap_or_default(), caps.reasoning);
    result["providerId"] = json!(route.id);
    result["model"] = json!(route.model);
    Ok(result)
}

pub async fn inference_support(State(state): State<AppState>, Query(query): Query<InferenceQuery>) -> Response {
    match support_for_route(&state, &query) {
        Ok(result) => Json(result).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

/// Check every candidate route with the same native transport contract.
/// This is a local metadata operation; it does not generate or bill tokens.
pub async fn inference_support_batch(State(state): State<AppState>, Json(queries): Json<Vec<InferenceQuery>>) -> Response {
    if queries.len() > 256 {
        return error(StatusCode::BAD_REQUEST, "한 번에 확인할 모델은 최대 256개입니다.");
    }
    match queries.iter().map(|query| support_for_route(&state, query)).collect::<Result<Vec<_>,_>>() {
        Ok(rows) => Json(json!(rows)).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

#[cfg(test)]
mod inference_support_tests {
    use super::*;
    #[test]
    fn route_controls_do_not_advertise_ignored_levels() {
        let advertised = vec![json!("none"),json!("low"),json!("high"),json!("ultra")];
        assert_eq!(inference_controls("ollama","chat_template",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":[]}));
        assert_eq!(inference_controls("openai","qwen",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":[]}));
        assert_eq!(inference_controls("openai","deepseek",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":["low","high"]}));
        assert_eq!(inference_controls("codex","openai",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":["none","low","high","ultra"]}));
        assert_eq!(inference_controls("openai","openai",&[],None),
            json!({"thinkingToggle":false,"reasoningEfforts":[]}));
        assert_eq!(inference_controls("openai","openai",&advertised,None),
            json!({"thinkingToggle":false,"reasoningEfforts":["low","high","ultra"]}));
        assert_eq!(inference_controls("openai","deepseek",&advertised,Some(false)),
            json!({"thinkingToggle":false,"reasoningEfforts":[]}));
    }
}
