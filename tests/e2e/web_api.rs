//! HTTP/SSE requests, real workspace effects, task jobs and core harness.
use super::*;

#[test]
fn chat_sse_persistence_and_origin_guard() {
    let _serial = serial();
    let provider_port = start_mock_provider();
    let port = free_port();
    let root = temp_root();
    let binary = env!("CARGO_BIN_EXE_dominion-rust");

    let child = Command::new(binary)
        .env("DOMINION_PORT", port.to_string())
        .env("DOMINION_DATA", root.join("runtime"))
        .env("DOMINION_WEB", root.join("web"))
        .env("DOMINION_WORKSPACE", &root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    let _guard = ChildGuard(child);
    wait_health(port, 20);

    let client = reqwest::blocking::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let health: serde_json::Value = client
        .get(format!("{base}/api/health"))
        .send()
        .expect("health")
        .json()
        .expect("health json");
    assert_eq!(health["runtime"], "rust");

    let provider: serde_json::Value = client
        .post(format!("{base}/api/providers"))
        .json(&serde_json::json!({
            "name": "mock",
            "baseUrl": format!("http://127.0.0.1:{provider_port}/v1"),
            "apiKey": "test",
            "model": "mock",
            "enabled": true
        }))
        .send()
        .expect("provider")
        .json()
        .expect("provider json");
    assert!(provider["hasKey"].as_bool().unwrap_or(true));

    let session: serde_json::Value = client
        .post(format!("{base}/api/sessions"))
        .json(&serde_json::json!({ "title": "e2e" }))
        .send()
        .expect("session")
        .json()
        .expect("session json");
    let session_id = session["id"].as_str().expect("session id").to_string();

    let stream = client
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "sessionId": session_id,
            "input": "ping",
            "toolsEnabled": false
        }))
        .send()
        .expect("chat")
        .text()
        .expect("chat body");
    assert!(stream.contains("hello-e2e"), "stream: {stream}");
    assert!(stream.contains("\"done\""));

    let messages: serde_json::Value = client
        .get(format!("{base}/api/sessions/{session_id}/messages"))
        .send()
        .expect("messages")
        .json()
        .expect("messages json");
    let roles = messages
        .as_array()
        .expect("array")
        .iter()
        .map(|row| row["role"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    assert_eq!(roles, vec!["user", "assistant"]);

    let executions: serde_json::Value = client
        .get(format!("{base}/api/executions"))
        .send()
        .expect("executions")
        .json()
        .expect("executions json");
    assert_eq!(executions[0]["tokens"], 11);

    // Cross-origin writes are rejected; same-origin writes are allowed.
    let evil = client
        .post(format!("{base}/api/policy"))
        .header("Origin", "http://evil.example")
        .json(&serde_json::json!({ "autoApproveAll": true }))
        .send()
        .expect("evil");
    assert_eq!(evil.status(), reqwest::StatusCode::FORBIDDEN);

    let local = client
        .post(format!("{base}/api/policy"))
        .header("Origin", format!("http://127.0.0.1:{port}"))
        .json(&serde_json::json!({ "autoApproveAll": false }))
        .send()
        .expect("local");
    assert_eq!(local.status(), reqwest::StatusCode::OK);
}

#[test]
fn concurrent_turns_on_one_session_are_rejected() {
    let _serial = serial();
    let provider_port = start_mock_provider();
    let port = free_port();
    let root = temp_root();
    let binary = env!("CARGO_BIN_EXE_dominion-rust");

    let child = Command::new(binary)
        .env("DOMINION_PORT", port.to_string())
        .env("DOMINION_DATA", root.join("runtime"))
        .env("DOMINION_WEB", root.join("web"))
        .env("DOMINION_WORKSPACE", &root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    let _guard = ChildGuard(child);
    wait_health(port, 20);

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::new();
    client
        .post(format!("{base}/api/providers"))
        .json(&serde_json::json!({
            "name": "mock",
            "baseUrl": format!("http://127.0.0.1:{provider_port}/v1"),
            "apiKey": "test",
            "model": "mock",
            "enabled": true
        }))
        .send()
        .expect("provider");
    let session: serde_json::Value = client
        .post(format!("{base}/api/sessions"))
        .json(&serde_json::json!({ "title": "race" }))
        .send()
        .expect("session")
        .json()
        .expect("json");
    let session_id = session["id"].as_str().unwrap().to_string();

    let base_a = base.clone();
    let session_a = session_id.clone();
    let first = std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(format!("{base_a}/api/chat"))
            .json(&serde_json::json!({
                "sessionId": session_a, "input": "one", "toolsEnabled": false
            }))
            .send()
            .map(|response| response.text().unwrap_or_default())
            .unwrap_or_default()
    });
    std::thread::sleep(Duration::from_millis(30));
    let second = client
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "sessionId": session_id, "input": "two", "toolsEnabled": false
        }))
        .send()
        .map(|response| response.text().unwrap_or_default())
        .unwrap_or_default();
    let first = first.join().unwrap_or_default();

    let combined = format!("{first}{second}");
    assert!(
        combined.contains("Session is already running"),
        "expected a duplicate-session rejection, got: {combined}"
    );
}

