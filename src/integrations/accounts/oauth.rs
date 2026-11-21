use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

struct Provider {
    id: &'static str,
    name: &'static str,
    authorize: &'static str,
    token: &'static str,
    user: &'static str,
    scopes: &'static str,
    extra: &'static [(&'static str, &'static str)],
}

const PROVIDERS: &[Provider] = &[
    Provider {
        id: "github",
        name: "GitHub",
        authorize: "https://github.com/login/oauth/authorize",
        token: "https://github.com/login/oauth/access_token",
        user: "https://api.github.com/user",
        scopes: "repo read:user user:email",
        extra: &[],
    },
    Provider {
        id: "google",
        name: "Google 계정",
        authorize: "https://accounts.google.com/o/oauth2/v2/auth",
        token: "https://oauth2.googleapis.com/token",
        user: "https://www.googleapis.com/oauth2/v3/userinfo",
        scopes: "openid email profile https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send https://www.googleapis.com/auth/drive https://www.googleapis.com/auth/documents https://www.googleapis.com/auth/calendar.events",
        extra: &[("access_type", "offline"), ("prompt", "consent")],
    },
];

fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

fn oauth_client(state:&AppState) -> Result<reqwest::Client,String> {
    state.account_http_client(30)
        .map_err(|error|format!("OAuth HTTP client initialization failed: {error}"))
}

/// Shared authority check for account listings and OAuth dispatch.
/// Persisted records of retired integrations are never displayable accounts.
pub fn is_supported_provider(id: &str) -> bool {
    provider(id).is_some()
}

/// Retire credentials left by removed built-in service integrations.
/// Only the obsolete OAuth grants are erased, never unrelated jobs, artifacts
/// or account grants for supported providers. Run after DB migrations.
pub fn purge_retired_grants(state: &AppState) {
    for id in ["youtube"] {
        state.store.remove_secret(&format!("oauth:{id}:token"));
        state.store.remove_secret(&format!("oauth:{id}:secret"));
        state.store.remove("oauth_config", id);
        for entry in state.store.all("oauth_state") {
            if entry["provider"].as_str() == Some(id) {
                if let Some(key) = entry["id"].as_str() {
                    state.store.remove_secret(&format!("oauth-state:{key}"));
                    state.store.remove("oauth_state", key);
                }
            }
        }
    }
}

fn config(state: &AppState, id: &str) -> Option<Value> {
    state.store.get("oauth_config", id)
}

fn callback_url(state: &AppState, id: &str) -> String {
    format!(
        "http://127.0.0.1:{}/api/oauth/{id}/callback",
        state.config.port
    )
}

pub fn status(state: &AppState) -> Vec<Value> {
    PROVIDERS
        .iter()
        .map(|provider| {
            let record = config(state, provider.id).unwrap_or_else(|| json!({}));
            let connected = record
                .get("connected")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                && state
                    .store
                    .has_secret(&format!("oauth:{}:token", provider.id));
            json!({
                "id": provider.id,
                "name": provider.name,
                "configured": client_credentials(state, provider.id).is_ok(),
                "connected": connected,
                "account": record.get("account").cloned().unwrap_or(Value::Null),
                "connectedAt": record.get("connectedAt"),
                "callback": callback_url(state, provider.id),
                "scopes": provider.scopes,
                "scope": provider.scopes,
                "grantedScopes": record.get("scopes"),
                "workspaceReady": provider.id!="google" || [
                    "https://www.googleapis.com/auth/gmail.readonly",
                    "https://www.googleapis.com/auth/gmail.send",
                    "https://www.googleapis.com/auth/drive",
                    "https://www.googleapis.com/auth/documents",
                    "https://www.googleapis.com/auth/calendar.events",
                ].iter().all(|scope|record["scopes"].as_str().unwrap_or("")
                    .split(|c:char|c.is_whitespace()||c==',').any(|s|s==*scope)),
            })
        })
        .collect()
}

