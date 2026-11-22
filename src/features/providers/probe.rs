//! Connection checks read catalogs; they never buy a model generation.
use crate::state::AppState;
use serde_json::{json, Value};

pub fn catalog_url(provider: &Value) -> Result<String, String> {
    let base = super::gateway::resolved_base_url(provider);
    let parsed = reqwest::Url::parse(&base).map_err(|_| "연결할 서버 주소를 확인하세요.")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("HTTP(S) 서버 주소를 확인하세요.".into());
    }
    if base.contains("ACCOUNT_ID") || base.contains("{accountId}") {
        return Err("연결 설정에서 계정 ID를 입력하거나 연결 정보를 다시 저장하세요.".into());
    }
    Ok(if provider["integration"] == "opencode-go" {
        "https://opencode.ai/inference/go/v1/models".to_string()
    } else if provider["kind"] == "ollama" {
        format!(
            "{}/api/tags",
            base.trim_end_matches("/v1").trim_end_matches('/')
        )
    } else if base.contains("/client/v4/accounts/") && base.contains("/ai/v1") {
        format!(
            "{}/ai/models/search?per_page=100",
            base.trim_end_matches("/ai/v1").trim_end_matches('/')
        )
    } else {
        format!(
            "{base}/models{}",
            if provider["integration"] == "freellmapi" {
                "?available=true"
            } else {
                ""
            }
        )
    })
}

pub fn catalog_request(
    client: &reqwest::Client,
    provider: &Value,
    key: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let mut request = client.get(catalog_url(provider)?);
    if provider["kind"] == "anthropic" {
        request = request.header("anthropic-version", "2023-06-01");
        if !key.is_empty() {
            request = request.header("x-api-key", key);
        }
    } else if !key.is_empty() {
        request = request.bearer_auth(key);
    }
    Ok(request)
}

pub async fn check(state: &AppState, provider: &Value, key: &str) -> Value {
    let result = async {
        let client = state.provider_http_client(20)?;
        let response = catalog_request(&client, provider, key)?.send().await
            .map_err(|_| "서버에 연결할 수 없습니다. 주소와 실행 상태를 확인하세요.".to_string())?;
        let status = response.status();
        if !status.is_success() {
            let message = match status.as_u16() {
                401 | 403 => "인증 정보나 모델 목록 조회 권한을 확인하세요.",
                404 | 405 => "이 서버는 모델 목록 경로를 제공하지 않습니다. 서버 주소와 직접 입력한 모델 ID를 확인하세요.",
                429 => "호출 한도에 걸렸습니다. 잠시 후 다시 확인하세요.",
                _ => "서버가 연결 확인 요청을 처리하지 못했습니다.",
            };
            return Ok::<Value, String>(json!({"ok":false,"status":status.as_u16(),"message":message}));
        }
        let body = crate::integrations::network::read_bounded_json(response, 8*1024*1024, "Connection catalog").await?;
        let models = body.as_array().or_else(|| body["data"].as_array())
            .or_else(|| body["models"].as_array()).or_else(|| body["result"].as_array())
            .ok_or("서버가 모델 목록 형식의 응답을 보내지 않았습니다.")?;
        Ok(json!({"ok":true,"status":status.as_u16(),"modelCount":models.len(),
            "message":format!("서버에서 {}개 모델을 조회했습니다. 답변 생성은 실행하지 않았습니다.",models.len())}))
    }.await;
    let mut report = result.unwrap_or_else(|message| json!({"ok":false,"message":message}));
    report["verification"] = json!("catalog_read");
    report["generationVerified"] = json!(false);
    report
}
