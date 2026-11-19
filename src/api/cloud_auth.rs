//! Google desktop sign-in using the user's system browser, loopback callback and S256 PKCE.
//! Adapted to DOMINION's own Cloudflare identity service; no Nous tokens or UI.
use crate::state::AppState;
use axum::{extract::{Query, State}, http::StatusCode, response::{Html, IntoResponse, Response}, Json};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::OnceLock, time::{Duration, Instant}};
use tokio::sync::Mutex;

const CLOUD: &str = "https://dominion-cloud.dominion-hushmind.workers.dev";
const TTL: Duration = Duration::from_secs(300);

enum Phase {
    Waiting,
    Exchanging,
    Done(String),
    Failed(String),
}
struct Flight {
    verifier: String,
    client_id: String,
    redirect_uri: String,
    started: Instant,
    phase: Phase,
}
fn flights() -> &'static Mutex<HashMap<String, Flight>> {
    static FLIGHTS: OnceLock<Mutex<HashMap<String, Flight>>> = OnceLock::new();
    FLIGHTS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn nonce() -> String {
    let mut value = [0u8; 32];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}
fn error(message: &str, status: StatusCode) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
fn browser_open(url: &str) -> Result<(), String> {
    // Only the fixed Google authorization endpoint constructed by start() reaches here.
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    result.map(|_| ()).map_err(|error| format!("시스템 브라우저 실행 실패: {error}"))
}
fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(15))
        .build().expect("static HTTP client config")
}
fn client_id(config: &Value) -> Option<&str> {
    let id = config.get("google_desktop_client_id")?.as_str()?;
    (id.len() <= 256 && id.ends_with(".apps.googleusercontent.com")
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.'))
        .then_some(id)
}
pub async fn start(State(state): State<AppState>) -> Response {
    let response = match client().get(format!("{CLOUD}/v1/public-config")).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => return error("DOMINION Cloud 인증 서버에 연결하지 못했습니다.", StatusCode::BAD_GATEWAY),
    };
    let config: Value = match response.json().await {
        Ok(config) => config,
        _ => return error("인증 서버 설정을 읽지 못했습니다.", StatusCode::BAD_GATEWAY),
    };
    let Some(client_id) = client_id(&config) else {
        return error("데스크톱용 Google OAuth 앱 ID가 아직 등록되지 않았습니다.", StatusCode::SERVICE_UNAVAILABLE);
    };
    let id = nonce();
    let verifier = nonce(); // 43-character RFC 7636 verifier
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let redirect_uri = format!("http://127.0.0.1:{}/api/cloud-auth/google/callback", state.config.port);
    let mut url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("fixed URL");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &id)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", "select_account");
    {
        let mut map = flights().lock().await;
        map.retain(|_, flight| flight.started.elapsed() < TTL);
        if map.len() >= 32 {
            return error("로그인 시도가 너무 많습니다. 잠시 후 다시 시도하세요.", StatusCode::TOO_MANY_REQUESTS);
        }
        map.insert(id.clone(), Flight {
            verifier,
            client_id: client_id.to_owned(),
            redirect_uri,
            started: Instant::now(),
            phase: Phase::Waiting,
        });
    }
    if let Err(err) = browser_open(url.as_str()) {
        flights().lock().await.remove(&id);
        return error(&err, StatusCode::BAD_GATEWAY);
    }
    Json(json!({"id":id,"status":"waiting","expires_in":300})).into_response()
}
#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}
fn done_page(success: bool) -> Response {
    let (title, body) = if success {
        ("DOMINION 로그인 완료", "인증을 완료했습니다. 이 창을 닫고 DOMINION으로 돌아가세요.")
    } else {
        ("DOMINION 로그인 실패", "인증을 마치지 못했습니다. DOMINION에서 다시 시도하세요.")
    };
    let html = format!(
        "<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><title>{title}</title>\
        <meta http-equiv=\"Cache-Control\" content=\"no-store\"><body style=\"font:16px system-ui;\
        margin:4rem;color:#24211d;background:#fffdf9\"><h2>{title}</h2><p>{body}</p></body></html>"
    );
    ([(axum::http::header::CACHE_CONTROL, "no-store"),
      (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")], Html(html)).into_response()
}
pub async fn callback(Query(params): Query<Callback>) -> Response {
    let Some(state) = params.state.filter(|s| s.len() == 43 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) else {
        return (StatusCode::BAD_REQUEST, done_page(false)).into_response();
    };
    let (verifier, client_id, redirect_uri) = {
        let mut map = flights().lock().await;
        let Some(flight) = map.get_mut(&state) else { return done_page(false); };
        if flight.started.elapsed() >= TTL || !matches!(flight.phase, Phase::Waiting) {
            return done_page(false);
        }
        flight.phase = Phase::Exchanging;
        (flight.verifier.clone(), flight.client_id.clone(), flight.redirect_uri.clone())
    };
    let result = async {
        if let Some(reason) = params.error {
            return Err(format!("Google 인증 취소 또는 거절: {}", reason.chars().take(100).collect::<String>()));
        }
        let code = params.code.ok_or_else(|| "Google 인증 코드가 없습니다.".to_string())?;
        if code.len() > 2048 { return Err("잘못된 Google 인증 코드입니다.".into()); }
        let response = client().post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client_id.as_str()),
                ("code", code.as_str()),
                ("redirect_uri", redirect_uri.as_str()),
                ("code_verifier", verifier.as_str()),
            ]).send().await.map_err(|e| format!("Google 토큰 연결 실패: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("Google 토큰 교환 거절: HTTP {}", response.status()));
        }
        let oauth: Value = response.json().await.map_err(|_| "Google 응답이 올바르지 않습니다.")?;
        let credential = oauth.get("id_token").and_then(Value::as_str)
            .ok_or_else(|| "Google ID 토큰이 없습니다.".to_string())?;
        let response = client().post(format!("{CLOUD}/v1/auth/google"))
            .json(&json!({"credential":credential})).send().await
            .map_err(|e| format!("Cloudflare 인증 서버 연결 실패: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("DOMINION 계정 인증 실패: HTTP {}", response.status()));
        }
        let body: Value = response.json().await.map_err(|_| "Cloudflare 세션 응답 오류")?;
        let token = body.get("token").and_then(Value::as_str)
            .filter(|s| s.len() >= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
            .ok_or_else(|| "Cloudflare 세션 토큰이 없습니다.".to_string())?;
        Ok::<String,String>(token.to_owned())
    }.await;
    let success = result.is_ok();
    if let Some(flight) = flights().lock().await.get_mut(&state) {
        flight.phase = match result { Ok(token) => Phase::Done(token), Err(message) => Phase::Failed(message) };
    }
    done_page(success)
}
#[derive(Deserialize)]
pub struct StatusQuery { id: String }
pub async fn status(Query(q): Query<StatusQuery>) -> Response {
    if q.id.len() != 43 { return error("알 수 없는 로그인 시도입니다.", StatusCode::NOT_FOUND); }
    let mut map = flights().lock().await;
    let Some(flight) = map.get(&q.id) else {
        return error("로그인 시도가 만료되었거나 없습니다.", StatusCode::NOT_FOUND);
    };
    if flight.started.elapsed() >= TTL {
        map.remove(&q.id);
        return error("Google 로그인 시간이 만료됐습니다.", StatusCode::GONE);
    }
    match &flight.phase {
        Phase::Waiting => Json(json!({"status":"waiting"})).into_response(),
        Phase::Exchanging => Json(json!({"status":"processing"})).into_response(),
        Phase::Failed(_) | Phase::Done(_) => {
            let flight = map.remove(&q.id).expect("present");
            match flight.phase {
                Phase::Done(token) => Json(json!({"status":"done","token":token})).into_response(),
                Phase::Failed(message) => Json(json!({"status":"error","error":message})).into_response(),
                _ => unreachable!(),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_and_state_are_unpredictable_url_safe_values() {
        let a=nonce();
        let b=nonce();
        assert_eq!(a.len(),43);
        assert_ne!(a,b);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(a.as_bytes())).len(),43);
    }
    #[test]
    fn desktop_client_id_requires_google_suffix() {
        assert!(client_id(&json!({"google_desktop_client_id":"example-abc.apps.googleusercontent.com"})).is_some());
        assert!(client_id(&json!({"google_desktop_client_id":"attacker.example"})).is_none());
    }
}
