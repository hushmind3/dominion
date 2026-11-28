//! MCP http transport and contract.
use super::*;

const MODERN_VERSION: &str = "2026-07-28";
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum HttpEra { Modern, Legacy }

#[derive(serde::Deserialize)]
struct DiscoverResult {
    #[serde(rename = "supportedVersions")]
    supported_versions: Vec<String>,
    #[serde(default)]
    capabilities: Value,
}

fn modern_params(mut params: Value) -> Value {
    if !params.is_object() { params = json!({}); }
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
        "io.modelcontextprotocol/clientInfo": {
            "name":"dominion", "version":env!("CARGO_PKG_VERSION")
        },
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    params
}

const MAX_SSE_EVENT_BYTES: usize = 2 * 1024 * 1024;

/// A request-scoped SSE stream can remain open after its JSON-RPC response.
/// Parse complete events as bytes (UTF-8 may cross network chunk boundaries),
/// skipping heartbeat comments and unrelated notifications/responses.
pub(super) fn sse_event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    for index in 0..bytes.len() {
        if bytes[index] != b'\n' {
            continue;
        }
        if bytes.get(index + 1) == Some(&b'\n') {
            return Some((index, 2));
        }
        if bytes.get(index + 1) == Some(&b'\r')
            && bytes.get(index + 2) == Some(&b'\n')
        {
            return Some((index, 3));
        }
    }
    None
}

pub(super) fn sse_response_event(event: &[u8], request_id: Option<&Value>) -> Option<Value> {
    let text = std::str::from_utf8(event).ok()?;
    let mut data = String::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    let response: Value = serde_json::from_str(&data).ok()?;
    if response.get("result").is_none() && response.get("error").is_none() {
        return None;
    }
    if let Some(request_id) = request_id {
        // 2026 Streamable HTTP also permits a transport-level JSON-RPC
        // error without an id. Never accept another request's response.
        if response.get("id") != Some(request_id)
            && !(response.get("id").is_none() && response.get("error").is_some())
        {
            return None;
        }
    }
    Some(response)
}

// ------------------------------------------------------------------- http

pub(super) struct HttpSession {
    pub(super) client: reqwest::Client,
    pub(super) url: String,
    pub(super) session: Option<String>,
    pub(super) token: Option<String>,
    pub(super) next_id: u64,
    pub(super) era: HttpEra,
    pub(super) tool_schema: Option<Value>,
}

