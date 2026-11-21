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
        panic!("approval was not requested; approvals={}; executions={}",
            self.get("/api/approvals"),self.get("/api/executions"));
    }
}


// A single lock and process fixture remain shared by every real E2E suite.
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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


// Domain-specific E2E suites run in the SAME test executable and share
// the original binary, API, mock provider and global serialization guard.
#[path = "e2e/rooms.rs"]
mod rooms;
#[path = "e2e/durability.rs"]
mod durability;
#[path = "e2e/adaptation.rs"]
mod adaptation;
#[path = "e2e/web_api.rs"]
mod web_api;
#[path = "e2e/continuity.rs"]
mod continuity;
