use crate::state::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Mutex as AsyncMutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const CACHE_TTL: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct McpTool {
    pub server_name: String,
    pub slug: String,
    pub tool: String,
    pub description: String,
    pub schema: Value,
}

impl McpTool {
    /// Tool name exposed to the model. Prefixed so two servers cannot collide.
    pub fn qualified(&self) -> String {
        format!("mcp__{}__{}", self.slug, self.tool)
    }
}

type Cache = Mutex<HashMap<String, (Instant, Vec<McpTool>)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn invalidate() {
    cache().lock().unwrap().clear();
    // Config or credential changes must never keep a pooled session alive.
    http_sessions().lock().unwrap().clear();
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Preserve original MCP tool names for non-colliding servers. When two
/// distinct display names normalize to the same identifier, use a stable
/// server-ID suffix instead of silently routing the wrong account's call.
fn server_namespace(server:&Value, servers:&[Value]) -> String {
    let mut name=slug(server["name"].as_str().unwrap_or("mcp"));
    if name.is_empty() {name="mcp".to_string();}
    let same=servers.iter().filter(|item|{
        let mut candidate=slug(item["name"].as_str().unwrap_or("mcp"));
        if candidate.is_empty(){candidate="mcp".to_string();}
        candidate==name
    }).count();
    if same<=1 {return name;}
    let identity=server["id"].as_str().unwrap_or("");
    let digest=format!("{:x}",Sha256::digest(identity.as_bytes()));
    format!("{}_{}",name,&digest[..16])
}

fn enabled_servers(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("mcp")
        .into_iter()
        .filter(|record| record.get("enabled").and_then(|value| value.as_bool()) != Some(false))
        .collect()
}

/// Fetch (and cache) the tool list of every enabled MCP server.
pub async fn tools(state: &AppState) -> Vec<McpTool> {
    let mut collected = Vec::new();
    let servers=enabled_servers(state);
    for server in &servers {
        let id = server
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let namespace=server_namespace(server,&servers);
        // Namespace is part of the cache key so adding a colliding server
        // cannot reuse a stale definition with its old unqualified name.
        let cache_key=format!("{id}:{namespace}");
        if let Some((at, tools)) = cache().lock().unwrap().get(&cache_key) {
            if at.elapsed() < CACHE_TTL {
                collected.extend(tools.clone());
                continue;
            }
        }
        match list_server(state, server, &namespace).await {
            Ok(tools) => {
                cache()
                    .lock()
                    .unwrap()
                    .insert(cache_key, (Instant::now(), tools.clone()));
                collected.extend(tools);
            }
            Err(error) => {
                state
                    .store
                    .audit("mcp.list_failed", json!({ "server": id, "error": error }));
            }
        }
    }
    collected
}

async fn list_server(state: &AppState, server: &Value, server_slug:&str) -> Result<Vec<McpTool>, String> {
    let server_name = server
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("mcp")
        .to_string();
    let transport = server
        .get("transport")
        .and_then(|value| value.as_str())
        .unwrap_or("stdio");

    let result = if transport == "http" {
        http_request(state,server,"tools/list",json!({})).await?
    } else {
        let mut session = StdioSession::open(server, &state.config.workspace).await?;
        let result = session.request("tools/list", json!({})).await;
        drop(session);
        result?
    };

    let tools = result
        .get("tools")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(tools
        .into_iter()
        .filter_map(|tool| {
            let name = tool
                .get("name")
                .and_then(|value| value.as_str())?
                .to_string();
            Some(McpTool {
                server_name: server_name.clone(),
                slug: server_slug.to_string(),
                tool: name,
                description: tool
                    .get("description")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            })
        })
        .collect())
}

pub async fn definitions(state: &AppState) -> Vec<Value> {
    tools(state)
        .await
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.qualified(),
                    "description": format!("[MCP · {}] {}", tool.server_name, tool.description),
                    "parameters": tool.schema,
                }
            })
        })
        .collect()
}

