//! Standalone optional MCP stdio transport for native and external hosts.
//! The host explicitly supplies a trusted executable, arguments and workspace.
//! This is a process boundary, not a sandbox. The child inherits the host
//! environment plus explicit overrides, as in the existing app transport.
use serde_json::{json,Value};
use std::{path::Path,time::Duration,collections::HashSet};
use tokio::io::{AsyncBufRead,AsyncBufReadExt,AsyncWriteExt,BufReader};
use tokio::process::{Child,ChildStdin,ChildStdout,Command};
const MAX_STDIO_FRAME_BYTES:usize=4*1024*1024;
const REQUEST_TIMEOUT:Duration=Duration::from_secs(40);
const PROBE_TIMEOUT:Duration=Duration::from_secs(3);
const MODERN_VERSION:&str="2026-07-28";

/// Read one newline-delimited MCP frame without allowing a child process to
/// grow memory without bound. Returns `None` only for EOF before any bytes.
#[doc(hidden)]
pub async fn read_stdio_frame<R:AsyncBufRead+Unpin>(
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

pub struct StdioSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    healthy: bool,
    modern: bool,
}

impl StdioSession {
    pub async fn open(record:&Value,workspace:&Path)->Result<Self,String>{
        let mode=record["protocolVersion"].as_str().unwrap_or("auto");
        if !matches!(mode,"auto"|""|"legacy"|"2024-11-05"|"2025-03-26"|"2025-06-18"|"2025-11-25"|"2026-07-28"){
            return Err("unsupported MCP stdio protocol selection".into());
        }
        let mut session=Self::spawn(record,workspace)?;
        if matches!(mode,"auto"|""|"2026-07-28"){
            session.modern=true;
            let probe=tokio::time::timeout(PROBE_TIMEOUT,
                session.exchange("server/discover",json!({}))).await;
            match probe {
                Ok(Ok(reply)) if reply.get("error").is_none()
                    && !reply["result"].as_object().is_some_and(serde_json::Map::is_empty)=>{
                    let versions=reply["result"]["supportedVersions"].as_array()
                        .ok_or("MCP discovery missing supportedVersions")?;
                    if !versions.iter().any(|v|v==MODERN_VERSION){
                        return Err("MCP server has no mutually supported modern version".into());
                    }
                    return Ok(session);
                }
                Ok(Ok(reply)) if reply["error"]["code"].as_i64()
                    .is_some_and(|code|(-32099..=-32020).contains(&code))=>{
                    return Err(format!("MCP modern discovery rejected: {}",reply["error"]));
                }
                _ if mode==MODERN_VERSION=>return Err("MCP modern discovery failed".into()),
                _=>{
                    // Discovery is read-only. Restart before legacy negotiation
                    // so partial frames or late probe replies cannot leak eras.
                    drop(session);
                    session=Self::spawn(record,workspace)?;
                }
            }
        }
        session.initialize_legacy(if matches!(mode,"auto"|""|"legacy"){ "2024-11-05" }else{mode}).await?;
        Ok(session)
    }

    fn spawn(record:&Value,workspace:&Path)->Result<Self,String>{
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
        Ok(Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            healthy: true,
            modern: false,
        })
    }
    async fn initialize_legacy(&mut self,version:&str)->Result<(),String>{
        let negotiated=self
            .request(
                "initialize",
                json!({
                    "protocolVersion": version,
                    "capabilities": {},
                    "clientInfo": { "name": "dominion-rust", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        // This shared transport retains the legacy initialized stdio
        // contract. Reject unknown negotiation rather than call blindly.
        if !matches!(negotiated["protocolVersion"].as_str(),
            Some("2024-11-05"|"2025-03-26"|"2025-06-18"|"2025-11-25")){
            return Err("unsupported initialized MCP stdio protocol".into());
        }
        tokio::time::timeout(REQUEST_TIMEOUT,
            self.notify("notifications/initialized",json!({})))
            .await.map_err(|_|"MCP initialization notification timeout".to_string())??;
        Ok(())
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

    /// A timeout, malformed response or cancellation invalidates this
    /// session. It is never silently reopened or replayed after a tool call.
    pub async fn request(&mut self,method:&str,params:Value)->Result<Value,String>{
        if !self.healthy{return Err("MCP stdio session is no longer usable".into());}
        self.healthy=false;
        let id=self.next_id;
        let result=match tokio::time::timeout(REQUEST_TIMEOUT,self.exchange(method,params)).await{
            Ok(Ok(reply))=>{
                if let Some(error)=reply.get("error"){Err(format!("MCP 오류: {error}"))}
                else if reply["result"]["resultType"]=="input_required"{
                    Err("MCP input_required: explicit owner input required".into())
                }else{reply.get("result").cloned().ok_or_else(||"MCP response missing result".into())}
            }
            Ok(Err(error))=>Err(error),
            Err(_)=>{
                let _=tokio::time::timeout(Duration::from_millis(250),
                    self.notify("notifications/cancelled",json!({"requestId":id,"reason":"request deadline"}))).await;
                Err(format!("MCP response deadline exceeded: {method}"))
            }
        };
        if result.is_ok(){self.healthy=true;}else{let _=self._child.start_kill();}
        result
    }

    /// Complete paginated tool discovery, with bounded pages and cursors.
    pub async fn list_tools(&mut self)->Result<Value,String>{
        let mut all=Vec::new();
        let mut cursor=None::<String>;
        let mut seen=HashSet::new();
        for _ in 0..16{
            let params=cursor.as_ref().map(|v|json!({"cursor":v})).unwrap_or_else(||json!({}));
            let page=self.request("tools/list",params).await?;
            let tools=page["tools"].as_array().ok_or("MCP tools/list missing tools")?;
            if all.len().saturating_add(tools.len())>1000{return Err("MCP catalog exceeds 1000 tools".into());}
            all.extend(tools.iter().cloned());
            match page["nextCursor"].as_str(){
                None|Some("")=>return Ok(json!({"tools":all})),
                Some(next) if next.len()<=1024 && seen.insert(next.to_string())=>cursor=Some(next.to_string()),
                _=>return Err("MCP repeated or invalid list cursor".into()),
            }
        }
        Err("MCP catalog exceeds 16 pages".into())
    }

    async fn exchange(&mut self, method: &str, mut params: Value) -> Result<Value, String> {
        if self.modern{
            if !params.is_object(){return Err("MCP parameters must be an object".into());}
            params["_meta"]=json!({
                "io.modelcontextprotocol/protocolVersion":MODERN_VERSION,
                "io.modelcontextprotocol/clientInfo":{"name":"dominion-rust","version":env!("CARGO_PKG_VERSION")},
                "io.modelcontextprotocol/clientCapabilities":{}
            });
        }
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
            let frame=read_stdio_frame(&mut self.stdout,MAX_STDIO_FRAME_BYTES)
                .await.map_err(|error|error.to_string())?;
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
            if value["jsonrpc"]!="2.0" || (value.get("result").is_some()==value.get("error").is_some()){
                return Err("invalid MCP JSON-RPC response".into());
            }
            return Ok(value);
        }
    }
}

