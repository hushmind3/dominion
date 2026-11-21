//! http execution E2E contracts backed by the shared real-process fixture.
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
fn direct_chat_answers_without_agent_tools_or_action_execution() {
    let _serial = serial();
    let (port, calls) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"일반 채팅에서는 실행하지 않고 설명만 드립니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    // The stored agent is deliberately missing. Direct chat must not try to
    // resolve it or interpret a file request as an agent job.
    let session = app.post("/api/sessions", &json!({
        "title":"direct conversation","agentId":"missing-agent"
    }));
    let id = session["id"].as_str().unwrap();
    let stream = app.chat(&json!({
        "sessionId":id,"input":"report.txt 파일을 만들어줘",
        "agentId":"missing-agent","directChat":true,
        "toolsEnabled":true,"toolsExecutionEnabled":true,
        "autoLearning":true
    }));
    assert!(stream.contains("owner_saved"), "{stream}");
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!app._root.join("report.txt").exists());
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "a direct chat requires just one model call");
    let wire = &calls[0];
    assert!(wire["tools"].as_array().is_none_or(Vec::is_empty));
    let prompt = wire["messages"][0]["content"].as_str().unwrap_or("");
    assert!(prompt.contains("DOMINION 대화형 AI"), "{prompt}");
    assert!(!prompt.contains("에이전트형 AI 작업 공간"), "{prompt}");
    drop(calls);

    let saved = app.get(&format!("/api/sessions/{id}"));
    assert_eq!(saved["chatMode"], "chat");
    assert!(saved["agentId"].is_null());
    let messages = app.get(&format!("/api/sessions/{id}/messages"));
    assert_eq!(messages.as_array().unwrap().len(), 2);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(app.get("/api/executions")[0]["taskOutcome"], "answered_unverified");
}





#[test]
fn client_turn_idempotency_prevents_duplicate_owner_effects_and_model_calls() {
    let _serial=serial();
    let (port,calls)=scripted_provider(|_|{
        vec![json!({"choices":[{"delta":{"content":"응답을 기록했습니다."},"finish_reason":"stop"}]})]
    });
    let app=TestApp::new(port,"native");
    let session=app.post("/api/sessions",&json!({"title":"idempotency"}));
    let id=session["id"].as_str().unwrap();
    let key="00000000-0000-4000-8000-000000000042";
    let first=app.chat(&json!({
        "sessionId":id,"input":"첫 요청","clientTurnId":key,
        "directChat":true,"toolsEnabled":false
    }));
    assert!(first.contains("owner_saved"),"{first}");
    assert!(first.contains("assistant_saved"),"{first}");

    let duplicate=app.chat(&json!({
        "sessionId":id,"input":"첫 요청","clientTurnId":key,
        "directChat":true,"toolsEnabled":false
    }));
    assert!(duplicate.contains("동일한 요청이 이미 저장되었습니다"),"{duplicate}");
    assert!(!duplicate.contains("assistant_saved"),"{duplicate}");
    assert_eq!(calls.lock().unwrap().len(),1);
    let messages=app.get(&format!("/api/sessions/{id}/messages"));
    assert_eq!(messages.as_array().unwrap().len(),2);
    assert_eq!(messages[0]["metadata"]["clientTurnId"],key);

    // New keys are new logical submissions, even if the text repeats.
    let third=app.chat(&json!({
        "sessionId":id,"input":"첫 요청",
        "clientTurnId":"00000000-0000-4000-8000-000000000043",
        "directChat":true,"toolsEnabled":false
    }));
    assert!(third.contains("assistant_saved"),"{third}");
    assert_eq!(calls.lock().unwrap().len(),2);
    assert_eq!(app.get(&format!("/api/sessions/{id}/messages")).as_array().unwrap().len(),4);
}



