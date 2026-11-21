//! End-to-end test: runs the real binary, a fake OpenAI-compatible model, and
//! exercises HTTP/SSE, persistence and the cross-origin write guard.

use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use std::time::{Duration, Instant};

fn scripted_provider(
    reply: impl Fn(&Value) -> Vec<Value> + Send + Sync + 'static,
) -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let reply = Arc::new(reply);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                break;
            };
            let observed = observed.clone();
            let reply = reply.clone();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let end = loop {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(at) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..end]).into_owned();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|length| length.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                // Websocket upgrades from the Codex adapter carry no JSON body;
                // ignore them instead of panicking the fixture thread.
                let Ok(body) = serde_json::from_slice::<Value>(&bytes[end..end + length]) else {
                    return;
                };
                let mut recorded=body.clone();
                if body["prompt_cache_key"].is_string() {recorded["_testHttpHeaders"]=json!(headers);}
                observed.lock().unwrap().push(recorded);
                let events = reply(&body);
                // Tests may emulate a strict provider's HTTP error before a
                // generation starts; normal scripted streams are unchanged.
                if let Some(status)=events.first().and_then(|event|event["_testHttpStatus"].as_u64()) {
                    let payload=events[0]["body"].to_string();
                    let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                    let _=stream.write_all(response.as_bytes());
                    return;
                }
                let stream_mode = body["stream"].as_bool().unwrap_or(false);
                let response = if stream_mode {
                    let mut payload = events
                        .iter()
                        .map(|event| format!("data: {event}\n\n"))
                        .collect::<String>();
                    payload.push_str("data: [DONE]\n\n");
                    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len())
                } else {
                    let content = events
                        .iter()
                        .filter_map(|event| event["choices"][0]["delta"]["content"].as_str())
                        .collect::<String>();
                    let payload = json!({"choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}]}).to_string();
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len())
                };
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    (port, requests)
}

struct TestApp {
    _child: ChildGuard,
    _root: PathBuf,
    base: String,
    client: reqwest::blocking::Client,
}