impl HttpSession {
    /// Probe 2026-07-28 discovery first. Old servers retain their initialize
    /// handshake, session id and notifications/initialized behavior.
    pub(super) async fn open(record: &Value, state: &AppState) -> Result<Self, String> {
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
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| error.to_string())?,
            url,
            session: None,
            token,
            next_id: 1,
            era: HttpEra::Modern,
            tool_schema: None,
        };
        if session.discover_modern().await? { return Ok(session); }
        session.era = HttpEra::Legacy;

        session
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "dominion", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        session.notify("notifications/initialized", json!({})).await;
        Ok(session)
    }

    pub(super) fn modern(&self) -> bool { self.era == HttpEra::Modern }
    pub(super) fn set_tool_schema(&mut self,schema:Value){self.tool_schema=Some(schema);}

    async fn discover_modern(&mut self) -> Result<bool, String> {
        let id=self.next_id;
        self.next_id+=1;
        let payload=json!({"jsonrpc":"2.0","id":id,"method":"server/discover",
            "params":modern_params(json!({}))});
        match self.post(&payload).await {
            Ok(value) => {
                let Some(result)=value.get("result") else {
                    return if value["error"]["code"] == -32601 { Ok(false) }
                        else { Err(format!("MCP discovery error: {}",value["error"])) };
                };
                // Old test/legacy servers may answer unknown methods with an
                // empty success. Only a schema-valid discovery selects modern.
                if result.get("supportedVersions").is_none() { return Ok(false); }
                let discovered: DiscoverResult=serde_json::from_value(result.clone())
                    .map_err(|error|format!("MCP discovery schema: {error}"))?;
                if !discovered.supported_versions.iter().any(|version|version==MODERN_VERSION) {
                    return Err("MCP 서버가 2026-07-28을 지원하지 않습니다.".into());
                }
                let _capabilities=discovered.capabilities;
                Ok(true)
            }
            Err(error) if error.starts_with("MCP HTTP 404") => Ok(false),
            // A compliant older server rejects the 2026 protocol version
            // with -32022. Retry the existing initialize handshake, but do not
            // downgrade on a header mismatch (-32020) or auth/validation error.
            Err(error) if error.starts_with("MCP HTTP 400")
                && error.contains("-32022") => Ok(false),
            Err(error)=>Err(error),
        }
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

    pub(super) async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": if self.modern(){modern_params(params)}else{params},
        });
        let value = self.post(&payload).await?;
        if let Some(error) = value.get("error") {
            return Err(format!("MCP 오류: {error}"));
        }
        Ok(value.get("result").cloned().unwrap_or_else(|| json!({})))
    }

    pub(super) async fn post(&mut self, payload: &Value) -> Result<Value, String> {
        let mut request = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .json(payload);
        if self.modern() {
            let method=payload["method"].as_str().ok_or("MCP 요청 메서드가 없습니다.")?;
            request=request.header("MCP-Protocol-Version",MODERN_VERSION)
                .header("Mcp-Method",method);
            if matches!(method,"tools/call"|"resources/read"|"prompts/get") {
                let name=payload["params"]["name"].as_str()
                    .or_else(||payload["params"]["uri"].as_str())
                    .ok_or("MCP 요청 이름 또는 URI가 없습니다.")?;
                request=request.header("Mcp-Name",super::schema::encode_header(name));
            }
            if method=="tools/call" {
                if let Some(schema)=self.tool_schema.as_ref() {
                    for (header,value) in super::schema::projected_headers(
                        schema,&payload["params"]["arguments"]
                    )? {
                        request=request.header(header.as_str(),value.as_str());
                    }
                }
            }
        }
        if !self.modern() {
            if let Some(session) = &self.session {
                request = request.header("mcp-session-id", session);
            }
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
            if !self.modern() { self.session = Some(session.to_string()); }
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        if content_type.contains("text/event-stream") {
            use futures_util::StreamExt;
            let mut stream = response.bytes_stream();
            let mut pending = Vec::new();
            while let Some(chunk) = stream.next().await {
                pending.extend_from_slice(&chunk.map_err(|error| error.to_string())?);
                while let Some((end, delimiter_len)) = sse_event_boundary(&pending) {
                    if end > MAX_SSE_EVENT_BYTES {
                        return Err("MCP SSE 이벤트 크기 제한을 초과했습니다.".into());
                    }
                    let event = pending[..end].to_vec();
                    pending.drain(..end + delimiter_len);
                    if let Some(value) = sse_response_event(&event, payload.get("id")) {
                        // Return without waiting for the server to close SSE.
                        return Ok(value);
                    }
                }
                if pending.len() > MAX_SSE_EVENT_BYTES {
                    return Err("MCP SSE 이벤트 크기 제한을 초과했습니다.".into());
                }
            }
            if let Some(value) = sse_response_event(&pending, payload.get("id")) {
                return Ok(value);
            }
            return Err("MCP SSE 스트림이 요청 결과 없이 종료됐습니다.".into());
        }
        let text = response.text().await.map_err(|error| error.to_string())?;
        if text.trim().is_empty() {
            return if payload.get("id").is_none() {
                Ok(json!({}))
            } else {
                Err("MCP HTTP 요청에 대한 응답 본문이 비었습니다.".into())
            };
        }
        let value = serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())?;
        if let Some(request_id) = payload.get("id") {
            if value.get("id") != Some(request_id)
                && !(value.get("id").is_none() && value.get("error").is_some())
            {
                return Err("MCP JSON-RPC 응답 ID가 요청과 일치하지 않습니다.".into());
            }
        }
        Ok(value)
    }
}