#[test]
fn ordinary_chat_rejects_word_only_completion() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"gate-claim.txt 파일을 저장했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"claim-only"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-claim.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("실행 증거가 부족"), "{stream}");
    assert!(!stream.contains("assistant_saved"), "{stream}");
    assert!(!app._root.join("gate-claim.txt").exists());
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
}

#[test]
fn ordinary_chat_accepts_real_tool_evidence() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let saw_tool = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool");
        if saw_tool {
            vec![json!({"choices":[{"delta":{"content":"파일을 만들고 확인했습니다."},"finish_reason":"stop"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"write-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"gate-real.txt\",\"content\":\"evidence\"}"}}]},"finish_reason":"tool_calls"}]})]
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"real-evidence"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-real.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("실행 증거가 부족"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("gate-real.txt")).unwrap(),
        "evidence"
    );
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    let final_roles = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .count();
    assert!(final_roles >= 1);
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn native_job_prompt_runs_and_client_cannot_forge_status() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"job-done"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let created = app.post(
        "/api/jobs",
        &json!({
            "kind": "prompt",
            "input": {"prompt": "job prompt", "toolsEnabled": false},
            "status": "completed",
            "result": {"forged": true}
        }),
    );
    assert_eq!(created["status"], "queued");
    assert!(created["result"].is_null());
    let id = created["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut job = created;
    while Instant::now() < deadline {
        job = app.get(&format!("/api/jobs/{id}"));
        if matches!(job["status"].as_str(), Some("completed" | "failed")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    assert_eq!(job["status"], "completed", "{job}");
    let session_id = job["result"]["sessionId"]
        .as_str()
        .expect("job result session")
        .to_string();
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    assert!(messages
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["role"] == "assistant" && row["content"] == "job-done"));
}

#[test]
fn executions_are_read_only_and_scoped_cancel_only_hits_running() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"exec-done"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let forged = app
        .client
        .post(format!("{}/api/executions", app.base))
        .json(&json!({"status":"completed","tokens":999,"evidence":[{"fake":true}]}))
        .send()
        .unwrap();
    assert!(
        matches!(
            forged.status(),
            reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
        ),
        "unexpected status {}",
        forged.status()
    );
    assert_eq!(app.get("/api/executions").as_array().unwrap().len(), 0);

    let session = app.post("/api/sessions", &json!({"title":"exec"}));
    app.chat(&json!({"sessionId":session["id"],"input":"hello","autoLearning":false}));
    let executions = app.get("/api/executions");
    let execution = &executions[0];
    assert_eq!(execution["status"], "completed");
    let cancel = app
        .client
        .post(format!(
            "{}/api/executions/{}/cancel",
            app.base,
            execution["id"].as_str().unwrap()
        ))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(cancel.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn cancelling_a_running_execution_marks_it_cancelled() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_secs(4));
        vec![json!({"choices":[{"delta":{"content":"late"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"exec-cancel"}));
    let base = app.base.clone();
    let chat_id = session["id"].as_str().unwrap().to_string();
    let turn = std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(format!("{base}/api/chat"))
            .json(&json!({"sessionId":chat_id,"input":"slow","autoLearning":false}))
            .send()
            .unwrap()
            .text()
            .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut execution_id = None;
    while Instant::now() < deadline {
        let executions = app.get("/api/executions");
        if let Some(execution) = executions
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["status"] == "running")
        {
            execution_id = execution["id"].as_str().map(str::to_string);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let id = execution_id.expect("running execution");
    let cancelled = app
        .client
        .post(format!("{}/api/executions/{id}/cancel", app.base))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(cancelled.status(), reqwest::StatusCode::OK);
    let stream = turn.join().unwrap();
    assert!(stream.contains("중단") || stream.contains("error"), "{stream}");
    let stored = app.get(&format!("/api/executions/{id}"));
    assert_eq!(stored["status"], "cancelled");
}

#[test]
fn cancelling_a_running_job_stops_it() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_secs(4));
        vec![json!({"choices":[{"delta":{"content":"late job"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let created = app.post(
        "/api/jobs",
        &json!({"kind":"prompt","input":{"prompt":"slow job","toolsEnabled":false}}),
    );
    let id = created["id"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut running = false;
    while Instant::now() < deadline {
        let job = app.get(&format!("/api/jobs/{id}"));
        if job["status"] == "running" {
            running = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(running, "job never started");
    let cancelled = app
        .client
        .post(format!("{}/api/jobs/{id}/cancel", app.base))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(cancelled.status(), reqwest::StatusCode::OK);
    std::thread::sleep(Duration::from_millis(800));
    let job = app.get(&format!("/api/jobs/{id}"));
    assert_eq!(job["status"], "cancelled", "{job}");
    assert_eq!(job["cancelRequested"], true);
}

#[test]
fn room_manual_learning_enqueues_each_participant_with_its_own_watermark() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"참가자 발언"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.get("/api/providers")[0]["id"].as_str().unwrap().to_string();
    let agent_a = app.post(
        "/api/agents",
        &json!({"name":"A","systemPrompt":"A","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let agent_b = app.post(
        "/api/agents",
        &json!({"name":"B","systemPrompt":"B","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let room = app.post(
        "/api/rooms",
        &json!({"name":"learn-room","agentIds":[agent_a,agent_b],"rounds":1,"maxTurns":2,"toolsEnabled":false,"autoLearning":false}),
    );
    let room_id = room["id"].as_str().unwrap().to_string();
    let submitted = app.post(
        &format!("/api/rooms/{room_id}/messages"),
        &json!({"input":"참가자별 학습 확인"}),
    );
    let job_id = submitted["jobId"].as_str().unwrap().to_string();
    let session_id = submitted["sessionId"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut status = String::new();
    while Instant::now() < deadline {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        status = job["status"].as_str().unwrap_or("").to_string();
        if matches!(status.as_str(), "completed" | "failed") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(status, "completed", "room job did not finish");
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    let last_for = |agent: &str| {
        messages
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| message["agent_id"].as_str() == Some(agent))
            .and_then(|message| message["id"].as_str().map(str::to_string))
    };
    let through_a = last_for(&agent_a).expect("agent A spoke");
    let through_b = last_for(&agent_b).expect("agent B spoke");

    let learn = app.post(&format!("/api/sessions/{session_id}/learn"), &json!({}));
    assert_eq!(learn["status"], "queued", "{learn}");
    let jobs = learn["jobs"].as_array().expect("per-agent jobs");
    assert_eq!(jobs.len(), 2, "{learn}");
    for job in jobs {
        let agent = job["agentId"].as_str().unwrap();
        let expected = if agent == agent_a { &through_a } else { &through_b };
        let record = app.get(&format!("/api/jobs/{}", job["jobId"].as_str().unwrap()));
        assert_eq!(record["input"]["agentId"].as_str().unwrap(), agent);
        assert_eq!(record["input"]["throughMessageId"].as_str().unwrap(), expected);
        assert_eq!(record["input"]["providerId"].as_str().unwrap(), provider);
    }
}

#[test]
fn token_budget_is_enforced_from_stream_usage() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        if request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool")
        {
            return vec![json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]})];
        }
        vec![json!({
            "choices":[{"delta":{"content":"big"},"finish_reason":"stop"}],
            "usage":{"total_tokens":300000}
        })]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"budget"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "budget check",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(stream.contains("토큰 예산"), "{stream}");
    assert!(!stream.contains("assistant_saved"), "{stream}");
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
}

#[test]
fn verified_work_completes_without_waiting_for_the_model_to_stop() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let has_tools = request.get("tools").and_then(|tools| tools.as_array()).is_some();
        if !has_tools {
            if system.contains("planner") {
                return vec![json!({"choices":[{"delta":{"content":"1. 파일 생성\\n2. 확인"}}]})];
            }
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        let arguments = json!({"path":"gate-limit.txt","content":"limit-ok"}).to_string();
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"write-limit","type":"function","function":{"name":"workspace_write","arguments":arguments}}]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"post-tool"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-limit.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("모델 반복 제한"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("gate-limit.txt")).unwrap(),
        "limit-ok"
    );
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn compat_mode_executes_a_bare_json_tool_call_from_a_weak_model() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let saw_tool = messages.iter().any(|message| {
            message["role"] == "tool"
                || message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("Tool observation"))
        });
        if system.contains("planner") {
            return vec![json!({"choices":[{"delta":{"content":"1. weak.txt 생성\\n2. 확인"}}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"파일을 실제로 만들었습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"{\"tool\":\"workspace_write\",\"args\":{\"path\":\"weak.txt\",\"content\":\"4b-ok\"}}"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "compat");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"weak-model"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "weak.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("weak.txt")).unwrap(),
        "4b-ok"
    );
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn delegate_task_runs_a_real_child_agent_and_returns_its_result() {
    let _serial = serial();
    use std::sync::{Arc, Mutex};
    let child_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let slot = child_slot.clone();
    let (port, _) = scripted_provider(move |request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if system.contains("CHILD-AGENT") {
            return vec![json!({"choices":[{"delta":{"content":"child result 42"},"finish_reason":"stop"}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"자식 결과를 받아 완료했습니다."},"finish_reason":"stop"}]})];
        }
        let child = slot.lock().unwrap().clone().unwrap_or_default();
        let arguments = json!({
            "agentId": child,
            "prompt": "compute the answer",
            "wait": true,
            "timeoutSeconds": 60
        })
        .to_string();
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"delegate-1","type":"function","function":{"name":"delegate_task","arguments":arguments}}]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.get("/api/providers")[0]["id"].as_str().unwrap().to_string();
    let parent = app.post(
        "/api/agents",
        &json!({"name":"Parent","systemPrompt":"PARENT-AGENT","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let child = app.post(
        "/api/agents",
        &json!({"name":"Child","systemPrompt":"CHILD-AGENT","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    *child_slot.lock().unwrap() = Some(child.clone());

    let session = app.post(
        "/api/sessions",
        &json!({"title":"delegation","agentId":parent}),
    );
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "agentId": parent,
        "input": "자식 에이전트에게 계산을 위임해",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("실행 증거가 부족"), "{stream}");
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    let tool_text = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| message["content"].as_str().unwrap_or(""))
        .collect::<String>();
    assert!(tool_text.contains("child result 42"), "{tool_text}");
    let child_sessions = app.get("/api/sessions");
    let child_session = child_sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["title"].as_str().unwrap_or("").starts_with("Delegate ·"))
        .expect("child session");
    let child_messages = app.get(&format!(
        "/api/sessions/{}/messages",
        child_session["id"].as_str().unwrap()
    ));
    assert!(child_messages
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "assistant"
            && message["content"] == "child result 42"));
    assert!(app
        .get("/api/jobs")
        .as_array()
        .unwrap()
        .iter()
        .any(|job| job["kind"] == "prompt" && job["status"] == "completed"));
}

#[test]
fn correction_turn_failure_still_enqueues_reflection() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let is_reflection = messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("실패를 분석하는 회고 분석기")
        });
        if is_reflection {
            return vec![json!({"choices":[{"delta":{"content":
                "{\"rootCause\":\"도구를 쓰지 않고 말로만 끝냈다\",\"expected\":\"파일 생성\",\"actual\":\"설명만 함\",\"lesson\":\"행동 요청은 도구 결과로 검증한다\",\"avoid\":\"도구 호출 없이 완료 선언\",\"strategy\":null,\"skill\":null,\"memory\":null}"
            },"finish_reason":"stop"}]})];
        }
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("")
            .to_string();
        if last_user.contains("틀렸") {
            // The corrective turn itself fails: reflection must still be queued.
            vec![json!({"choices":[{"delta":{},"finish_reason":"content_filter"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"content":"설명만 했습니다."},"finish_reason":"stop"}]})]
        }
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"reflect on failure"}));
    let session_id = session["id"].as_str().unwrap();
    let first = app.chat(&json!({
        "sessionId": session_id, "input": "러스트의 장점을 한 문장으로 말해",
        "toolsEnabled": false, "autoLearning": false
    }));
    assert!(first.contains("assistant_saved"), "{first}");

    let second = app.chat(&json!({
        "sessionId": session_id, "input": "아니 그게 아니라, 틀렸어. 다시 해.",
        "toolsEnabled": false, "autoLearning": true
    }));
    assert!(second.contains("error"), "{second}");

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut reflect_status = String::new();
    while Instant::now() < deadline {
        if let Some(job) = app
            .get("/api/jobs")
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["kind"] == "reflect")
        {
            reflect_status = job["status"].as_str().unwrap_or("").to_string();
            if matches!(reflect_status.as_str(), "completed" | "failed") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        reflect_status, "completed",
        "reflection did not complete after failed correction turn"
    );
    let experience = app.get("/api/experience");
    assert!(
        !experience.as_array().unwrap().is_empty(),
        "reflection produced no experience"
    );
}

#[test]
fn health_reports_running_binary_identity_instead_of_moving_checkout_head(){
    let _serial=serial();
    let (port,_)=scripted_provider(|_|vec![json!({
        "choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]})]);
    let app=TestApp::new(port,"native");
    let health=app.get("/api/health");
    assert_eq!(health["binaryBuildSha"],health["commit"]);
    assert_ne!(health["identitySource"],"source-checkout",
        "health cannot call checkout HEAD the identity of an old binary");
    match health["identitySource"].as_str() {
        Some("embedded-build"|"embedded-release")=>{
            let commit=health["binaryBuildSha"].as_str().unwrap();
            assert_eq!(commit.len(),40);
            assert!(commit.bytes().all(|b|b.is_ascii_hexdigit()));
        },
        Some("unavailable")=>assert_eq!(health["binaryBuildSha"],"unknown"),
        other=>panic!("unrecognized binary identity source: {other:?}")
    }
}

#[test]
fn sub3_standalone_explanation_uses_zero_tool_schemas_over_real_http_sse() {
    let _serial=serial();
    let (port, observed)=scripted_provider(|_| {
        vec![json!({"choices":[{"index":0,"finish_reason":"stop",
            "message":{"role":"assistant","content":"양자 얽힘은 두 계의 상관관계입니다."}}],
            "usage":{"prompt_tokens":20,"completion_tokens":12,
                "total_tokens":32}})]
    });
    let app=TestApp::new(port,"native");
    let session=app.post("/api/sessions",&json!({"title":"sub3 lightweight question"}));
    let stream=app.chat(&json!({"sessionId":session["id"],
        "input":"양자 얽힘이 뭐야?","toolsEnabled":true,
        "toolsExecutionEnabled":true,"autoLearning":false}));
    assert!(stream.contains("assistant_saved"),"Chat did not settle: {stream}");
    // A provider's final-message-only SSE may be persisted without echoing
    // its settled body on the streaming transport. Assert the durable answer
    // below instead of requiring a duplicate live text delta.
    let requests=observed.lock().unwrap();
    assert_eq!(requests.len(),1,"A standalone explanation must make exactly one paid model call");
    assert!(requests[0]["tools"].as_array().is_none_or(Vec::is_empty),
        "MCP schemas must not be sent for a context-free explanation: {}",requests[0]);
    drop(requests);
    let messages=app.get(&format!("/api/sessions/{}/messages",
        session["id"].as_str().unwrap()));
    assert!(messages.as_array().unwrap().iter().any(|message|
        message["role"]=="assistant" &&
        message["content"].as_str().unwrap_or("").contains("양자 얽힘")));
}

#[test]
fn completed_chat_without_verification_is_not_a_success() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"안녕하세요."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"plain chat"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "간단히 인사해",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    let executions = app.get("/api/executions");
    assert_eq!(executions[0]["status"], "completed");
    assert_eq!(executions[0]["taskOutcome"], "answered_unverified");
    assert!(app.get("/api/experience").as_array().unwrap().is_empty());
}

#[test]
fn tools_disabled_action_is_failed_not_succeeded() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"만들었습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"disabled tools"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "report.txt 파일을 만들어줘",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(stream.contains("error"), "{stream}");
    let executions = app.get("/api/executions");
    assert_eq!(executions[0]["status"], "failed");
    assert_eq!(executions[0]["taskOutcome"], "failed");
}

#[test]
fn procedure_violation_blocks_the_target_and_repairs_with_the_prerequisite() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("회고 분석기") {
            let reflection = json!({
                "rootCause": "audit를 먼저 만들지 않고 report를 작성했다",
                "expected": "audit 먼저",
                "actual": "report만 작성",
                "lesson": "report를 쓰기 전에 audit을 먼저 쓴다",
                "avoid": "선행 작업을 생략하지 않는다",
                "strategy": {
                    "text": "report를 쓰기 전에 audit을 먼저 쓴다",
                    "rationale": "주인의 교정에서 확인된 선행 조건",
                    "verification": {
                        "type": "procedure",
                        "enforce": true,
                        "appliesTo": { "requestContains": ["report.txt"] },
                        "before": [{
                            "when": { "tool": "workspace_write", "path": "report.txt" },
                            "require": { "tool": "workspace_write", "path": "audit.txt" }
                        }],
                        "after": [{
                            "kind": "file_contains",
                            "path": "audit.txt",
                            "text": "audit-ok"
                        }]
                    }
                },
                "skill": null,
                "memory": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":reflection},"finish_reason":"stop"}]})];
        }
        if system.contains("증거 기반 학습기") {
            let empty = json!({
                "memories": [],
                "skill": null,
                "strategies": [],
                "toolPolicies": [],
                "harnessProposal": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":empty},"finish_reason":"stop"}]})];
        }
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if last_user.contains("아니") {
            return vec![json!({"choices":[{"delta":{"content":"알겠습니다."},"finish_reason":"stop"}]})];
        }
        let last_tool = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if last_tool.contains("procedureViolation") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"audit-call","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"audit.txt\",\"content\":\"audit-ok\"}"}}]},"finish_reason":"tool_calls"}]})];
        }
        if last_tool.contains("audit.txt") && !last_tool.contains("report.txt") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"report-call","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"report.txt\",\"content\":\"ok\"}"}}]},"finish_reason":"tool_calls"}]})];
        }
        if last_tool.contains("report.txt") {
            return vec![json!({"choices":[{"delta":{"content":"절차를 지켜 완료했습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"first-report","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"report.txt\",\"content\":\"ok\"}"}}]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll": true}));

    let session = app.post("/api/sessions", &json!({"title":"procedure"}));
    let session_id = session["id"].as_str().unwrap();
    let baseline = app.chat(&json!({
        "sessionId": session_id,
        "input": "report.txt 파일을 만들고 내용을 ok로 저장해.",
        "toolsEnabled": true,
        "autoLearning": false
    }));
    assert!(baseline.contains("assistant_saved"), "{baseline}");

    let correction = app.chat(&json!({
        "sessionId": session_id,
        "input": "아니, report.txt를 쓰기 전에 audit.txt를 먼저 만들어야 해. 다시 해.",
        "toolsEnabled": false,
        "autoLearning": true
    }));
    assert!(correction.contains("error"), "{correction}");

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut reflected = false;
    while Instant::now() < deadline {
        if app
            .get("/api/jobs")
            .as_array()
            .unwrap()
            .iter()
            .any(|job| job["kind"] == "reflect" && job["status"] == "completed")
        {
            reflected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(reflected, "reflection did not complete");

    let _ = std::fs::remove_file(app._root.join("report.txt"));
    let _ = std::fs::remove_file(app._root.join("audit.txt"));
    let after = app.post("/api/sessions", &json!({"title":"procedure after"}));
    let after_id = after["id"].as_str().unwrap();
    let stream = app.chat(&json!({
        "sessionId": after_id,
        "input": "report.txt 파일을 만들고 내용을 ok로 저장해.",
        "toolsEnabled": true,
        "autoLearning": false
    }));
    assert!(
        stream.contains("절차 위반") || stream.contains("procedureViolation"),
        "{stream}"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("audit.txt")).unwrap_or_default(),
        "audit-ok"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("report.txt")).unwrap_or_default(),
        "ok"
    );
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == after_id)
        .expect("post execution");
    assert_eq!(execution["taskOutcome"], "succeeded", "{}", execution);
    let compliance = execution["strategyCompliance"].as_array().expect("compliance");
    assert!(compliance.iter().any(|item| {
        item["applicable"] == true
            && item["result"]["verified"] == true
            && item["result"]["followed"] == true
    }));
}

#[test]
fn harness_assisted_executes_dag_nodes_through_the_common_dispatch() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("회고 분석기") {
            let reflection = json!({
                "rootCause": "선행 audit 없이 report를 작성했다",
                "expected": "audit 먼저",
                "actual": "report만 작성",
                "lesson": "harness 절차로 audit 후 report를 실행한다",
                "avoid": "선행 작업 생략",
                "strategy": {
                    "text": "harness 절차로 audit 후 report를 실행한다",
                    "rationale": "주인의 교정에서 확인된 선행 조건",
                    "verification": {
                        "type":"procedure",
                        "appliesTo":{"paths":["harness_report.txt"]},
                        "bindings":{},
                        "nodes":[
                            {"id":"audit","tool":"workspace_write","arguments":{"path":"harness_audit.txt","content":"audit-harness"}},
                            {"id":"report","tool":"workspace_write","arguments":{"path":"harness_report.txt","content":"REPORT-OK"}}
                        ],
                        "edges":[{"from":"audit","to":"report"}],
                        "after":[{"kind":"file_contains","path":"harness_audit.txt","text":"audit-harness"}]
                    }
                },
                "skill": null,
                "memory": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":reflection},"finish_reason":"stop"}]})];
        }
        if system.contains("증거 기반 학습기") {
            let empty = json!({
                "memories": [], "skill": null, "strategies": [],
                "toolPolicies": [], "harnessProposal": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":empty},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"확인했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll": true}));

    let session = app.post("/api/sessions", &json!({"title":"harness setup"}));
    let session_id = session["id"].as_str().unwrap();
    app.chat(&json!({
        "sessionId": session_id,
        "input": "간단히 설명해",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    let correction = app.chat(&json!({
        "sessionId": session_id,
        "input": "아니, harness_report.txt를 만들기 전에 harness_audit.txt를 먼저 만들어. 다시 해.",
        "toolsEnabled": false,
        "autoLearning": true
    }));
    assert!(correction.contains("error"), "{correction}");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if app.get("/api/jobs").as_array().unwrap().iter().any(|job| {
            job["kind"] == "reflect" && job["status"] == "completed"
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = std::fs::remove_file(app._root.join("harness_report.txt"));
    let _ = std::fs::remove_file(app._root.join("harness_audit.txt"));
    let disabled = app.post("/api/sessions", &json!({"title":"harness disabled"}));
    let disabled_id = disabled["id"].as_str().unwrap();
    let disabled_stream = app.chat(&json!({
        "sessionId": disabled_id,
        "input": "harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(!disabled_stream.contains("error"), "{disabled_stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_audit.txt")).unwrap(),
        "audit-harness"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_report.txt")).unwrap(),
        "REPORT-OK"
    );
    let executions = app.get("/api/executions");
    let disabled_execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == disabled_id)
        .expect("disabled execution");
    assert_eq!(disabled_execution["taskOutcome"], "succeeded");
    let disabled_operations = app.get(&format!(
        "/api/executions/{}/operations",
        disabled_execution["id"].as_str().unwrap()
    ));
    assert!(disabled_operations
        .as_array()
        .unwrap()
        .iter()
        .any(|operation| operation["origin"] == "procedure"));

    let _ = std::fs::remove_file(app._root.join("harness_report.txt"));
    let _ = std::fs::remove_file(app._root.join("harness_audit.txt"));
    let blocked = app.post("/api/sessions", &json!({"title":"harness execution blocked"}));
    let blocked_id = blocked["id"].as_str().unwrap();
    let blocked_stream = app.chat(&json!({
        "sessionId": blocked_id,
        "input": "harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "toolsEnabled": true,
        "toolsExecutionEnabled": false,
        "autoLearning": false
    }));
    assert!(blocked_stream.contains("error"), "{blocked_stream}");
    assert!(!app._root.join("harness_report.txt").exists());
    assert!(!app._root.join("harness_audit.txt").exists());
    let executions = app.get("/api/executions");
    let blocked_execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == blocked_id)
        .expect("blocked execution");
    let blocked_operations = app.get(&format!(
        "/api/executions/{}/operations",
        blocked_execution["id"].as_str().unwrap()
    ));
    assert!(!blocked_operations
        .as_array()
        .unwrap()
        .iter()
        .any(|operation| operation["origin"] == "procedure"));

    let post = app.post("/api/sessions", &json!({"title":"harness post"}));
    let post_id = post["id"].as_str().unwrap();
    let stream = app.chat(&json!({
        "sessionId": post_id,
        "input": "harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "toolsEnabled": true,
        "autoLearning": false
    }));
    assert!(!stream.contains("error"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_audit.txt")).unwrap(),
        "audit-harness"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_report.txt")).unwrap(),
        "REPORT-OK"
    );
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == post_id)
        .expect("post execution");
    assert_eq!(execution["taskOutcome"], "succeeded");
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        execution["id"].as_str().unwrap()
    ));
    let nodes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["origin"] == "procedure")
        .filter_map(|operation| operation["procedureNodeId"].as_str())
        .collect::<Vec<_>>();
    assert!(nodes.contains(&"audit"));
    assert!(nodes.contains(&"report"));
}