impl TestApp {
    fn new(provider_port: u16, tool_mode: &str) -> Self {
        Self::new_with_env(provider_port, tool_mode, &[])
    }
    fn new_with_env(provider_port: u16, tool_mode: &str, extra: &[(&str, &str)]) -> Self {
        Self::from_root(provider_port, tool_mode, temp_root(), extra)
    }
    fn from_root(
        provider_port: u16,
        tool_mode: &str,
        root: PathBuf,
        extra: &[(&str, &str)],
    ) -> Self {
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_dominion-rust"));
        command
            .env("DOMINION_PORT", port.to_string())
            .env("DOMINION_DATA", root.join("runtime"))
            .env("DOMINION_WEB", root.join("web"))
            .env("DOMINION_WORKSPACE", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (key, value) in extra {
            command.env(key, value);
        }
        let child = command.spawn().unwrap();
        let app = Self {
            _child: ChildGuard(child),
            _root: root,
            base: format!("http://127.0.0.1:{port}"),
            client: reqwest::blocking::Client::new(),
        };
        wait_health(port, 20);
        let providers = app.get("/api/providers");
        if providers.as_array().is_none_or(|items| items.is_empty()) {
            app.post("/api/providers",&json!({"name":"mock","baseUrl":format!("http://127.0.0.1:{provider_port}/v1"),"model":"mock","enabled":true,"toolMode":tool_mode}));
        }
        app
    }
    fn post(&self, path: &str, body: &Value) -> Value {
        self.client
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn delete(&self, path: &str) {
        let _ = self
            .client
            .delete(format!("{}{path}", self.base))
            .send();
    }
    /// Status-preserving post for contracts that must reject a request.
    fn post_status(&self, path: &str, body: &Value) -> (u16, Value) {
        let response = self
            .client
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .unwrap();
        let status = response.status().as_u16();
        let value = response.json().unwrap_or(Value::Null);
        (status, value)
    }
    fn get(&self, path: &str) -> Value {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn chat(&self, body: &Value) -> String {
        self.client
            .post(format!("{}/api/chat", self.base))
            .json(body)
            .send()
            .unwrap()
            .text()
            .unwrap()
    }

    fn chat_async(&self, body: &Value) -> std::thread::JoinHandle<String> {
        let client = self.client.clone();
        let url = format!("{}/api/chat", self.base);
        let body = body.clone();
        std::thread::spawn(move || {
            client
                .post(url)
                .json(&body)
                .send()
                .map(|response| response.text().unwrap_or_default())
                .unwrap_or_default()
        })
    }

    fn wait_pending_approval(&self, seconds: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if let Some(record) = self
                .get("/api/approvals")
                .as_array()
                .and_then(|items| items.first())
            {
                return record.clone();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("approval was not requested");
    }
}

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}


fn remote_backup_sink(fail: bool) -> (u16, Arc<Mutex<Vec<(String, String)>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let observed = received.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let observed = observed.clone();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let end = loop {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(at) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..end]).to_string();
                let path = headers
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string();
                let hash = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("x-dominion-sha256:")
                            .map(|value| value.trim().to_string())
                    })
                    .unwrap_or_default();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                if fail {
                    let _ = stream.write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                observed.lock().unwrap().push((path, hash.clone()));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nx-dominion-sha256: {hash}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    (port, received)
}


fn evidence_bundle(pairs: u64, baseline_ok: bool, treatment_ok: bool) -> Value {
    let mut trials = Vec::new();
    for pair in 0..pairs {
        for (arm, ok) in [("baseline", baseline_ok), ("treatment", treatment_ok)] {
            trials.push(json!({
                "pair":pair,"arm":arm,
                "executionId":format!("e2e-exec-{pair}-{arm}"),
                "status":"completed",
                "auditOk":ok,"reportOk":ok,
                "taskOutcome":if ok {"succeeded"} else {"failed"},
                "compliance":ok,"requiresCompliance":false,"errorClass":"",
                "planQuality":0.0,"planQualityIndependent":0.0,"elapsedMs":10
            }));
        }
    }
    let delta = if treatment_ok {1.0} else {0.0} - if baseline_ok {1.0} else {0.0};
    let n = pairs as f64;
    let halfwidth = (2.0 * (2.0f64 / 0.05).ln() / n).sqrt();
    let verdict = if pairs >= 10 && delta - halfwidth > 0.0 {
        "preliminary_support"
    } else if pairs >= 10 && delta + halfwidth < 0.0 {
        "regressed"
    } else {
        "inconclusive"
    };
    let manifest = json!({
        "pairs":pairs,"seed":94371,"oracleVersion":"s7-harness-audit-v1",
        "fixture":"planning_evaluation","mode":"smoke","model":{"model":"mock"}
    });
    let hash = format!("{:x}", Sha256::digest(manifest.to_string().as_bytes()));
    json!({
        "trials":trials,"manifest":manifest,"manifestHash":hash,
        "delta":delta,"independentUnits":pairs,"verdict":verdict
    })
}


fn seed_evidence_executions(app: &TestApp, evidence: &Value) {
    let path = app._root.join("runtime/dominion.sqlite");
    let Ok(conn) = Connection::open(path) else {
        return;
    };
    let _ = conn.busy_timeout(Duration::from_secs(5));
    for trial in evidence["trials"].as_array().into_iter().flatten() {
        if trial["taskOutcome"].as_str() != Some("succeeded") {
            continue;
        }
        let Some(id) = trial["executionId"].as_str() else {
            continue;
        };
        let record = json!({
            "id": id,
            "status": "completed",
            "taskOutcome": "succeeded",
        });
        let _ = conn.execute(
            "INSERT OR REPLACE INTO records(kind,id,data) VALUES('execution',?1,?2)",
            rusqlite::params![id, record.to_string()],
        );
    }
}


fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

/// Minimal OpenAI-compatible SSE server.
fn start_mock_provider() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mock bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let _ = handle(stream);
        }
    });
    port
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    // Read headers.
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buffer).to_string();
    let length = text
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    let header_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(buffer.len());
    while buffer.len() < header_end + length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    if text.contains("/chat/completions") {
        // Slow enough that a second concurrent turn overlaps the first.
        std::thread::sleep(Duration::from_millis(600));
    }
    let body = if text.contains("/chat/completions") {
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello-e2e\"}}]}\n\n\
         data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"total_tokens\":11}}\n\n\
         data: [DONE]\n\n"
    } else {
        "{}"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn wait_health(port: u16, seconds: u64) {
    let client = reqwest::blocking::Client::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{port}/api/health"))
            .timeout(Duration::from_secs(1))
            .send()
        {
            if response.status().is_success() {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("server did not become healthy");
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn temp_root() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("dominion-e2e-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("web")).expect("web dir");
    std::fs::write(dir.join("web/index.html"), "<html>ok</html>").expect("index");
    dir
}


fn measure_estimate(text: &str) -> usize {
    let ascii = text.chars().filter(char::is_ascii).count();
    ascii.div_ceil(4) + text.chars().count() - ascii
}

/// Research measurement, not a behavioural guarantee: shows exactly what the
/// harness sends per participant across three owner turns in a nine-person room.

/// Minimal Codex Responses websocket: one chain per connection, exactly like
/// the subscription backend. Records every `response.create` frame it receives
/// so a test can inspect the incremental payload.
fn scripted_websocket_provider() -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let observed = observed.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                        return;
                    };
                    let mut count = 0usize;
                    while let Some(Ok(message)) = socket.next().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(body) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if body["type"] != "response.create" {
                            continue;
                        }
                        count += 1;
                        observed.lock().unwrap().push(body.clone());
                        let id = format!("resp_ws_fixture_{count}");
                        let answer = format!("WEBSOCKET_ANSWER_{count}");
                        let events = [
                            json!({"type":"response.output_text.delta","delta":answer}),
                            json!({"type":"response.completed","response":{
                                "id":id,
                                "output":[{"type":"message","role":"assistant",
                                    "content":[{"type":"output_text","text":answer}]}],
                                "usage":{"input_tokens":40,
                                    "input_tokens_details":{"cached_tokens":0},
                                    "output_tokens":5,"total_tokens":45}
                            }}),
                        ];
                        for event in events {
                            if socket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                });
            }
        });
    });
    (port, requests)
}