pub fn save(
    state: &AppState,
    id: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<Value, String> {
    let provider = provider(id).ok_or_else(|| format!("지원하지 않는 OAuth 제공자입니다: {id}"))?;
    if client_id.trim().is_empty() || (id == "github" && client_secret.trim().is_empty()) {
        return Err("Client ID가 필요합니다. GitHub 앱은 Client Secret도 입력하세요.".into());
    }
    let mut record = config(state, id).unwrap_or_else(|| json!({ "id": id }));
    // A new OAuth app registration cannot inherit an earlier app's access
    // grant or previously reported scopes. Explicit reconnect is required.
    state.store.remove_secret(&format!("oauth:{id}:token"));
    record["connected"] = json!(false);
    record["authSource"] = Value::Null;
    record["account"] = Value::Null;
    record["scopes"] = Value::Null;
    record["id"] = json!(id);
    record["clientId"] = json!(client_id.trim());
    record["configuredAt"] = json!(chrono::Utc::now().to_rfc3339());
    state
        .store
        .put("oauth_config", &record)
        .ok_or("OAuth 앱 설정 저장 실패")?;
    state.save_secret(
        &format!("oauth:{}:secret", provider.id),
        &json!({ "clientId": client_id.trim(), "clientSecret": client_secret.trim() }).to_string(),
    );
    if client_credentials(state, id).is_err() {
        return Err("OAuth 앱 자격 증명 저장 실패".into());
    }
    Ok(json!({
        "id": provider.id,
        "name": provider.name,
        "configured": true,
        "callback": callback_url(state, provider.id),
    }))
}

/// Packaged deployments can configure a single registered OAuth app once,
/// enabling account owners to click Connect without entering developer
/// credentials individually. Secrets remain in the host environment.
fn deployed_client_credentials(id:&str)->Option<(String,String)> {
    let service=match id{
        "github"=>"GITHUB","google"=>"GOOGLE",
        _=>return None
    };
    let read=|key:&str|std::env::var(format!("DOMINION_{service}_OAUTH_{key}"))
        .ok().filter(|v|!v.trim().is_empty());
    let client=read("CLIENT_ID")?;
    let secret=read("CLIENT_SECRET").unwrap_or_default();
    if id=="github"&&secret.is_empty(){return None;}
    Some((client,secret))
}
fn client_credentials(state: &AppState, id: &str) -> Result<(String, String), String> {
    let sealed = state
        .read_secret(&format!("oauth:{id}:secret"))
        .or_else(||deployed_client_credentials(id).map(|(client,secret)|json!({
            "clientId":client,"clientSecret":secret
        }).to_string()))
        .ok_or_else(|| "OAuth 앱이 설정되지 않았습니다. 설치 시 OAuth Client ID를 등록하거나 개발자 설정에 입력하세요.".to_string())?;
    let value: Value = serde_json::from_str(&sealed).map_err(|error| error.to_string())?;
    let client_id = value
        .get("clientId")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let client_secret = value
        .get("clientSecret")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    if client_id.is_empty() || (id == "github" && client_secret.is_empty()) {
        return Err("OAuth 앱 자격 증명이 비어 있습니다.".into());
    }
    Ok((client_id, client_secret))
}

pub fn start(state: &AppState, id: &str) -> Result<Value, String> {
    let provider = provider(id).ok_or_else(|| format!("지원하지 않는 OAuth 제공자입니다: {id}"))?;
    let (client_id, _) = client_credentials(state, id)?;
    use base64::Engine;
    use sha2::Digest;
    let state_token = uuid::Uuid::new_v4().to_string();
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    state.save_secret(&format!("oauth-state:{state_token}"), &verifier);
    state.store.put(
        "oauth_state",
        &json!({
            "id": state_token,
            "provider": id,
            "createdMs": chrono::Utc::now().timestamp_millis(),
        }),
    );

    let mut url = reqwest::Url::parse(provider.authorize).map_err(|error| error.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &callback_url(state, id))
            .append_pair("scope", provider.scopes)
            .append_pair("state", &state_token);
        for (key, value) in provider.extra {
            query.append_pair(key, value);
        }
        if id == "google" {
            query
                .append_pair("response_type", "code")
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256");
            if let Some(account) =
                config(state, "google").and_then(|r| r["account"].as_str().map(str::to_string))
            {
                query.append_pair("login_hint", &account);
            }
        }
    }
    Ok(json!({ "url": url.to_string(), "state": state_token }))
}