pub fn requires_approval(state: &AppState, qualified: &str) -> bool {
    let policy = state
        .store
        .get("policy", "default")
        .unwrap_or_else(|| json!({}));
    if policy
        .get("autoApproveMcp")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return false;
    }
    let qualified = qualified.to_string();
    let server_slug = qualified
        .strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
        .map(|(slug, _)| slug.to_string())
        .unwrap_or_default();
    let servers = enabled_servers(state);
    servers
        .iter()
        .find(|server| server_namespace(server,&servers)==server_slug)
        .and_then(|server| server.get("approval").and_then(|value| value.as_bool()))
        .unwrap_or(true)
}

/// Executes `mcp__{server}__{tool}`.
pub async fn call(state: &AppState, qualified: &str, args: &Value) -> Result<Value, String> {
    let rest = qualified
        .strip_prefix("mcp__")
        .ok_or_else(|| format!("MCP 도구 이름이 아닙니다: {qualified}"))?;
    let (server_slug, tool_name) = rest
        .split_once("__")
        .ok_or_else(|| format!("MCP 도구 이름을 해석할 수 없습니다: {qualified}"))?;

    let servers=enabled_servers(state);
    let server=servers.iter()
        .find(|server|server_namespace(server,&servers)==server_slug)
        .ok_or_else(|| format!("MCP 서버를 찾을 수 없습니다: {server_slug}"))?;

    let transport = server
        .get("transport")
        .and_then(|value| value.as_str())
        .unwrap_or("stdio");
    let params = json!({ "name": tool_name, "arguments": args });

    if transport == "http" {
        http_request(state,server,"tools/call",params).await
    } else {
        let mut session = StdioSession::open(server, &state.config.workspace).await?;
        let result = session.request("tools/call", params).await;
        drop(session);
        result
    }
}

// ------------------------------------------------------------------ stdio

struct StdioSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl StdioSession {
    async fn open(record: &Value, workspace: &Path) -> Result<Self, String> {
        let command = record
            .get("command")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or("MCP stdio 서버에 command 가 없습니다.")?;
        let args = record
            .get("args")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut child = Command::new(command);
        child
            .args(&args)
            .current_dir(workspace)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(env) = record.get("env").and_then(|value| value.as_object()) {
            for (key, value) in env {
                if let Some(value) = value.as_str() {
                    child.env(key, value);
                }
            }
        }

        let mut child = child
            .spawn()
            .map_err(|error| format!("MCP 서버 실행 실패 ({command}): {error}"))?;
        let stdin = child.stdin.take().ok_or("MCP stdin 을 열 수 없습니다.")?;
        let stdout = child.stdout.take().ok_or("MCP stdout 을 열 수 없습니다.")?;
        drop(child.stderr.take());

        let mut session = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        };
        session
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "dominion-rust", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        session
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(session)
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let payload = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.send(&payload).await
    }

    async fn send(&mut self, payload: &Value) -> Result<(), String> {
        self.stdin
            .write_all(format!("{payload}\n").as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        self.stdin.flush().await.map_err(|error| error.to_string())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&payload).await?;

        loop {
            let mut line = String::new();
            let read =
                tokio::time::timeout(Duration::from_secs(40), self.stdout.read_line(&mut line))
                    .await
                    .map_err(|_| format!("MCP 응답 시간 초과: {method}"))?
                    .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("MCP 서버가 응답 없이 종료되었습니다.".into());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };
            if value.get("id").and_then(|item| item.as_u64()) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(format!("MCP 오류: {error}"));
            }
            return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
        }
    }
}

// ------------------------------------------------------------------- http

