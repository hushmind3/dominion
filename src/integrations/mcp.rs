use crate::state::AppState;
mod modern;
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
    fn is_modern(&self)->bool{
        matches!(self,Self::Http(HttpSession{modern:true,..}))
    }
    async fn request_tool(&mut self,params:Value,schema:&Value)->Result<Value,String>{
        match self {
            Self::Stdio(session)=>session.request("tools/call",params).await,
            Self::Http(session)=>session.request_tool(params,schema).await,
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
    let modern=session.is_modern();
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
            // 2026 HTTP requires every x-mcp-header annotation to be valid.
            // Drop an invalid declaration, not its required routing headers.
            if modern && modern::validate_tool_schema(&tool["inputSchema"]).is_err(){
                return None;
            }
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
    let result=if session.is_modern(){
        // Modern HTTP tools can require schema-declared Mcp-Param-* headers.
        // Read the live manifest on the SAME transport before dispatch.
        match session.request("tools/list",json!({})).await {
            Ok(list)=>{
                let found=list["tools"].as_array().into_iter().flatten()
                    .filter(|tool|tool["name"].as_str()==Some(tool_name))
                    .collect::<Vec<_>>();
                if found.len()!=1{Err("MCP tool absent or duplicated on live server".into())}
                else {
                    let schema=&found[0]["inputSchema"];
                    match modern::validate_tool_schema(schema){
                        Ok(())=>session.request_tool(params,schema).await,
                        Err(error)=>Err(error),
                    }
                }
            },
            Err(error)=>Err(error),
        }
    }else{session.request("tools/call",params).await};
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
  session.request_tool(params,schema).await
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
    modern: bool,
    legacy_version: Option<String>,
    // Only the very next tools/call consumes this live validated schema.
    tool_schema: Option<Value>,
}

impl HttpSession {
    /// Modern HTTP probes only a read-only server/discover operation.
    /// The fallback never retries a paid/effecting tool invocation.
    async fn open(record: &Value, state: &AppState) -> Result<Self, String> {
        let url = record.get("url").and_then(Value::as_str)
            .filter(|value|!value.is_empty())
            .ok_or("MCP HTTP 서버에 url 이 없습니다.")?.to_string();
        let parsed = reqwest::Url::parse(&url).map_err(|error|error.to_string())?;
        if !matches!(parsed.scheme(),"http"|"https") {
            return Err("MCP URL 은 HTTP(S)여야 합니다.".into());
        }
        let id=record["id"].as_str().unwrap_or_default();
        let mode=record["protocolMode"].as_str().unwrap_or("auto");
        if !matches!(mode,"auto"|"modern"|"legacy"){
            return Err("MCP protocolMode must be auto, modern or legacy".into());
        }
        // Prefer 2025-11-25 when the HTTP peer needs an initialize handshake.
        // An explicit older version can still be pinned in its server record.
        let mut legacy_request=record["legacyProtocolVersion"].as_str()
            .unwrap_or("2025-11-25").to_string();
        if !matches!(legacy_request.as_str(),
            "2025-11-25"|"2025-06-18"|"2025-03-26"|"2024-11-05"){
            return Err("Unsupported legacy MCP protocol version".into());
        }
        let mut session=Self{
            client:reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .build().map_err(|error|error.to_string())?,
            url,session:None,token:state.read_secret(&format!("mcp:{id}")),
            next_id:1,modern:mode!="legacy",legacy_version:None,tool_schema:None,
        };
        if session.modern {
            let probe=json!({"jsonrpc":"2.0","id":session.next_id,
                "method":"server/discover","params":{}});
            session.next_id+=1;
            match session.post(&probe).await {
                Ok(value)=>{
                    if value["error"]["code"]==-32601 && mode=="auto" {
                        // A legacy handshake-era peer replied MethodNotFound.
                        session.modern=false;
                    }else if let Some(error)=value.get("error"){
                        return Err(format!("MCP modern discovery rejected: {error}"));
                    }else if value["id"]!=probe["id"] {
                        return Err("MCP modern discovery replied to another call".into());
                    }else{
                        let result=&value["result"];
                        if !result["supportedVersions"].as_array()
                            .is_some_and(|versions|versions.iter()
                                .any(|v|v.as_str()==Some(modern::VERSION))) {
                            return Err("MCP server/discover does not advertise 2026-07-28".into());
                        }
                    }
                },
                Err(error) if mode=="auto" &&
                    modern::advertised_legacy_version(&error).is_some()=>{
                    legacy_request=modern::advertised_legacy_version(&error)
                        .unwrap().into();
                    session.modern=false;
                },
                Err(error) if mode=="auto" &&
                    modern::legacy_http_probe_rejection(&error)=>{
                    // Ordinary handshake-era HTTP gateways often reject the
                    // unknown discovery method. A modern JSON-RPC error, 401,
                    // 403, 5xx or ambiguous network failure NEVER falls back.
                    session.modern=false;
                },
                Err(error)=>return Err(error),
            }
        }
        if !session.modern {
            let response=session.request("initialize",json!({
                "protocolVersion":legacy_request,
                "capabilities":{},
                "clientInfo":{"name":"dominion-rust",
                    "version":env!("CARGO_PKG_VERSION")}
            })).await?;
            let negotiated=response["protocolVersion"].as_str()
                .ok_or("MCP initialize returned no protocolVersion")?;
            if !matches!(negotiated,
                "2025-11-25"|"2025-06-18"|"2025-03-26"|"2024-11-05"){
                return Err("MCP initialize selected an unsupported version".into());
            }
            session.legacy_version=Some(negotiated.into());
            session.notify("notifications/initialized",json!({})).await?;
        }
        Ok(session)
    }

    async fn notify(&mut self,method:&str,params:Value)->Result<(),String>{
        if self.modern {
            return Err("Modern MCP does not use initialized notifications".into());
        }
        let payload=json!({"jsonrpc":"2.0","method":method,"params":params});
        self.post(&payload).await.map(|_|())
    }

    async fn request_tool(&mut self,params:Value,schema:&Value)->Result<Value,String>{
        self.tool_schema=Some(schema.clone());
        self.request("tools/call",params).await
    }

    async fn request(&mut self,method:&str,params:Value)->Result<Value,String>{
        let id=self.next_id;
        self.next_id+=1;
        let payload=json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        let value=self.post(&payload).await?;
        if value["id"]!=id {
            return Err(format!("MCP JSON-RPC response ID mismatch for {method}"));
        }
        if let Some(error)=value.get("error") {
            return Err(format!("MCP 오류: {error}"));
        }
        let result=value.get("result").cloned()
            .ok_or("MCP reply missing result")?;
        if self.modern && result["resultType"]=="input_required" {
            // MRTR must return control to owner approval, NEVER repeat an
            // effecting tools/call without explicit inputResponses.
            return Err("MCP input_required: user input/approval needed; no automatic replay".into());
        }
        Ok(result)
    }

    async fn post(&mut self,payload:&Value)->Result<Value,String>{
        let (body,headers)=if self.modern {
            modern::decorate(payload,self.tool_schema.take().as_ref())?
        }else {
            self.tool_schema=None;
            (payload.clone(),Vec::new())
        };
        let mut request=self.client.post(&self.url)
            .header("content-type","application/json")
            .header("accept","application/json, text/event-stream")
            .json(&body);
        for (name,value) in headers {
            let key=reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|error|format!("Invalid MCP routing header name: {error}"))?;
            let value=reqwest::header::HeaderValue::from_str(&value)
                .map_err(|error|format!("Invalid MCP routing header: {error}"))?;
            request=request.header(key,value);
        }
        if !self.modern {
            if let Some(session)=&self.session{
                request=request.header("mcp-session-id",session);
            }
            if let Some(version)=self.legacy_version.as_deref() {
                if version>="2025-06-18" {
                    request=request.header("MCP-Protocol-Version",version);
                }
            }
        }
        if let Some(token)=&self.token {
            request=request.header("authorization",format!("Bearer {token}"));
        }
        let response=request.send().await.map_err(|error|error.to_string())?;
        if !response.status().is_success() {
            let status=response.status();
            let text=response.text().await.unwrap_or_default();
            return Err(format!("MCP HTTP {status}: {}",
                text.chars().take(700).collect::<String>()));
        }
        if !self.modern {
            if let Some(session)=response.headers().get("mcp-session-id")
                .and_then(|value|value.to_str().ok()){
                self.session=Some(session.to_string());
            }
        }
        let content_type=response.headers().get("content-type")
            .and_then(|value|value.to_str().ok()).unwrap_or("").to_string();
        let text=response.text().await.map_err(|error|error.to_string())?;
        if text.trim().is_empty() {
            if self.modern{return Err("MCP response missing JSON-RPC completion".into());}
            return Ok(json!({}));
        }
        if content_type.contains("text/event-stream") {
            let normalized=text.replace("\r\n","\n");
            let mut settled:Option<Value>=None;
            for block in normalized.split("\n\n") {
                let data=block.lines()
                    .filter_map(|line|line.strip_prefix("data:")
                        .map(str::trim_start)).collect::<Vec<_>>().join("\n");
                if data.is_empty(){continue;}
                let value:Value=serde_json::from_str(&data)
                    .map_err(|error|format!("Invalid MCP SSE JSON: {error}"))?;
                if value.get("result").is_some()||value.get("error").is_some(){
                    if value["id"]!=body["id"] {
                        return Err("MCP SSE returned an unrelated JSON-RPC result".into());
                    }
                    if settled.replace(value).is_some(){
                        return Err("MCP SSE contained multiple terminal results".into());
                    }
                }
            }
            return settled.ok_or("MCP SSE ended without a terminal JSON-RPC result".into());
        }
        serde_json::from_str(&text).map_err(|error|error.to_string())
    }
}

#[cfg(test)]
#[path = "mcp/tests.rs"]
mod tests;