/// Codex Responses websocket that answers the first request with a structured
/// `workspace_write` function call and the continuation with a final answer.
/// The continuation must carry the function-call output, not a text transcript.
fn scripted_codex_tool_provider() -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let observed = observed.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                        return;
                    };
                    let mut count = 0usize;
                    while let Some(Ok(message)) = socket.next().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(body) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if body["type"] != "response.create" {
                            continue;
                        }
                        count += 1;
                        observed.lock().unwrap().push(body.clone());
                        let id = format!("resp_ws_tool_{count}");
                        let events = if body.get("previous_response_id").is_none() {
                            let call = json!({
                                "type":"function_call",
                                "id":"fc_ws_tool_1",
                                "call_id":"call_ws_tool_1",
                                "name":"workspace_write",
                                "arguments":"{\"path\":\"ws_tool.txt\",\"content\":\"WS_TOOL_OK\"}"
                            });
                            vec![
                                json!({"type":"response.output_item.done","item":call.clone()}),
                                json!({"type":"response.completed","response":{
                                    "id":id,
                                    "output":[call],
                                    "usage":{"input_tokens":40,
                                        "input_tokens_details":{"cached_tokens":0},
                                        "output_tokens":5,"total_tokens":45}
                                }}),
                            ]
                        } else {
                            let answer = "웹소켓 도구 실행 완료";
                            vec![
                                json!({"type":"response.output_text.delta","delta":answer}),
                                json!({"type":"response.completed","response":{
                                    "id":id,
                                    "output":[{"type":"message","role":"assistant",
                                        "content":[{"type":"output_text","text":answer}]}],
                                    "usage":{"input_tokens":45,
                                        "input_tokens_details":{"cached_tokens":40},
                                        "output_tokens":5,"total_tokens":50}
                                }}),
                            ]
                        };
                        for event in events {
                            if socket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                });
            }
        });
    });
    (port, requests)
}

#[path = "e2e/room_identity.rs"]
mod room_identity;

#[path = "e2e/room_collaboration.rs"]
mod room_collaboration;

#[path = "e2e/run_recovery.rs"]
mod run_recovery;

#[path = "e2e/experiments_backup.rs"]
mod experiments_backup;

#[path = "e2e/adaptation_program.rs"]
mod adaptation_program;

#[path = "e2e/preflight_tools.rs"]
mod preflight_tools;

#[path = "e2e/http_execution.rs"]
mod http_execution;

#[path = "e2e/procedure_collaboration.rs"]
mod procedure_collaboration;

#[path = "e2e/codex_and_sdk.rs"]
mod codex_and_sdk;