#[test]
fn duplicate_client_turn_id_cannot_repeat_a_real_agent_file_write() {
    let _serial=serial();
    let (port,requests)=scripted_provider(|request|{
        let saw_tool=request["messages"].as_array()
            .is_some_and(|messages|messages.iter().any(|message|message["role"]=="tool"));
        if saw_tool {
            vec![json!({"choices":[{"delta":{"content":"파일 생성을 확인했습니다."},"finish_reason":"stop"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"tool_calls":[{
                "index":0,"id":"write-once","type":"function",
                "function":{"name":"workspace_write",
                    "arguments":"{\"path\":\"single-write.txt\",\"content\":\"created-once\"}"}
            }]},"finish_reason":"tool_calls"}]})]
        }
    });
    let app=TestApp::new(port,"native");
    app.post("/api/policy",&json!({"autoApproveAll":true}));
    let session=app.post("/api/sessions",&json!({"title":"write exactly once"}));
    let id=session["id"].as_str().unwrap();
    let key="00000000-0000-4000-8000-000000000044";
    let body=json!({
        "sessionId":id,"input":"single-write.txt 파일을 만들어줘",
        "clientTurnId":key,"toolsEnabled":true,"autoLearning":false
    });
    let first=app.chat(&body);
    assert!(first.contains("assistant_saved"),"{first}");
    assert_eq!(std::fs::read_to_string(app._root.join("single-write.txt")).unwrap(),"created-once");
    let calls_before=requests.lock().unwrap().len();
    assert!(calls_before>=2);
    let duplicate=app.chat(&body);
    assert!(duplicate.contains("동일한 요청이 이미 저장되었습니다"),"{duplicate}");
    assert!(!duplicate.contains("assistant_saved"),"{duplicate}");
    assert_eq!(requests.lock().unwrap().len(),calls_before);
    assert_eq!(std::fs::read_to_string(app._root.join("single-write.txt")).unwrap(),"created-once");
}


#[test]
fn direct_chat_search_request_remains_a_model_reply_without_web_tool_or_error() {
    let _serial=serial();
    let (port,requests)=scripted_provider(|_|{
        vec![json!({"choices":[{"delta":{"content":"현재 웹 검색은 실행되지 않았습니다."},"finish_reason":"stop"}]})]
    });
    let app=TestApp::new(port,"native");
    let session=app.post("/api/sessions",&json!({"title":"model-only search wording"}));
    let id=session["id"].as_str().unwrap();
    let response=app.chat(&json!({
        "sessionId":id,"input":"2026년 최신 기술을 웹에서 검색해",
        "directChat":true,"toolsEnabled":true,"toolsExecutionEnabled":true
    }));
    assert!(response.contains("owner_saved"),"{response}");
    assert!(response.contains("assistant_saved"),"{response}");
    assert!(!response.contains("도구 실행이 꺼져 있습니다"),"{response}");
    let requests=requests.lock().unwrap();
    assert_eq!(requests.len(),1);
    assert!(requests[0]["tools"].as_array().is_none_or(Vec::is_empty));
    drop(requests);
    let executions=app.get("/api/executions");
    assert_eq!(executions[0]["toolCalls"],0);
    assert_eq!(executions[0]["taskOutcome"],"answered_unverified");
}


