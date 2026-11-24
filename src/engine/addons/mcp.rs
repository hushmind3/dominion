#[path = "mcp_modern.rs"]
mod modern;
use crate::state::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool,Ordering};
use tokio::sync::Mutex as AsyncMutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const CACHE_TTL: Duration = Duration::from_secs(120);
const MAX_STDIO_FRAME_BYTES:usize=4*1024*1024;

#[derive(Clone)]
pub struct McpTool {
    pub server_name: String,
    pub slug: String,
    pub tool: String,
    pub description: String,
    pub schema: Value,
}

/// Read one newline-delimited MCP frame without allowing a child process to
/// grow memory without bound. Returns `None` only for EOF before any bytes.
async fn read_stdio_frame<R:AsyncBufRead+Unpin>(
    reader:&mut R,max_bytes:usize,
)->std::io::Result<Option<Vec<u8>>>{
    let mut frame=Vec::new();
    loop{
        let available=reader.fill_buf().await?;
        if available.is_empty(){
            return if frame.is_empty(){Ok(None)}else{Ok(Some(frame))};
        }
        let count=available.iter().position(|byte|*byte==b'\n')
            .map(|index|index+1).unwrap_or(available.len());
        if frame.len().saturating_add(count)>max_bytes{
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
                format!("MCP stdio frame exceeds {max_bytes} bytes")));
        }
        let ended=available[count-1]==b'\n';
        frame.extend_from_slice(&available[..count]);
        reader.consume(count);
        if ended{return Ok(Some(frame));}
    }
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
    modern_http_modes().lock().unwrap().clear();
    // Config or credential changes must never keep a pooled session alive.
    let mut pool=http_sessions().lock().unwrap();
    for entry in pool.values(){entry.healthy.store(false,Ordering::SeqCst);}
    pool.clear();
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

/// Discover each enabled server, including empty catalogs and failures.
/// Persisted identity is independent of potentially duplicate display names.
pub(crate) async fn discover_servers(state: &AppState)
    -> Vec<(String,String,Result<Vec<McpTool>,String>)> {
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
        let name=server["name"].as_str().unwrap_or("mcp").to_string();
        let namespace=server_namespace(server,&servers);
        // Namespace is part of the cache key so adding a colliding server
        // cannot reuse a stale definition with its old unqualified name.
        let cache_key=format!("{id}:{namespace}");
        if let Some((at, tools)) = cache().lock().unwrap().get(&cache_key) {
            if at.elapsed() < CACHE_TTL {
                collected.push((id,name,Ok(tools.clone())));
                continue;
            }
        }
        match list_server(state, server, &namespace).await {
            Ok(tools) => {
                cache()
                    .lock()
                    .unwrap()
                    .insert(cache_key, (Instant::now(), tools.clone()));
                collected.push((id,name,Ok(tools)));
            }
            Err(error) => {
                state.store.audit("mcp.list_failed", json!({"server":id,"error":error}));
                collected.push((id,name,Err("MCP discovery failed".into())));
            }
        }
    }
    collected
}