pub async fn callback(
    state: &AppState,
    id: &str,
    code: &str,
    state_token: &str,
) -> Result<Value, String> {
    let provider = provider(id).ok_or_else(|| format!("지원하지 않는 OAuth 제공자입니다: {id}"))?;
    if code.trim().is_empty() || state_token.trim().is_empty() {
        return Err("code 와 state 가 필요합니다.".into());
    }

    let record = state
        .store
        .get("oauth_state", state_token)
        .ok_or("알 수 없는 OAuth state 입니다. 다시 연결하세요.")?;
    let created_ms = record
        .get("createdMs")
        .and_then(|value| value.as_i64())
        .ok_or("OAuth state 가 손상되었습니다.")?;
    if chrono::Utc::now().timestamp_millis() - created_ms > 10 * 60 * 1000 {
        state.store.remove("oauth_state", state_token);
        state
            .store
            .remove_secret(&format!("oauth-state:{state_token}"));
        return Err("OAuth state 가 만료되었습니다. 다시 연결하세요.".into());
    }
    if record.get("provider").and_then(|value| value.as_str()) != Some(provider.id) {
        return Err("OAuth state 제공자가 일치하지 않습니다.".into());
    }
    state
        .store
        .patch_if(
            "oauth_state",
            state_token,
            "used",
            &Value::Null,
            &json!({"used":true}),
        )
        .ok_or("이미 처리한 OAuth state입니다.")?;
    state.store.remove("oauth_state", state_token);
    let verifier = state.read_secret(&format!("oauth-state:{state_token}"));
    state
        .store
        .remove_secret(&format!("oauth-state:{state_token}"));

    let (client_id, client_secret) = client_credentials(state, id)?;
    let client = oauth_client(state)?;
    let mut form = vec![
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code.to_string()),
        ("redirect_uri", callback_url(state, id)),
        ("grant_type", "authorization_code".to_string()),
    ];
    if provider.id == "github" {
        form.pop();
    } else {
        form.push((
            "code_verifier",
            verifier.ok_or("PKCE verifier가 없습니다. 다시 연결하세요.")?,
        ));
        form.retain(|(key, value)| *key != "client_secret" || !value.is_empty());
    }

    let response = client
        .post(provider.token)
        .header("Accept", "application/json")
        .form(&form)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| format!("토큰 교환 실패: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("OAuth 토큰 교환 거부: HTTP {status}"));
    }
    let text = response.text().await.unwrap_or_default();
    let token_value: Value = serde_json::from_str(&text)
        .map_err(|_| format!("토큰 응답을 해석할 수 없습니다 (HTTP {status})"))?;
    let access_token = token_value
        .get("access_token")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if access_token.is_empty() {
        let error = token_value
            .get("error_description")
            .or_else(|| token_value.get("error"))
            .and_then(|value| value.as_str())
            .unwrap_or("access_token 없음");
        return Err(format!("OAuth 토큰 교환 거부됨: {error}"));
    }

    let mut account = Value::Null;
    if let Ok(info) = client
        .get(provider.user)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(20))
        .send()
        .await
    {
        if let Ok(value) = info.json::<Value>().await {
            account = value
                .get("login")
                .or_else(|| value.get("email"))
                .or_else(|| value.get("name"))
                .cloned()
                .unwrap_or(Value::Null);
        }
    }

    state.save_secret(
        &format!("oauth:{id}:token"),
        &json!({
            "accessToken": access_token,
            "refreshToken": token_value.get("refresh_token"),
            "scopes": token_value.get("scope"),
            "obtainedAt": chrono::Utc::now().to_rfc3339(),
            "expiresAt": token_value["expires_in"].as_i64().map(|seconds| chrono::Utc::now().timestamp() + seconds),
        })
        .to_string(),
    );

    let mut record = config(state, id).unwrap_or_else(|| json!({ "id": id }));
    record["id"] = json!(id);
    record["connected"] = json!(true);
    record["authSource"] = json!("oauth");
    record["scopes"] = token_value
        .get("scope")
        .cloned()
        .unwrap_or(json!(provider.scopes));
    record["account"] = account.clone();
    record["connectedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if self::access_token(state, id).as_deref() != Some(access_token.as_str()) {
        return Err("OAuth 토큰 저장 실패".into());
    }
    state
        .store
        .put("oauth_config", &record)
        .ok_or("OAuth 연결 상태 저장 실패")?;
    state.bus.record("oauth_config", &record);
    state.store.audit(
        "oauth.connected",
        json!({ "provider": id, "account": account }),
    );

    Ok(json!({ "provider": id, "connected": true, "account": account }))
}

