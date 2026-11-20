use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sha2::{Digest,Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const CACHE_TTL: Duration = Duration::from_secs(120);
const MCP_IDLE_TTL: Duration = Duration::from_secs(180);
const MAX_IDLE_MCP_SESSIONS: usize = 12;

/// Only healthy, idle sessions are pooled. A leased session is owned by one
/// request, so concurrent calls never share a live JSON-RPC request stream.
/// Stdio children retain kill_on_drop, including after TTL eviction.
enum LiveSession {
    Stdio(StdioSession),
    Http(HttpSession),
}
impl LiveSession {
    async fn request(&mut self, method:&str, params:Value)->Result<Value,String> {
        match self {
            Self::Stdio(session)=>session.request(method,params).await,
            Self::Http(session)=>session.request(method,params).await,
        }
    }
}
struct IdleMcpSession {
    server_id: String,
    identity: String,
    last_used: Instant,
    session: LiveSession,
}
#[derive(Default)]
pub(crate) struct SessionPool {
    idle: Mutex<Vec<IdleMcpSession>>,
}
impl SessionPool {
    fn checkout(&self,server_id:&str,identity:&str)->Option<LiveSession> {
        let mut idle=self.idle.lock().unwrap();
        // Config changes, token rotation and TTL all invalidate a saved
        // process/session. No bearer credentials are written to disk.
        idle.retain(|entry|entry.last_used.elapsed()<MCP_IDLE_TTL &&
            (entry.server_id!=server_id || entry.identity==identity));
        let index=idle.iter().rposition(|entry|entry.identity==identity)?;
        Some(idle.swap_remove(index).session)
    }
    fn checkin(&self,server_id:String,identity:String,session:LiveSession) {
        let mut idle=self.idle.lock().unwrap();
        idle.retain(|entry|entry.last_used.elapsed()<MCP_IDLE_TTL &&
            (entry.server_id!=server_id || entry.identity==identity));
        if idle.len()>=MAX_IDLE_MCP_SESSIONS {
            idle.remove(0);
        }
        idle.push(IdleMcpSession{server_id,identity,
            last_used:Instant::now(),session});
    }
    pub(crate) fn clear(&self) {
        self.idle.lock().unwrap().clear();
    }
}

/// A short-lived, per-engine pool key tied to the current MCP record, local
/// workspace, and current scoped secret. Only a digest is retained as a key.
fn connection_identity(state:&AppState,server:&Value)->String {
    let id=server["id"].as_str().unwrap_or("");
    let credential=state.read_secret(&format!("mcp:{id}")).unwrap_or_default();
    let credential_hash=format!("{:x}",Sha256::digest(credential.as_bytes()));
    let identity=json!({"engine":state.instance_id,"server":server,
        "workspace":state.config.workspace.to_string_lossy(),
        "credentialHash":credential_hash});
    format!("{:x}",Sha256::digest(identity.to_string().as_bytes()))
}
async fn checkout_session(state:&AppState,server:&Value)
    ->Result<(String,LiveSession),String> {
    let id=server["id"].as_str().unwrap_or("").to_string();
    let identity=connection_identity(state,server);
    if let Some(session)=state.integrations.mcp_sessions.checkout(&id,&identity){
        return Ok((identity,session));
    }
    let session=if server["transport"].as_str()==Some("http") {
        LiveSession::Http(HttpSession::open(server,state).await?)
    }else{
        LiveSession::Stdio(StdioSession::open(server,&state.config.workspace).await?)
    };
    Ok((identity,session))
}
fn return_session(state:&AppState,server:&Value,identity:String,session:LiveSession) {
    // Do not return a session whose endpoint or secret changed during the call.
    if identity==connection_identity(state,server) &&
        state.store.get("mcp",server["id"].as_str().unwrap_or(""))
            .as_ref()==Some(server) {
        state.integrations.mcp_sessions.checkin(server["id"].as_str().unwrap_or("").to_string(),
            identity,session);
    }
}


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

pub fn invalidate(state:&AppState) {
    cache().lock().unwrap().clear();
    state.integrations.mcp_sessions.clear();
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
    for server in enabled_servers(state) {
        let id = server
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let identity=connection_identity(state,&server);
        if let Some((at, tools)) = cache().lock().unwrap().get(&identity) {
            if at.elapsed() < CACHE_TTL {
                collected.extend(tools.clone());
                continue;
            }
        }
        match list_server(state, &server).await {
            Ok(tools) => {
                cache()
                    .lock()
                    .unwrap()
                    .insert(identity, (Instant::now(), tools.clone()));
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

async fn list_server(state: &AppState, server: &Value) -> Result<Vec<McpTool>, String> {
    let server_name = server
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("mcp")
        .to_string();
    let server_slug = slug(&server_name);
    let (identity,mut session)=checkout_session(state,server).await?;
    let outcome=session.request("tools/list",json!({})).await;
    if outcome.is_ok(){return_session(state,server,identity,session);}
    let result=outcome?;

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
                slug: server_slug.clone(),
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
        .find(|server| {
            slug(
                server
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("mcp"),
            ) == server_slug
        })
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

    let server = enabled_servers(state)
        .into_iter()
        .find(|server| {
            slug(
                server
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("mcp"),
            ) == server_slug
        })
        .ok_or_else(|| format!("MCP 서버를 찾을 수 없습니다: {server_slug}"))?;

    let params = json!({ "name": tool_name, "arguments": args });
    let (identity,mut session)=checkout_session(state,&server).await?;
    let result=session.request("tools/call",params).await;
    // A failed call may have applied an effect. Never replay it: discard the
    // possibly broken session and return its error to the Rust agent journal.
    if result.is_ok(){return_session(state,&server,identity,session);}
    result
}

/// Bind the complete MCP server identity and its approved tool declaration.
/// This is calculated once before native execution and checked again at each
/// dispatched action. An API token is not placed in the job record.
pub(crate) fn native_contract_fingerprint(
 state:&AppState,qualified:&str,schema:&Value,description:&str
)->Result<String,String>{
 let server_slug=qualified.strip_prefix("mcp__")
  .and_then(|rest|rest.split_once("__").map(|(slug,_)|slug))
  .ok_or("Invalid MCP-qualified tool name")?;
 let servers=enabled_servers(state).into_iter().filter(|v|
  slug(v["name"].as_str().unwrap_or("mcp"))==server_slug)
  .collect::<Vec<_>>();
 if servers.len()!=1{return Err("Ambiguous or unavailable MCP server identity".into())}
 let data=json!({"server":servers[0],"qualified":qualified,
   "schema":schema,"description":description});
 Ok(format!("{:x}",Sha256::digest(data.to_string().as_bytes())))
}

/// Validate an uncached tools/list response from the SAME MCP session that
/// will execute tools/call. A 120-second discovery cache is never authority
/// to dispatch a previously approved but changed external capability.
fn verify_pinned_tool(list:&Value,name:&str,schema:&Value,description:&str)->Result<(),String>{
 let available=list["tools"].as_array()
  .ok_or("MCP tools/list did not contain a tool array")?;
 let matched=available.iter().filter(|item|item["name"].as_str()==Some(name))
  .collect::<Vec<_>>();
 if matched.len()!=1{return Err("MCP tool missing or duplicated during pinned dispatch".into())}
 let live_schema=matched[0].get("inputSchema")
  .cloned().unwrap_or_else(||json!({"type":"object","properties":{}}));
 let live_description=matched[0]["description"].as_str().unwrap_or("");
 if &live_schema!=schema||live_description!=description{
  return Err("MCP tool schema or description changed after owner-approved catalog; review required".into())
 }
 Ok(())
}

/// Approval and the last-minute manifest check happen in the product Rust
/// host. The tools/list and tools/call are then made on one MCP session.
pub(crate) async fn call_pinned(state:&AppState,qualified:&str,args:&Value,
 schema:&Value,description:&str)->Result<Value,String>{
 let (server_slug,tool_name)=qualified.strip_prefix("mcp__")
  .and_then(|rest|rest.split_once("__"))
  .ok_or("Cannot parse pinned MCP tool name")?;
 let servers=enabled_servers(state).into_iter().filter(|server|
  slug(server["name"].as_str().unwrap_or("mcp"))==server_slug).collect::<Vec<_>>();
 if servers.len()!=1{return Err("Ambiguous or unavailable MCP server identity".into())}
 let server=&servers[0];
 let params=json!({"name":tool_name,"arguments":args});
 let (identity,mut session)=checkout_session(state,server).await?;
 // The latest capability manifest and effect execution stay on the SAME
 // leased session, even when it originated in the idle pool.
 let outcome=async {
  let fresh=session.request("tools/list",json!({})).await?;
  verify_pinned_tool(&fresh,tool_name,schema,description)?;
  session.request("tools/call",params).await
 }.await;
 if outcome.is_ok(){return_session(state,server,identity,session);}
 outcome
}
#[cfg(test)]
mod pinned_tests{
 use super::*;
 #[test]
 fn same_session_manifest_rejects_changed_schema_duplicate_and_description(){
  let schema=json!({"type":"object","required":["id"],
   "properties":{"id":{"type":"string"}}});
  let original=json!({"tools":[{"name":"publish","description":"Publish safely",
   "inputSchema":schema}]});
  assert!(verify_pinned_tool(&original,"publish",&schema,"Publish safely").is_ok());
  let changed=json!({"tools":[{"name":"publish","description":"Publish safely",
   "inputSchema":{"type":"object"}}]});
  assert!(verify_pinned_tool(&changed,"publish",&schema,"Publish safely").is_err());
  assert!(verify_pinned_tool(&original,"publish",&schema,"Not the reviewed description").is_err());
  assert!(verify_pinned_tool(&json!({"tools":[original["tools"][0],original["tools"][0]]}),
    "publish",&schema,"Publish safely").is_err());
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
    async fn open(record: &Value, state: &AppState) -> Result<Self, String> {
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
        let id = record
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let token = state.read_secret(&format!("mcp:{id}"));
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
        session.notify("notifications/initialized", json!({})).await?;
        Ok(session)
    }

    async fn notify(&mut self, method: &str, params: Value)->Result<(),String> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        // 202 + empty body is valid; 401/403/404 must not enter the pool.
        self.post(&payload).await.map(|_|())
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
    fn pinned_native_contract_changes_on_server_reconfiguration_and_slug_collision(){
        let state=temp_state();
        let schema=json!({"type":"object","required":["value"],
            "properties":{"value":{"type":"string"}}});
        let server=json!({"id":"mcp-a","name":"Echo Lab","transport":"http",
            "url":"http://127.0.0.1:12112/mcp","enabled":true});
        state.store.put("mcp",&server).unwrap();
        let first=native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").unwrap();
        let second=native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").unwrap();
        assert_eq!(first,second);
        let mut changed=server.clone();
        changed["url"]=json!("http://127.0.0.1:12113/mcp");
        state.store.put("mcp",&changed).unwrap();
        let changed=native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").unwrap();
        assert_ne!(first,changed,"reconfigured endpoints cannot inherit approval");
        state.store.put("mcp",&json!({"id":"mcp-b","name":"Echo-Lab",
            "transport":"http","enabled":true,"url":"http://127.0.0.1:9999"})).unwrap();
        assert!(native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").is_err(),"duplicate namespaces must fail closed");
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

    #[tokio::test]
    async fn http_reuses_mcp_session_and_reconnects_after_credential_rotation() {
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        use tokio::io::{AsyncReadExt,AsyncWriteExt,AsyncBufReadExt};
        let state=temp_state();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let starts=Arc::new(AtomicUsize::new(0));
        let calls=Arc::new(AtomicUsize::new(0));
        let starts_task=starts.clone();
        let calls_task=calls.clone();
        let server_task=tokio::spawn(async move {
            loop {
                let Ok((stream,_))=listener.accept().await else {break;};
                let starts=starts_task.clone();
                let calls=calls_task.clone();
                tokio::spawn(async move {
                    let (reader,mut writer)=stream.into_split();
                    let mut reader=tokio::io::BufReader::new(reader);
                    loop {
                        let mut line=String::new();
                        if reader.read_line(&mut line).await.unwrap_or(0)==0 {break;}
                        if !line.starts_with("POST ") {break;}
                        let mut body_len=0usize;
                        let mut session_header=false;
                        loop {
                            let mut header=String::new();
                            if reader.read_line(&mut header).await.unwrap_or(0)==0 {return;}
                            if header.trim().is_empty(){break;}
                            let low=header.to_ascii_lowercase();
                            if let Some(value)=low.strip_prefix("content-length:"){
                                body_len=value.trim().parse().unwrap_or(0);
                            }
                            if low.starts_with("mcp-session-id: test-session") {
                                session_header=true;
                            }
                        }
                        if body_len==0 || body_len>65536 {return;}
                        let mut body=vec![0;body_len];
                        if reader.read_exact(&mut body).await.is_err(){return;}
                        let request:Value=serde_json::from_slice(&body).unwrap();
                        let method=request["method"].as_str().unwrap_or("");
                        let valid=method=="initialize" || session_header;
                        let response=match method {
                            "initialize"=>{
                                starts.fetch_add(1,Ordering::SeqCst);
                                json!({"jsonrpc":"2.0","id":request["id"],"result":{
                                    "protocolVersion":"2024-11-05",
                                    "capabilities":{"tools":{}},
                                    "serverInfo":{"name":"mock-mcp","version":"1"}
                                }})
                            }
                            "notifications/initialized"=>json!({}),
                            "tools/list"=>json!({"jsonrpc":"2.0","id":request["id"],
                                "result":{"tools":[{"name":"echo","description":"HTTP Echo",
                                    "inputSchema":{"type":"object"}}]}}),
                            "tools/call"=>{
                                calls.fetch_add(1,Ordering::SeqCst);
                                json!({"jsonrpc":"2.0","id":request["id"],
                                    "result":{"content":[{"type":"text",
                                        "text":request["params"]["arguments"]["text"]}]}})
                            }
                            _=>json!({"jsonrpc":"2.0","id":request["id"],
                                "error":{"code":-32601,"message":"unknown"}}),
                        };
                        let payload=if valid {response.to_string()}
                            else {r#"{"error":"missing MCP session id"}"#.to_string()};
                        let status=if valid {"200 OK"}else{"400 Bad Request"};
                        let raw=format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nmcp-session-id: test-session\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n{}",
                            payload.len(),payload);
                        if writer.write_all(raw.as_bytes()).await.is_err(){return;}
                    }
                });
            }
        });
        let server=json!({"id":"mcp-http-pool","name":"HTTP Lab","transport":"http",
            "url":format!("http://127.0.0.1:{port}/mcp"),"enabled":true});
        state.store.put("mcp",&server).unwrap();
        state.save_secret("mcp:mcp-http-pool","first-secret");
        let specs=tools(&state).await;
        assert_eq!(specs.len(),1);
        let qualified=specs[0].qualified();
        for text in ["one","two"] {
            let value=tokio::time::timeout(Duration::from_secs(10),
                call_pinned(&state,&qualified,&json!({"text":text}),
                    &specs[0].schema,&specs[0].description))
                .await.expect("HTTP MCP call stalled").unwrap();
            assert!(value.to_string().contains(text));
        }
        assert_eq!(starts.load(Ordering::SeqCst),1,
            "MCP HTTP initialization should be amortized across discovery and calls");
        assert_eq!(calls.load(Ordering::SeqCst),2);

        // Changing the scoped credential must not reuse a stale bearer session.
        state.save_secret("mcp:mcp-http-pool","second-secret");
        call(&state,&qualified,&json!({"text":"rotated"})).await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst),2);
        assert_eq!(calls.load(Ordering::SeqCst),3);
        invalidate(&state);
        call(&state,&qualified,&json!({"text":"reconnected"})).await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst),3);
        assert_eq!(calls.load(Ordering::SeqCst),4);
        server_task.abort();
    }

    #[tokio::test]
    async fn stdio_reuses_one_live_mcp_process_and_rotates_on_config_change() {
        // The shipped fixture is a real stdio MCP child, not a fake
        // transport. Hosted CI installs Node before the full test matrix.
        if std::process::Command::new("node").arg("--version").output().is_err() {
            eprintln!("node unavailable; stdio fixture skipped");
            return;
        }
        let state=temp_state();
        let script=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/mcp-echo-server.mjs");
        assert!(script.is_file(),"missing real MCP stdio fixture");
        let log=state.config.data_dir.join("mcp-initializations.log");
        let server=json!({
            "id":"mcp-pool-echo","name":"Echo Lab","enabled":true,
            "transport":"stdio","command":"node",
            "args":[script.to_string_lossy()],
            "env":{"DOMINION_MCP_TEST_INIT_LOG":log.to_string_lossy()}
        });
        state.store.put("mcp",&server).unwrap();
        let tools=tools(&state).await;
        assert_eq!(tools.len(),1);
        let qualified=tools[0].qualified();
        for text in ["first","second"] {
            let result=tokio::time::timeout(Duration::from_secs(10),
                call(&state,&qualified,&json!({"text":text})))
                .await.expect("MCP response timed out").unwrap();
            assert!(result.to_string().contains(&format!("MCP_ECHO: {text}")));
        }
        // The first discovery and two tool calls all borrowed the same
        // long-lived child; an idle stdio server must not restart per call.
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),1);
        let pinned=tokio::time::timeout(Duration::from_secs(10),call_pinned(
            &state,&qualified,&json!({"text":"verified"}),
            &tools[0].schema,&tools[0].description,
        )).await.expect("pinned MCP response timed out").unwrap();
        assert!(pinned.to_string().contains("MCP_ECHO: verified"));
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),1);
        let cloned=state.clone();
        call(&cloned,&qualified,&json!({"text":"shared"})).await.unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),1);

        let mut changed=server.clone();
        changed["env"]["DOMINION_MCP_TEST_CONFIG"]=json!("reconfigured");
        state.store.put("mcp",&changed).unwrap();
        invalidate(&state);
        call(&state,&qualified,&json!({"text":"after-config"})).await.unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),2);

        // A rotated secret changes the pool identity even if the public
        // server record was not edited.
        state.save_secret("mcp:mcp-pool-echo","rotated");
        call(&state,&qualified,&json!({"text":"after-rotation"})).await.unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),3);
        assert!(state.integrations.mcp_sessions.idle.lock().unwrap().len()<=1);
        drop(cloned);
        drop(state);
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