/// The provider sees actual tools only, never connection-status rows.
pub async fn tools(state:&AppState)->Vec<McpTool>{
    discover_servers(state).await.into_iter()
        .filter_map(|(_,_,result)|result.ok()).flatten().collect()
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

    let modern_http=transport=="http" && configured_http_era(server)
        .ok().flatten().or_else(||http_identity(state,server).ok()
            .and_then(|(identity,_)|modern_http_modes().lock().unwrap()
                .get(&identity).copied())).unwrap_or(false);
    let tools = result
        .get("tools")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(tools
        .into_iter()
        .filter_map(|tool| {
            if modern_http && modern::header_specs(&tool["inputSchema"]).is_err(){
                // Invalid mirrored-header annotations are not safe to expose.
                return None;
            }
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
            // Stderr is diagnostic output, never protocol data. Piping it
            // without draining lets a noisy child block once the pipe fills.
            .stderr(std::process::Stdio::null())
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
        let mut frame=serde_json::to_vec(payload).map_err(|error|error.to_string())?;
        if frame.len()>MAX_STDIO_FRAME_BYTES{
            return Err(format!("MCP stdio request exceeds {MAX_STDIO_FRAME_BYTES} bytes"));
        }
        frame.push(b'\n');
        self.stdin
            .write_all(&frame)
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
            let frame=tokio::time::timeout(Duration::from_secs(40),
                read_stdio_frame(&mut self.stdout,MAX_STDIO_FRAME_BYTES))
                .await.map_err(|_|format!("MCP 응답 시간 초과: {method}"))?
                .map_err(|error|error.to_string())?;
            let Some(frame)=frame else{
                return Err("MCP 서버가 응답 없이 종료되었습니다.".into());
            };
            let line=String::from_utf8(frame).map_err(|error|error.to_string())?;
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
struct HttpEntry{last_used:Instant,handle:HttpHandle,healthy:Arc<AtomicBool>}
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
struct HttpLease{key:String,handle:HttpHandle,healthy:Arc<AtomicBool>,valid:bool}
impl Drop for HttpLease{
    fn drop(&mut self){
        if self.valid{return;}
        // Poison the Arc as well as evicting its map entry: another already
        // waiting caller must never reuse a session after a cancelled tool.
        self.healthy.store(false,Ordering::SeqCst);
        let mut pool=http_sessions().lock().unwrap();
        if pool.get(&self.key).is_some_and(|entry|
            Arc::ptr_eq(&entry.handle,&self.handle)){
            pool.remove(&self.key);
        }
    }
}
type HttpEraCache=Mutex<HashMap<String,bool>>;
fn modern_http_modes()->&'static HttpEraCache{
    static MODES:OnceLock<HttpEraCache>=OnceLock::new();
    MODES.get_or_init(||Mutex::new(HashMap::new()))
}
fn configured_http_era(record:&Value)->Result<Option<bool>,String>{
    match record["protocolVersion"].as_str().unwrap_or("auto"){
        "auto"|""=>Ok(None),
        "2026-07-28"=>Ok(Some(true)),
        "2024-11-05"|"legacy"=>Ok(Some(false)),
        other=>Err(format!("지원하지 않는 MCP HTTP 프로토콜 버전: {other}")),
    }
}
fn cached_modern_schema(state:&AppState,record:&Value,tool_name:&str)->Option<Value>{
    let id=record["id"].as_str()?;
    let namespace=server_namespace(record,&enabled_servers(state));
    let key=format!("{id}:{namespace}");
    cache().lock().unwrap().get(&key)?.1.iter()
        .find(|tool|tool.tool==tool_name).map(|tool|tool.schema.clone())
}
/// MCP 2026 and handshake-era HTTP share one owner-approved tool gateway.
/// Protocol discovery can fall back only during read-only tools/list. Never
/// resubmit an ambiguous tools/call through another transport.
async fn http_request(state:&AppState,record:&Value,method:&str,params:Value)
    ->Result<Value,String>{
    let (identity,token)=http_identity(state,record)?;
    let selected=configured_http_era(record)?
        .or_else(||modern_http_modes().lock().unwrap().get(&identity).copied())
        .or_else(||(method=="tools/list").then_some(true));
    match selected{
        Some(true)=>{
            let response=if method=="tools/list"{
                modern::list(record,token.as_deref()).await
            }else if method=="tools/call"{
                let name=params["name"].as_str().ok_or("MCP tool name missing")?;
                let schema=cached_modern_schema(state,record,name);
                let schema=match schema{
                    Some(schema)=>schema,
                    None=>{
                        // A direct tool invocation must not bypass schema
                        // discovery, including x-mcp-header annotations.
                        let listed=modern::list(record,token.as_deref()).await
                            .map_err(modern::ModernError::message)?;
                        let raw=listed["tools"].as_array().ok_or("MCP tool list is malformed")?;
                        raw.iter().find(|row|row["name"]==name)
                            .map(|row|row["inputSchema"].clone())
                            .ok_or("MCP tool not in advertised server catalog")?
                    }
                };
                modern::call(record,token.as_deref(),params.clone(),Some(&schema)).await
            }else{return Err("Unsupported modern MCP method".into());};
            match response{
                Ok(value)=>{
                    modern_http_modes().lock().unwrap().insert(identity,true);
                    Ok(value)
                }
                Err(modern::ModernError::Legacy(_)) if method=="tools/list"
                    && configured_http_era(record)?.is_none()=>{
                    modern_http_modes().lock().unwrap().insert(identity,false);
                    http_request_legacy(state,record,method,params).await
                }
                Err(error)=>Err(error.message())
            }
        }
        Some(false)=>http_request_legacy(state,record,method,params).await,
        None=>Err("MCP tools/list discovery must finish before an effectful call".into()),
    }
}
async fn http_request_legacy(state:&AppState,record:&Value,method:&str,params:Value)
    ->Result<Value,String>{
    let (key,token)=http_identity(state,record)?;
    let (handle,healthy)={
        let mut pool=http_sessions().lock().unwrap();
        pool.retain(|_,entry|{
            let expired=entry.last_used.elapsed()>=HTTP_IDLE_TTL
                && Arc::strong_count(&entry.handle)==1;
            if expired {entry.healthy.store(false,Ordering::SeqCst);}
            !expired
        });
        if let Some(entry)=pool.get_mut(&key){
            entry.last_used=Instant::now();
            (entry.handle.clone(),entry.healthy.clone())
        }else{
            // Never evict an active session to make space for another.
            if pool.len()>=MAX_HTTP_SESSIONS{
                let oldest=pool.iter()
                    .filter(|(_,entry)|Arc::strong_count(&entry.handle)==1)
                    .min_by_key(|(_,entry)|entry.last_used)
                    .map(|(key,_)|key.clone());
                let oldest=oldest.ok_or("MCP HTTP 동시 세션 한도(16)에 도달했습니다.")?;
                if let Some(entry)=pool.remove(&oldest){
                    entry.healthy.store(false,Ordering::SeqCst);
                }
            }
            let handle=Arc::new(AsyncMutex::new(None));
            let healthy=Arc::new(AtomicBool::new(true));
            pool.insert(key.clone(),HttpEntry{
                last_used:Instant::now(),handle:handle.clone(),
                healthy:healthy.clone()
            });
            (handle,healthy)
        }
    };
    // Inner mutex serializes only requests to this server+credential session;
    // independent servers can work in parallel.
    let mut lease=HttpLease{
        key,handle:handle.clone(),healthy:healthy.clone(),valid:false
    };
    let mut slot=handle.lock().await;
    if !healthy.load(Ordering::SeqCst){
        return Err("이 MCP 세션은 취소되거나 인증이 변경됐습니다. 새로운 작업으로 다시 조회하세요.".into());
    }
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
                // MCP servers may return request bodies containing tool
                // arguments. Never replay those bodies across redirects.
                .redirect(reqwest::redirect::Policy::none())
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
            let bytes=modern::read_limited(response,4*1024*1024).await?;
            let text=String::from_utf8_lossy(&bytes);
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
        let bytes=modern::read_limited(response,4*1024*1024).await?;
        let text=String::from_utf8(bytes).map_err(|error|error.to_string())?;
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

    #[tokio::test]
    async fn discovery_distinguishes_connected_empty_failed_and_disabled_servers(){
        use axum::{extract::Path,routing::post,Json,Router};
        async fn handler(Path(kind):Path<String>,Json(body):Json<Value>)->Json<Value>{
            let tools=if kind=="empty"{json!([])}else{
                json!([{"name":"echo","inputSchema":{"type":"object"}}])
            };
            Json(json!({"jsonrpc":"2.0","id":body["id"],"result":{"tools":tools}}))
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let app=Router::new().route("/{kind}",post(handler));
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
        let state=temp_state();
        let id=uuid::Uuid::new_v4().to_string();
        let good=format!("{id}-good");
        let empty=format!("{id}-empty");
        let broken=format!("{id}-broken");
        let disabled=format!("{id}-disabled");
        for (key,kind) in [(&good,"good"),(&empty,"empty")]{
            state.store.put("mcp",&json!({"id":key,"name":"Same name",
                "transport":"http","url":format!("{base}/{kind}"),
                "protocolVersion":"2026-07-28","enabled":true})).unwrap();
        }
        state.store.put("mcp",&json!({"id":broken,"name":"Broken",
            "transport":"stdio","enabled":true})).unwrap();
        state.store.put("mcp",&json!({"id":disabled,"name":"Disabled",
            "transport":"stdio","enabled":false})).unwrap();
        let rows=discover_servers(&state).await;
        assert_eq!(rows.len(),3);
        let connected=rows.iter().find(|row|row.0==good).unwrap();
        assert_eq!(connected.2.as_ref().unwrap().len(),1);
        assert!(rows.iter().find(|row|row.0==empty).unwrap().2.as_ref().unwrap().is_empty());
        assert!(rows.iter().find(|row|row.0==broken).unwrap().2.is_err());
        assert_eq!(tools(&state).await.len(),1,"status rows cannot become model tools");
        server.abort();
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }

    #[tokio::test]
    async fn modern_probe_falls_back_to_legacy_only_before_tool_effects(){
        use axum::{extract::State,http::{HeaderMap,StatusCode},
            routing::post,Json,Router};
        #[derive(Clone)]
        struct Observations(Arc<Mutex<Vec<(String,bool)>>>);
        async fn handler(State(state):State<Observations>,headers:HeaderMap,
            Json(body):Json<Value>)->(StatusCode,Json<Value>){
            let method=body["method"].as_str().unwrap_or("").to_string();
            let modern=headers.get("mcp-protocol-version").is_some();
            state.0.lock().unwrap().push((method.clone(),modern));
            if modern{
                return (StatusCode::BAD_REQUEST,Json(json!({"error":"not initialized"})));
            }
            let result=match method.as_str(){
                "initialize"=>json!({"protocolVersion":"2024-11-05",
                    "capabilities":{"tools":{}},"serverInfo":{"name":"legacy","version":"1"}}),
                "tools/list"=>json!({"tools":[{"name":"echo","inputSchema":{
                    "type":"object"}}]}),
                "tools/call"=>json!({"content":[{"type":"text","text":"legacy-ok"}]}),
                _=>json!({}),
            };
            (StatusCode::OK,Json(json!({"jsonrpc":"2.0","id":body["id"],
                "result":result})))
        }
        let observed=Observations(Arc::new(Mutex::new(Vec::new())));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let app=Router::new().route("/mcp",post(handler)).with_state(observed.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
        let state=temp_state();
        let record=json!({"id":uuid::Uuid::new_v4().to_string(),
            "name":"legacy-mock","transport":"http","url":url});
        let list=http_request(&state,&record,"tools/list",json!({})).await.unwrap();
        assert_eq!(list["tools"][0]["name"],"echo");
        let reply=http_request(&state,&record,"tools/call",
            json!({"name":"echo","arguments":{}})).await.unwrap();
        assert_eq!(reply["content"][0]["text"],"legacy-ok");
        let seen=observed.0.lock().unwrap().clone();
        assert_eq!(seen.iter().filter(|row|row.1).count(),1,
            "only read-only tools/list may probe the modern era");
        assert!(seen.iter().any(|row|row.0=="initialize"&&!row.1));
        assert!(seen.iter().any(|row|row.0=="tools/call"&&!row.1));
        invalidate();
        server.abort();
    }
    #[tokio::test]
    async fn modern_unsupported_version_never_silently_downgrades(){
        use axum::{extract::State,http::StatusCode,routing::post,Json,Router};
        #[derive(Clone)]struct Count(Arc<Mutex<usize>>);
        async fn reject(State(count):State<Count>,Json(body):Json<Value>)
            ->(StatusCode,Json<Value>){
            *count.0.lock().unwrap()+=1;
            (StatusCode::BAD_REQUEST,Json(json!({"jsonrpc":"2.0",
                "id":body["id"],"error":{"code":-32022,
                "message":"Unsupported protocol version",
                "data":{"supported":["2026-07-28"]}}})))
        }
        let count=Count(Arc::new(Mutex::new(0)));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app=Router::new().route("/mcp",post(reject)).with_state(count.clone());
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
        let state=temp_state();
        let record=json!({"id":uuid::Uuid::new_v4().to_string(),
            "name":"modern-mock","transport":"http","url":url});
        let error=http_request(&state,&record,"tools/list",json!({}))
            .await.unwrap_err();
        assert!(error.contains("-32022"),"{error}");
        assert_eq!(*count.0.lock().unwrap(),1,
            "modern negotiation failures must not trigger legacy initialize");
        invalidate();
        server.abort();
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
        let healthy=Arc::new(AtomicBool::new(true));
        http_sessions().lock().unwrap().insert(key.clone(),HttpEntry{
            last_used:Instant::now(),handle:handle.clone(),
            healthy:healthy.clone(),
        });
        {
            let _unfinished=HttpLease{
                key:key.clone(),handle,healthy:healthy.clone(),valid:false
            };
        }
        assert!(!healthy.load(Ordering::SeqCst));
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

    #[tokio::test]
    async fn stdio_frames_are_bounded_and_newline_delimited(){
        let (mut writer,reader)=tokio::io::duplex(32);
        writer.write_all(b"12345\n").await.unwrap();
        let mut reader=BufReader::new(reader);
        assert_eq!(read_stdio_frame(&mut reader,6).await.unwrap().unwrap(),b"12345\n");

        let (mut writer,reader)=tokio::io::duplex(32);
        writer.write_all(b"123456\n").await.unwrap();
        let mut reader=BufReader::new(reader);
        let error=read_stdio_frame(&mut reader,6).await.unwrap_err();
        assert_eq!(error.kind(),std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeds 6 bytes"));
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