pub fn disconnect(state: &AppState, id: &str) {
    if provider(id).is_none() { return; }
    state.store.remove_secret(&format!("oauth:{id}:token"));
    if let Some(mut record) = config(state, id) {
        record["connected"] = json!(false);
        record["authSource"] = Value::Null;
        record["account"] = Value::Null;
        state.store.put("oauth_config", &record);
        state.bus.record("oauth_config", &record);
    }
}

/// Bearer token for registered service tools only.
pub fn access_token(state: &AppState, id: &str) -> Option<String> {
    provider(id)?;
    let sealed = state.read_secret(&format!("oauth:{id}:token"))?;
    let value: Value = serde_json::from_str(&sealed).ok()?;
    value
        .get("accessToken")
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

/// Refresh long-lived service grants only for supported providers.
/// Old removed-provider tokens must not be usable even before startup cleanup.
pub async fn fresh_access_token(state: &AppState, id: &str) -> Result<String, String> {
    provider(id).ok_or("지원하지 않는 OAuth 제공자입니다.")?;
    if id=="github"&&config(state,"github")
        .is_some_and(|v|v["authSource"]=="github_cli"){
        // Disconnect revokes Dominion's authority even if the user stays
        // signed in to the separate official GitHub CLI installation.
        if config(state,"github").is_none_or(|r|r["connected"]!=true)
            ||!state.store.has_secret("oauth:github:token"){
            return Err("GitHub account is disconnected in Dominion".into());
        }
        // GitHub CLI owns its browser grant and rotates/refreshes credentials
        // itself. Read the CURRENT token, pinned to the connected username;
        // do not reuse a stale copied bearer when the CLI is logged out.
        let record=config(state,"github").ok_or("GitHub account missing")?;
        let name=record["account"].as_str().unwrap_or("");
        if name.is_empty()||name.len()>128||
           !name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'){
            return Err("Connected GitHub username is invalid".into());
        }
        let token=cli_output("gh",&["auth","token","--hostname","github.com","--user",name])
            .await?.trim().to_string();
        if token.len()<12||token.len()>8192{
            return Err("GitHub CLI authorization has expired; reconnect GitHub".into());
        }
        return Ok(token);
    }
    let raw = state
        .read_secret(&format!("oauth:{id}:token"))
        .ok_or("계정을 먼저 연결하세요.")?;
    let mut token: Value =
        serde_json::from_str(&raw).map_err(|_| "저장된 OAuth 토큰이 손상되었습니다.")?;
    if token["expiresAt"]
        .as_i64()
        .is_none_or(|expiry| expiry > chrono::Utc::now().timestamp() + 60)
    {
        return token["accessToken"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .ok_or("액세스 토큰이 없습니다.".into());
    }
    let refresh = token["refreshToken"]
        .as_str()
        .ok_or("인증이 만료되었습니다. 계정을 다시 연결하세요.")?;
    let provider = provider(id).ok_or("지원하지 않는 OAuth 제공자입니다.")?;
    let (client_id, client_secret) = client_credentials(state, id)?;
    let response = oauth_client(state)?
        .post(provider.token)
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh),
            ("grant_type", "refresh_token"),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "OAuth 갱신 연결 실패")?;
    if !response.status().is_success() {
        return Err("OAuth 갱신이 거부되었습니다. 계정을 다시 연결하세요.".into());
    }
    let updated: Value = response.json().await.map_err(|_| "OAuth 갱신 응답 오류")?;
    let access = updated["access_token"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("갱신된 토큰이 없습니다.")?
        .to_string();
    token["accessToken"] = json!(access);
    token["expiresAt"] =
        json!(chrono::Utc::now().timestamp() + updated["expires_in"].as_i64().unwrap_or(3600));
    state.save_secret(&format!("oauth:{id}:token"), &token.to_string());
    Ok(access)
}

