//! Optional protocol adapters. No external gateway code is bundled or executed.
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::time::Duration;
fn error(code: StatusCode, message: &str) -> Response {
    (code, Json(json!({"error":message}))).into_response()
}
async fn bounded_json(mut response:reqwest::Response,limit:usize)->Result<Value,String>{
    if response.content_length().is_some_and(|length|length>limit as u64){
        return Err(format!("허브 응답이 {limit} 바이트 한도를 초과했습니다."));
    }
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())?{
        if bytes.len().saturating_add(chunk.len())>limit{
            return Err(format!("허브 응답이 {limit} 바이트 한도를 초과했습니다."));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|error|error.to_string())
}
pub async fn freellmapi() -> Json<Value> {
    Json(
        json!({"id":"freellmapi","name":"FreeLLMAPI","kind":"openai-compatible-gateway","engine":"external-optional","repository":"https://github.com/tashfeenahmed/freellmapi","docs":"https://github.com/tashfeenahmed/freellmapi/blob/main/docs/en/api/01-rest-api.md","baseUrl":"http://127.0.0.1:3001/v1","models":["auto:fast","auto:balanced","auto:reliable","auto:smart","auto"],"authentication":"unified-bearer-key","signupAutomation":false,"bundled":false,"note":"각 제공자의 키를 FreeLLMAPI에 등록하고, 통합 키 하나를 DOMINION에 연결합니다. Google 로그인은 API 키 발급이나 무료 한도를 대신하지 않습니다."}),
    )
}
pub async fn inspect(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(provider) = state
        .store
        .get("provider", &id)
        .filter(|p| p["integration"] == "freellmapi")
    else {
        return error(StatusCode::NOT_FOUND, "FreeLLMAPI 연결을 찾을 수 없습니다.");
    };
    let base = provider["baseUrl"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('/');
    let key = state
        .read_secret(&format!("provider:{id}"))
        .unwrap_or_default();
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client)=>client,
        Err(build_error)=>return error(StatusCode::INTERNAL_SERVER_ERROR,&build_error.to_string()),
    };
    let models = client
        .get(format!("{base}/models"))
        .query(&[("available", "true")])
        .bearer_auth(&key)
        .timeout(Duration::from_secs(10))
        .send();
    let usage = client
        .get(format!("{base}/usage"))
        .bearer_auth(&key)
        .timeout(Duration::from_secs(10))
        .send();
    let (models, usage) = tokio::join!(models, usage);
    let models = match models {
        Ok(r) if r.status().is_success() => bounded_json(r,2*1024*1024).await.unwrap_or(json!({})),
        Ok(r) => {
            return error(
                StatusCode::BAD_GATEWAY,
                &format!("허브 모델 목록 HTTP {}", r.status()),
            )
        }
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "FreeLLMAPI 허브에 연결할 수 없습니다.",
            )
        }
    };
    let usage = match usage {
        Ok(r) if r.status().is_success() => bounded_json(r,256*1024).await.ok(),
        _ => None,
    };
    let entries = models["data"].as_array().cloned().unwrap_or_default();
    let usable = entries
        .iter()
        .filter(|m| m["execution_status"] == "ready")
        .count();
    // List only known usage fields; a remote extension cannot reflect a key.
    let usage=usage.map(|v|json!({"month":v["month"],"resetsAt":v["resetsAt"],"requests":v["requests"],"tokens":v["tokens"],"byPlatform":v["byPlatform"]}));
    Json(json!({"providerId":id,"reachable":true,"usableModels":usable,"catalogEntries":entries.len(),"models":entries.iter().map(|m|json!({"id":m["id"],"execution_status":m["execution_status"]})).collect::<Vec<_>>(),"usage":usage,"inferenceVerified":false})).into_response()
}