// HTTP sessions are scoped by the complete server config, vault credential,
// and workspace. Never share a token across URLs or accounts. The pool is
// demand-driven (no resident network work) and bounded by idle expiry and size.
const HTTP_IDLE_TTL:Duration=Duration::from_secs(120);
const MAX_HTTP_SESSIONS:usize=16;
type HttpHandle=Arc<AsyncMutex<Option<HttpSession>>>;
struct HttpEntry{last_used:Instant,handle:HttpHandle}
type HttpPool=Mutex<HashMap<String,HttpEntry>>;
fn http_sessions()->&'static HttpPool{
    static POOL:OnceLock<HttpPool>=OnceLock::new();
    POOL.get_or_init(||Mutex::new(HashMap::new()))
}
fn http_identity(state:&AppState,record:&Value)->Result<(String,Option<String>),String>{
    let id=record["id"].as_str().filter(|s|!s.is_empty())
        .ok_or("MCP 서버 ID가 없습니다.")?;
    let token=state.read_secret(&format!("mcp:{id}"));
    let mut digest=Sha256::new();
    digest.update(record.to_string().as_bytes());
    digest.update([0]);
    digest.update(state.config.workspace.to_string_lossy().as_bytes());
    digest.update([0]);
    if let Some(secret)=&token{digest.update(secret.as_bytes());}
    Ok((format!("{:x}",digest.finalize()),token))
}
/// If a future is aborted while an external tool is in-flight, do not lend
/// its session to another call. Unknown external effects are never replayed
/// automatically: a later request must establish a fresh connection.
struct HttpLease{key:String,handle:HttpHandle,valid:bool}
impl Drop for HttpLease{
    fn drop(&mut self){
        if self.valid{return;}
        let mut pool=http_sessions().lock().unwrap();
        if pool.get(&self.key).is_some_and(|entry|
            Arc::ptr_eq(&entry.handle,&self.handle)){
            pool.remove(&self.key);
        }
    }
}
async fn http_request(state:&AppState,record:&Value,method:&str,params:Value)
    ->Result<Value,String>{
    let (key,token)=http_identity(state,record)?;
    let handle={
        let mut pool=http_sessions().lock().unwrap();
        pool.retain(|_,entry|entry.last_used.elapsed()<HTTP_IDLE_TTL);
        if let Some(entry)=pool.get_mut(&key){
            entry.last_used=Instant::now();
            entry.handle.clone()
        }else{
            if pool.len()>=MAX_HTTP_SESSIONS{
                if let Some(oldest)=pool.iter()
                    .min_by_key(|(_,entry)|entry.last_used)
                    .map(|(key,_)|key.clone()){
                    pool.remove(&oldest);
                }
            }
            let handle=Arc::new(AsyncMutex::new(None));
            pool.insert(key.clone(),HttpEntry{
                last_used:Instant::now(),handle:handle.clone()
            });
            handle
        }
    };
    // Inner mutex serializes only requests to this server+credential session;
    // independent servers can work in parallel.
    let mut lease=HttpLease{key,handle:handle.clone(),valid:false};
    let mut slot=handle.lock().await;
    if slot.is_none(){
        *slot=Some(HttpSession::open(record,token).await?);
    }
    let result=slot.as_mut().ok_or("MCP HTTP 세션 초기화 실패")?
        .request(method,params).await;
    lease.valid=result.is_ok();
    result
}

struct HttpSession {
    client: reqwest::Client,
    url: String,
    session: Option<String>,
    token: Option<String>,
    next_id: u64,
}