/// Browser login through the provider's own installed CLI. The application
/// never requests or displays an API key / Client ID from an end user.
async fn cli_output(binary:&str,args:&[&str])->Result<String,String>{
    let mut command=tokio::process::Command::new(binary);
    command.args(args).kill_on_drop(true);
    if binary=="gh"{command.env_remove("GH_TOKEN").env_remove("GITHUB_TOKEN");}
    let output=tokio::time::timeout(Duration::from_secs(15),command.output())
        .await.map_err(|_|format!("{binary} credential query timed out"))?
        .map_err(|e|format!("{binary} CLI unavailable: {e}"))?;
    if !output.status.success(){
        return Err(format!("{binary} login is not ready. Complete browser login first."));
    }
    if output.stdout.len()>65536{return Err("CLI credential response exceeded limit".into());}
    String::from_utf8(output.stdout).map_err(|_|"CLI response is not UTF-8".into())
}
fn parse_workspace_credentials(text:&str)->Result<(String,String,String),String>{
    // gws prints notices before a JSON object on some versions. Never return
    // exported credential contents to a frontend response or the model.
    let start=text.find('{').ok_or("Google Workspace CLI did not export JSON credentials")?;
    let value:Value=serde_json::from_str(&text[start..])
        .map_err(|_|"Google Workspace CLI credential export is not JSON")?;
    let take=|key|value[key].as_str().unwrap_or("").to_string();
    let id=take("client_id");let secret=take("client_secret");
    let refresh=take("refresh_token");
    if id.is_empty()||refresh.is_empty()||id.contains("...")||
        refresh.contains("...")||secret.contains("..."){
        return Err("Google Workspace CLI export is masked/incomplete; reauthorize the official CLI".into());
    }
    Ok((id,secret,refresh))
}
/// Import a completed official CLI browser session into the existing local
/// vault so chat, native jobs and Rust SDK tools share one account context.
pub async fn adopt_cli_grant(state:&AppState,source:&str)->Result<Value,String>{
    match source {
        "github"=>{
            let client=oauth_client(state)?;
            let token=cli_output("gh",&["auth","token","--hostname","github.com"]).await?
                .trim().to_string();
            if token.len()<12||token.len()>8192{return Err("GitHub CLI token is invalid".into());}
            let response=client
                .get("https://api.github.com/user").bearer_auth(&token)
                .header("User-Agent","DOMINION").timeout(Duration::from_secs(15))
                .send().await.map_err(|_|"GitHub account validation failed")?;
            if !response.status().is_success(){
                return Err(format!("GitHub CLI grant is not usable: {}",response.status()));
            }
            let scopes=response.headers().get("x-oauth-scopes")
                .and_then(|v|v.to_str().ok()).unwrap_or("").to_string();
            let info:Value=response.json().await.map_err(|_|"Invalid GitHub account response")?;
            let name=info["login"].as_str().ok_or("GitHub account identity missing")?;
            // Do not assume an imported GitHub token has all repo permissions.
            state.save_secret("oauth:github:token",&json!({
                "accessToken":token,"scopes":scopes,
                "obtainedAt":chrono::Utc::now().to_rfc3339()
            }).to_string());
            let mut record=config(state,"github").unwrap_or_else(||json!({"id":"github"}));
            record["connected"]=json!(true);
            record["authSource"]=json!("github_cli");
            record["account"]=json!(name);
            record["scopes"]=json!(scopes);
            record["connectedAt"]=json!(chrono::Utc::now().to_rfc3339());
            let saved=state.store.put("oauth_config",&record)
                .ok_or("GitHub CLI account save failed")?;
            state.bus.record("oauth_config",&saved);
            Ok(json!({"provider":"github","account":name,"connected":true}))
        }
        "google_workspace"=>{
            let client=oauth_client(state)?;
            let exported=cli_output("gws",&["auth","export","--unmasked"]).await?;
            let (id,secret,refresh)=parse_workspace_credentials(&exported)?;
            // Validate the actual grant by refreshing it. Merely finding a
            // gws credentials file is not proof of Workspace API permission.
            let response=client
                .post("https://oauth2.googleapis.com/token")
                .form(&[("client_id",id.as_str()),("client_secret",secret.as_str()),
                         ("refresh_token",refresh.as_str()),("grant_type","refresh_token")])
                .timeout(Duration::from_secs(20)).send().await
                .map_err(|_|"Google CLI credential verification failed")?;
            if !response.status().is_success(){
                return Err(format!("Google Workspace CLI grant cannot refresh (HTTP {})",response.status()));
            }
            let grant:Value=response.json().await.map_err(|_|"Google token response invalid")?;
            let access=grant["access_token"].as_str().filter(|v|!v.is_empty())
                .ok_or("Google refreshed access token missing")?.to_string();
            // Inspect the provider's actual scope report; never infer grants
            // from the application's requested scope list.
            let introspection=client
                .get("https://oauth2.googleapis.com/tokeninfo")
                .query(&[("access_token",access.as_str())])
                .timeout(Duration::from_secs(15)).send().await
                .map_err(|_|"Google OAuth permissions could not be checked")?;
            if !introspection.status().is_success(){
                return Err("Google OAuth permission inspection failed".into());
            }
            let details:Value=introspection.json().await
                .map_err(|_|"Google OAuth permission response invalid")?;
            let scopes=details["scope"].as_str().filter(|s|!s.is_empty())
                .ok_or("Google did not report granted scopes")?.to_string();
            let profile=client
                .get("https://www.googleapis.com/oauth2/v3/userinfo")
                .bearer_auth(&access).timeout(Duration::from_secs(15))
                .send().await.map_err(|_|"Google identity lookup failed")?;
            if !profile.status().is_success(){
                return Err("Google account identity permission was not granted".into());
            }
            let identity:Value=profile.json().await.map_err(|_|"Google identity response invalid")?;
            let email=identity["email"].as_str().filter(|s|!s.is_empty())
                .ok_or("Google account email missing from granted identity scope")?.to_string();
            // Tokens and application credentials are only retained in the
            // pre-existing encrypted vault, never the public oauth_config DB.
            state.save_secret("oauth:google:secret",&json!({
                "clientId":id,"clientSecret":secret
            }).to_string());
            state.save_secret("oauth:google:token",&json!({
                "accessToken":access,"refreshToken":refresh,"scopes":scopes,
                "expiresAt":chrono::Utc::now().timestamp()
                  +grant["expires_in"].as_i64().unwrap_or(3600),
                "obtainedAt":chrono::Utc::now().to_rfc3339()
            }).to_string());
            let mut record=config(state,"google").unwrap_or_else(||json!({"id":"google"}));
            record["connected"]=json!(true);
            record["authSource"]=json!("gws_cli");
            record["account"]=json!(email);
            record["scopes"]=json!(scopes);
            record["connectedAt"]=json!(chrono::Utc::now().to_rfc3339());
            let saved=state.store.put("oauth_config",&record)
                .ok_or("Google Workspace account save failed")?;
            state.bus.record("oauth_config",&saved);
            Ok(json!({"provider":"google","account":email,"connected":true}))
        }
        _=>Err("Unsupported desktop account provider".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-oauth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 4999,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn cli_google_export_rejects_masked_or_missing_secrets(){
        assert!(parse_workspace_credentials(r#"{"client_id":"id","client_secret":"masked...","refresh_token":"real"}"#).is_err());
        assert!(parse_workspace_credentials(r#"{"client_id":"id","refresh_token":null}"#).is_err());
        assert!(parse_workspace_credentials("notice\n{\"client_id\":\"id\",\"client_secret\":\"secret\",\"refresh_token\":\"refresh\"}")
            .is_ok());
    }

    #[tokio::test]
    async fn retired_youtube_cannot_reuse_old_google_or_saved_grants() {
        let (state, dir) = temp_state();
        state.store.put("oauth_config", &json!({"id":"youtube","connected":true})).unwrap();
        state.save_secret("oauth:youtube:token",
            &json!({"accessToken":"old-upload-token"}).to_string());
        state.save_secret("oauth:youtube:secret",
            &json!({"clientId":"old-client","clientSecret":"old-secret"}).to_string());
        state.store.put("oauth_state", &json!({"id":"old-state","provider":"youtube"})).unwrap();
        state.save_secret("oauth-state:old-state", "old-verifier");
        assert_eq!(status(&state).len(), 2);
        assert!(provider("youtube").is_none());
        assert!(start(&state, "youtube").is_err());
        assert!(access_token(&state, "youtube").is_none());
        assert!(fresh_access_token(&state, "youtube").await.is_err());
        purge_retired_grants(&state);
        assert!(state.store.get("oauth_config", "youtube").is_none());
        assert!(state.store.get("oauth_state", "old-state").is_none());
        assert!(!state.store.has_secret("oauth:youtube:token"));
        assert!(!state.store.has_secret("oauth:youtube:secret"));
        assert!(!state.store.has_secret("oauth-state:old-state"));
        assert_eq!(status(&state).len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn start_requires_configured_credentials() {
        let (state, dir) = temp_state();
        assert!(start(&state, "github").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn authorize_url_contains_state_and_redirect() {
        let (state, dir) = temp_state();
        save(&state, "github", "client-123", "secret-456").expect("save");
        let started = start(&state, "github").expect("start");
        let url = started["url"].as_str().unwrap();
        assert!(url.starts_with("https://github.com/login/oauth/authorize"));
        assert!(url.contains("client_id=client-123"));
        assert!(url.contains("state="));
        assert!(
            url.contains("127.0.0.1%3A4999%2Fapi%2Foauth%2Fgithub%2Fcallback")
                || url.contains("127.0.0.1:4999")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn secrets_are_not_stored_in_records() {
        let (state, dir) = temp_state();
        save(&state, "github", "client-123", "super-secret").expect("save");
        let record = state
            .store
            .get("oauth_config", "github")
            .unwrap()
            .to_string();
        // The client id is public; the client secret must never appear.
        assert!(!record.contains("super-secret"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn disconnected_github_cli_grant_must_not_be_reused_from_host_cli(){
        let (state,dir)=temp_state();
        state.store.put("oauth_config",&json!({
            "id":"github","connected":true,"authSource":"github_cli",
            "account":"example-user","scopes":"repo"
        })).unwrap();
        state.save_secret("oauth:github:token",
            &json!({"accessToken":"fake-host-token"}).to_string());
        disconnect(&state,"github");
        let err=fresh_access_token(&state,"github").await.unwrap_err();
        assert!(err.contains("계정을 먼저 연결")||err.contains("disconnected"),"{err}");
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changing_oauth_app_never_inherits_previous_account_token_or_scopes(){
        let (state,dir)=temp_state();
        save(&state,"github","client-1","secret-1").unwrap();
        state.save_secret("oauth:github:token",
            &json!({"accessToken":"previous-grant","scopes":"repo"}).to_string());
        state.store.put("oauth_config",&json!({
            "id":"github","connected":true,"account":"previous-account",
            "scopes":"repo","clientId":"client-1"
        })).unwrap();
        save(&state,"github","client-2","secret-2").unwrap();
        assert!(access_token(&state,"github").is_none());
        let record=state.store.get("oauth_config","github").unwrap();
        assert_eq!(record["connected"],false);
        assert!(record["scopes"].is_null());
        assert!(record["account"].is_null());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn callback_rejects_unknown_state() {
        let (state, dir) = temp_state();
        save(&state, "github", "client-123", "secret").unwrap();
        let error = callback(&state, "github", "code", "bogus-state")
            .await
            .unwrap_err();
        assert!(error.contains("state"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