#[test]
fn chat_mode_switch_removes_and_restores_agent_persona_without_losing_history() {
    let _serial = serial();
    let (port, calls) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"답변했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let agent = app.post("/api/agents", &json!({
        "name":"switch-agent","systemPrompt":"AGENT_PERSONA_MUST_ONLY_RUN_IN_AGENT_MODE"
    }));
    let agent_id = agent["id"].as_str().unwrap();
    let session = app.post("/api/sessions", &json!({"title":"switch","agentId":agent_id}));
    let id = session["id"].as_str().unwrap();

    let agent_first = app.chat(&json!({
        "sessionId":id,"agentId":agent_id,"input":"첫 번째 인사",
        "toolsEnabled":false,"autoLearning":false
    }));
    assert!(agent_first.contains("assistant_saved"), "{agent_first}");
    let plain = app.chat(&json!({
        "sessionId":id,"agentId":agent_id,"input":"두 번째 인사",
        "directChat":true,"toolsEnabled":true,"autoLearning":true
    }));
    assert!(plain.contains("assistant_saved"), "{plain}");
    let state = app.get(&format!("/api/sessions/{id}"));
    assert_eq!(state["chatMode"], "chat");
    assert!(state["agentId"].is_null());

    let agent_again = app.chat(&json!({
        "sessionId":id,"agentId":agent_id,"input":"세 번째 인사",
        "toolsEnabled":false,"autoLearning":false
    }));
    assert!(agent_again.contains("assistant_saved"), "{agent_again}");
    let state = app.get(&format!("/api/sessions/{id}"));
    assert_eq!(state["chatMode"], "agent");
    assert_eq!(state["agentId"], agent_id);

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    let system = |index:usize|calls[index]["messages"][0]["content"]
        .as_str().unwrap_or("").to_string();
    assert!(system(0).contains("AGENT_PERSONA_MUST_ONLY_RUN_IN_AGENT_MODE"));
    assert!(!system(1).contains("AGENT_PERSONA_MUST_ONLY_RUN_IN_AGENT_MODE"));
    assert!(system(2).contains("AGENT_PERSONA_MUST_ONLY_RUN_IN_AGENT_MODE"));
    let messages = app.get(&format!("/api/sessions/{id}/messages"));
    assert_eq!(messages.as_array().unwrap().len(), 6);
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
#[ignore = "acceptance repair now skips the paid reviewer while a typed receipt frontier makes progress; this fixture never stalls, so no strategy improvement is proposed. Rewrite for the stalled-frontier path."]
fn reviewer_repairs_a_claim_with_a_real_tool() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let has_tools = request.get("tools").and_then(|tools| tools.as_array()).is_some();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");

        if !has_tools {
            if system.contains("strict reviewer") {
                return vec![json!({"choices":[{"delta":{"content":"{\"complete\":false,\"gaps\":[\"gate-review.txt가 없습니다\"],\"nextAction\":\"workspace_write로 gate-review.txt를 생성\"}"}}]})];
            }
            return vec![json!({"choices":[{"delta":{"content":"1. gate-review.txt 생성\n2. 존재 확인"}}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"파일을 만들고 확인했습니다."},"finish_reason":"stop"}]})];
        }
        if last_user.contains("다음 전략") || last_user.contains("자동 검증") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"review-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"gate-review.txt\",\"content\":\"reviewed\"}"}}]},"finish_reason":"tool_calls"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"gate-review.txt 파일을 저장했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"reviewer-repair"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-review.txt 파일을 만들어줘. 내용은 reviewed로 저장해줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("실행 증거가 부족"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("gate-review.txt")).unwrap(),
        "reviewed"
    );
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
    let improvements = app.get("/api/improvements");
    assert!(
        improvements.as_array().unwrap().iter().any(|item| {
            item["kind"] == "strategy"
                && item["status"] == "adopted"
                && item["text"].as_str().unwrap_or("").contains("workspace_write")
        }),
        "reviewer strategy was not adopted: {improvements}"
    );
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
fn benchmark_identity_reports_the_isolated_roots() {
    let _serial = serial();
    let port = start_mock_provider();
    let app = TestApp::new_with_env(
        port,
        "native",
        &[
            ("DOMINION_BENCHMARK", "1"),
            ("DOMINION_ENGINE_INSTANCE", "bench-test"),
        ],
    );
    let identity = app.get("/api/benchmark/identity");
    assert_eq!(identity["benchmark"], true);
    assert_eq!(identity["instanceId"], "bench-test");
    let workspace = app._root.canonicalize().unwrap();
    assert_eq!(
        identity["workspace"].as_str().unwrap(),
        workspace.to_string_lossy()
    );
    let data = app._root.join("runtime").canonicalize().unwrap();
    assert_eq!(
        identity["dataRoot"].as_str().unwrap(),
        data.to_string_lossy()
    );
}