impl HttpSession {
    /// Full MCP handshake: initialize, capture Mcp-Session-Id, then
    /// notifications/initialized. Stateless one-shot requests fail against
    /// spec-compliant Streamable HTTP servers.
    async fn open(record: &Value, token: Option<String>) -> Result<Self, String> {
        let url = record
            .get("url")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or("MCP HTTP 서버에 url 이 없습니다.")?
            .to_string();
        let parsed = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("MCP URL 은 HTTP(S)여야 합니다.".into());
        }
        let mut session = Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|error| error.to_string())?,
            url,
            session: None,
            token,
            next_id: 1,
        };

        session
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "dominion-rust", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        session.notify("notifications/initialized", json!({})).await;
        Ok(session)
    }

    async fn notify(&mut self, method: &str, params: Value) {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        // A notification may legitimately return 202 with an empty body.
        let _ = self.post(&payload).await;
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let value = self.post(&payload).await?;
        if let Some(error) = value.get("error") {
            return Err(format!("MCP 오류: {error}"));
        }
        Ok(value.get("result").cloned().unwrap_or_else(|| json!({})))
    }

    async fn post(&mut self, payload: &Value) -> Result<Value, String> {
        let mut request = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .json(payload);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        if let Some(token) = &self.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }

        let response = request.send().await.map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!(
                "MCP HTTP {status}: {}",
                text.chars().take(400).collect::<String>()
            ));
        }
        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            self.session = Some(session.to_string());
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = response.text().await.map_err(|error| error.to_string())?;
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        if content_type.contains("text/event-stream") {
            for block in text.split("\n\n") {
                let payload = block
                    .lines()
                    .filter(|line| line.starts_with("data:"))
                    .map(|line| line[5..].trim_start())
                    .collect::<Vec<_>>()
                    .join("\n");
                if payload.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(&payload) {
                    if value.get("result").is_some() || value.get("error").is_some() {
                        return Ok(value);
                    }
                }
            }
            return Ok(json!({}));
        }
        serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-mcp-{}", uuid::Uuid::new_v4()));
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
    fn authenticated_http_pool_isolated_by_server_url_and_credentials(){
        let state=temp_state();
        let first=json!({"id":"mcp-a","transport":"http","url":"https://a.example/mcp"});
        let second=json!({"id":"mcp-b","transport":"http","url":"https://b.example/mcp"});
        let (initial,_) = http_identity(&state,&first).unwrap();
        let (different,_) = http_identity(&state,&second).unwrap();
        assert_ne!(initial,different);
        state.save_secret("mcp:mcp-a","token-one");
        let (changed,token) = http_identity(&state,&first).unwrap();
        assert_eq!(token.as_deref(),Some("token-one"));
        assert_ne!(initial,changed);
        state.save_secret("mcp:mcp-a","token-two");
        assert_ne!(changed,http_identity(&state,&first).unwrap().0);
        // Simulate an abandoned tool call: its pool lease must be evicted.
        let key=changed.clone();
        let handle=Arc::new(AsyncMutex::new(None));
        http_sessions().lock().unwrap().insert(key.clone(),HttpEntry{
            last_used:Instant::now(),handle:handle.clone()
        });
        {
            let _unfinished=HttpLease{key:key.clone(),handle,valid:false};
        }
        assert!(!http_sessions().lock().unwrap().contains_key(&key));
    }
    #[test]
    fn qualified_names_are_namespaced() {
        let tool = McpTool {
            server_name: "Echo Lab".into(),
            slug: slug("Echo Lab"),
            tool: "echo".into(),
            description: String::new(),
            schema: json!({}),
        };
        assert_eq!(slug("Echo Lab"), "echo_lab");
        assert_eq!(tool.qualified(), "mcp__echo_lab__echo");
    }

    #[test]
    fn collision_renames_only_ambiguous_server_namespaces() {
        let first=json!({"id":"one","name":"Search Lab","approval":false});
        let second=json!({"id":"two","name":"Search-Lab","approval":true});
        let lone=vec![first.clone()];
        assert_eq!(server_namespace(&first,&lone),"search_lab");
        let both=vec![first.clone(),second.clone()];
        let a=server_namespace(&first,&both);
        let b=server_namespace(&second,&both);
        assert_ne!(a,b);
        assert!(a.starts_with("search_lab_"));
        assert!(b.starts_with("search_lab_"));
        assert_eq!(a,server_namespace(&first,&both));
    }

    #[test]
    fn colliding_mcp_servers_retain_independent_approval_policies() {
        let state=temp_state();
        let first=state.store.put("mcp",&json!({"id":"mcp-one",
            "name":"Search Lab","approval":false})).unwrap();
        let second=state.store.put("mcp",&json!({"id":"mcp-two",
            "name":"Search-Lab","approval":true})).unwrap();
        let servers=enabled_servers(&state);
        let a=format!("mcp__{}__lookup",server_namespace(&first,&servers));
        let b=format!("mcp__{}__lookup",server_namespace(&second,&servers));
        assert_ne!(a,b);
        assert!(!requires_approval(&state,&a));
        assert!(requires_approval(&state,&b));
        // An ambiguous legacy alias must never silently target the first.
        assert!(requires_approval(&state,"mcp__search_lab__lookup"));
    }

    #[test]
    fn mcp_approval_follows_policy() {
        let state = temp_state();
        state.store.put(
            "mcp",
            &json!({ "name": "Echo Lab", "transport": "stdio", "command": "node" }),
        );
        assert!(requires_approval(&state, "mcp__echo_lab__echo"));

        state.store.put(
            "policy",
            &json!({ "id": "default", "autoApproveMcp": true }),
        );
        assert!(!requires_approval(&state, "mcp__echo_lab__echo"));
    }
}
