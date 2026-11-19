//! procedure collaboration E2E contracts backed by the shared real-process fixture.
use super::*;

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


#[test]
fn measure_room_input_growth_across_three_turns() {
    let _serial = serial();
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reply_counter = counter.clone();
    let (port, requests) = scripted_provider(move |_| {
        let n = reply_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        vec![json!({
            "choices":[{"delta":{"content":format!("응답 {n}")},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":100,"completion_tokens":5,"total_tokens":105}
        })]
    });
    let root = temp_root();
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let persona = "지침 문장입니다. ".repeat(220);
    let mut ids = Vec::new();
    for index in 1..=9 {
        let agent = app.post("/api/agents", &json!({
            "id":format!("m{index}"),
            "name":format!("참가자{index}"),
            "systemPrompt":format!("{persona}나는 참가자{index}입니다.")
        }));
        ids.push(agent["id"].as_str().unwrap().to_string());
    }
    app.post("/api/rooms", &json!({
        "id":"measure-room","name":"측정 방","agentIds":ids,
        "routingMode":"discussion","rounds":1,"maxTurns":10,
        "toolsEnabled":false,"autoLearning":false,"contextBudgetTokens":2000
    }));
    let mut seen = 0usize;
    for (round, text) in ["안녕", "오늘 기분은 어때", "점심은 뭐 먹을까"].iter().enumerate() {
        let sent = app.post("/api/rooms/measure-room/messages", &json!({"input":text}));
        let job_id = sent["jobId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let job = app.get(&format!("/api/jobs/{job_id}"));
            if job["status"] == "completed" { break; }
            assert_ne!(job["status"], "failed", "{job}");
            assert!(Instant::now() < deadline, "timeout: {job}");
            std::thread::sleep(Duration::from_millis(30));
        }
        let bodies = requests.lock().unwrap();
        let fresh: Vec<Value> = bodies[seen..].to_vec();
        seen = bodies.len();
        drop(bodies);
        let mut total = 0usize;
        for (index, body) in fresh.iter().enumerate() {
            let tokens = measure_estimate(&body["messages"].to_string());
            total += tokens;
            let prompt = body["messages"][0]["content"].as_str().unwrap_or("");
            let name = prompt.split("활성 에이전트: ").nth(1)
                .and_then(|rest| rest.split(" (id:").next()).unwrap_or("?");
            println!("round {} call {} agent={} estimate={} items={}", round + 1, index + 1, name, tokens, body["messages"].as_array().map(Vec::len).unwrap_or(0));
        }
        println!("round {} summary calls={} total_estimate={}", round + 1, fresh.len(), total);
    }
    let db = rusqlite::Connection::open_with_flags(
        root.join("runtime/dominion.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).unwrap();
    let mut statement = db.prepare("select data from records where kind='participant_conversation'").unwrap();
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
    for row in rows {
        let data: Value = serde_json::from_str(&row.unwrap()).unwrap();
        let view = &data["workingView"];
        let frames = view["frames"].as_array().cloned().unwrap_or_default();
        let mut contents = std::collections::HashMap::new();
        for frame in &frames {
            let text = frame["message"]["content"].as_str().unwrap_or("").to_string();
            if !text.is_empty() { *contents.entry(text).or_insert(0usize) += 1; }
        }
        let duplicates = contents.values().filter(|count| **count > 1).count();
        println!(
            "view agent={} epoch={} frames={} duplicate_contents={} estimate={}",
            data["agentId"].as_str().unwrap_or("?"),
            view["epoch"], frames.len(), duplicates,
            measure_estimate(&json!(frames).to_string())
        );
    }
}
