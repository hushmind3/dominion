use crate::state::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
#[path = "mcp/stdio.rs"]
mod stdio;
#[path = "mcp/http.rs"]
mod http;
#[path = "mcp/schema.rs"]
mod schema;
use stdio::StdioSession;
use http::{HttpSession, HttpEra, sse_event_boundary, sse_response_event};
#[cfg(test)]
#[path = "mcp/tests.rs"]
mod tests;


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

type Cache = Mutex<HashMap<String, (Instant, Duration, Vec<McpTool>)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn invalidate() {
    cache().lock().unwrap().clear();
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
    use futures_util::{stream, StreamExt};
    let servers = enabled_servers(state);
    // Discover independent MCP servers concurrently, but keep tool ordering
    // deterministic for model-schema caching and reproducible tool selection.
    // Bounded fan-out also prevents many configured servers from spawning at
    // once or a single slow server from delaying all other handshakes.
    // Own each record before entering an async future. Borrowing from an
    // iterator closure across await makes the outer agent future non-Send on
    // some stable Rust compilers, breaking tokio::spawn call sites.
    let entries = servers.iter().filter_map(|server| {
        let id = server["id"].as_str()?.to_string();
        if id.is_empty() { return None; }
        Some((id, server_namespace(server, &servers), server.clone()))
    }).collect::<Vec<_>>();
    let lookups = entries.into_iter().map(|(id, namespace, server)| async move {
        let cache_key = format!("{id}:{namespace}");
            let cached = {
                let guard = cache().lock().unwrap();
                guard.get(&cache_key).and_then(|(at, ttl, tools)| {
                    (at.elapsed() < *ttl).then(|| tools.clone())
                })
            };
            if let Some(tools) = cached { return tools; }
            match list_server(state, &server, &namespace).await {
                Ok((tools, ttl)) => {
                    if !ttl.is_zero() {
                        cache().lock().unwrap().insert(
                            cache_key, (Instant::now(), ttl, tools.clone())
                        );
                    }
                    tools
                }
                Err(error) => {
                    state.store.audit(
                        "mcp.list_failed", json!({ "server": id, "error": error })
                    );
                    Vec::new()
                }
            }
    });
    stream::iter(lookups)
        .buffered(4)
        .collect::<Vec<Vec<McpTool>>>()
        .await
        .into_iter()
        .flatten()
        .collect()
}

fn list_cache_ttl(result:&Value, modern_http:bool)->Duration {
    if !modern_http { return CACHE_TTL; }
    if matches!(result["cacheScope"].as_str(), Some("public" | "private")) {
        Duration::from_millis(result["ttlMs"].as_u64().unwrap_or(0).min(300_000))
    } else {
        Duration::ZERO
    }
}

async fn list_server(state: &AppState, server: &Value, server_slug:&str) -> Result<(Vec<McpTool>, Duration), String> {
    let server_name = server
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("mcp")
        .to_string();
    let transport = server
        .get("transport")
        .and_then(|value| value.as_str())
        .unwrap_or("stdio");

    let (result, modern_http) = if transport == "http" {
        let mut session = HttpSession::open(server, state).await?;
        let result = session.request("tools/list", json!({})).await?;
        (result, session.modern())
    } else {
        let mut session = StdioSession::open(server, &state.config.workspace).await?;
        let result = session.request("tools/list", json!({})).await;
        drop(session);
        (result?, false)
    };

    // In 2026, ttlMs/cacheScope are server-owned. Never reuse a modern
    // tool catalog beyond its advertised freshness, and never invent TTL.
    let ttl = list_cache_ttl(&result, modern_http);
    let tools = result
        .get("tools")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let listed = tools
        .into_iter()
        .filter_map(|tool| {
            let name = tool
                .get("name")
                .and_then(|value| value.as_str())?
                .to_string();
            Some(McpTool {
                server_name: server_name.clone(),
                slug: server_slug.to_string(),
                tool: name.clone(),
                description: tool
                    .get("description")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                schema: {
                    let schema=tool.get("inputSchema").cloned()
                        .unwrap_or_else(||json!({"type":"object","properties":{}}));
                    if modern_http {
                        if let Err(error)=schema::validate(&schema) {
                            state.store.audit("mcp.schema_rejected",json!({
                                "server":server_name,"tool":name,"reason":error
                            }));
                            return None;
                        }
                    }
                    schema
                },
            })
        })
        .collect::<Vec<_>>();
    Ok((listed, ttl))
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
    server_requires_approval(state,qualified)
}

/// Server-level requirement independent of the global MCP bypass.
pub fn server_requires_approval(state: &AppState, qualified: &str) -> bool {
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
        let mut session = HttpSession::open(server, state).await?;
        if session.modern() {
            let definition=tools(state).await.into_iter()
                .find(|item|item.slug==server_slug&&item.tool==tool_name)
                .ok_or("MCP 도구 스키마를 검증할 수 없습니다.")?;
            session.set_tool_schema(definition.schema);
        }
        session.request("tools/call", params).await
    } else {
        let mut session = StdioSession::open(server, &state.config.workspace).await?;
        let result = session.request("tools/call", params).await;
        drop(session);
        result
    }
}

// ------------------------------------------------------------------ stdio

