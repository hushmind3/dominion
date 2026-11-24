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
    let mut result = engine::reasoning::route_controls(state, &provider, &route);
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
        assert_eq!(engine::reasoning::inference_controls("ollama","chat_template",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":[]}));
        assert_eq!(engine::reasoning::inference_controls("openai","qwen",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":[]}));
        assert_eq!(engine::reasoning::inference_controls("openai","deepseek",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":["low","high"]}));
        assert_eq!(engine::reasoning::inference_controls("codex","openai",&advertised,None),
            json!({"thinkingToggle":true,"reasoningEfforts":["none","low","high","ultra"]}));
        assert_eq!(engine::reasoning::inference_controls("openai","openai",&[],None),
            json!({"thinkingToggle":false,"reasoningEfforts":[]}));
        assert_eq!(engine::reasoning::inference_controls("openai","openai",&advertised,None),
            json!({"thinkingToggle":false,"reasoningEfforts":["low","high","ultra"]}));
        assert_eq!(engine::reasoning::inference_controls("openai","deepseek",&advertised,Some(false)),
            json!({"thinkingToggle":false,"reasoningEfforts":[]}));
    }
}
