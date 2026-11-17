//! Adaptive repair, parallel tool calls, recovery and provider failover.
use super::*;

#[test]
fn a_failed_call_is_not_repeated_and_the_model_adapts() {
    let _serial = serial();
    let calls = Arc::new(Mutex::new(0u32));
    let counter = calls.clone();
    let (port, _) = scripted_provider(move |_| {
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fail-1","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fail-2","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            3 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"adapt-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"adapted.txt\",\"content\":\"OK\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"다른 방법으로 완료했습니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _stream = app.chat(&json!({"input":"adapted.txt 파일을 만들어줘","autoLearning":false}));
    assert_eq!(
        std::fs::read_to_string(app._root.join("adapted.txt")).unwrap_or_default(),
        "OK"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let adaptations = execution["adaptations"].as_array().cloned().unwrap_or_default();
    assert!(
        adaptations.iter().any(|item| item["observation"]["tool"] == "shell_exec"),
        "adaptation was not recorded: {execution}"
    );
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let shell_calls = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "shell_exec")
        .count();
    assert_eq!(shell_calls, 1, "failed call was re-executed: {operations}");
    let writes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "workspace_write")
        .count();
    assert_eq!(writes, 1, "adapted action did not run: {operations}");
    let write = operations
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["tool"] == "workspace_write")
        .unwrap();
    assert_eq!(write["effectState"], "verified", "{write}");
    assert_eq!(write["toolReceipt"]["verified"], true, "{write}");
    assert_eq!(
        write["toolReceipt"]["verifier"]["kind"],
        "sha256_readback",
        "{write}"
    );
    assert!(
        write["observedVersion"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")),
        "{write}"
    );
    assert_eq!(
        execution["effectSummary"]["verifiedEffects"],
        1,
        "{execution}"
    );
    let failed = operations
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["tool"] == "shell_exec")
        .unwrap();
    assert_eq!(failed["effectState"], "failed", "{failed}");
    assert_eq!(
        failed["toolReceipt"]["failureLayer"], "execution",
        "{failed}"
    );
}

#[test]
fn a_fully_specified_replacement_runs_without_a_model_call() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"MODEL SHOULD NOT RUN"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    std::fs::create_dir_all(app._root.join("bench")).unwrap();
    std::fs::write(app._root.join("bench/frozen.txt"), "start ALPHA end\n").unwrap();
    let _stream = app.chat(&json!({
        "input":"edit `bench/frozen.txt` and replace `ALPHA` with `BETA` and preserve everything else.",
        "autoLearning":false
    }));
    assert_eq!(
        std::fs::read_to_string(app._root.join("bench/frozen.txt")).unwrap(),
        "start BETA end\n"
    );
    assert!(
        requests.lock().unwrap().is_empty(),
        "the model was called for a frozen transformation"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    assert_eq!(execution["taskOutcome"], "succeeded", "{execution}");
    assert_eq!(execution["modelCalls"], 0, "{execution}");
    assert_eq!(
        execution["deterministicShortcut"]["modelCalls"], 0,
        "{execution}"
    );
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        execution["id"].as_str().unwrap()
    ));
    let items = operations.as_array().unwrap();
    let write = items
        .iter()
        .find(|operation| operation["tool"] == "file_write")
        .unwrap();
    assert_eq!(write["effectState"], "verified", "{write}");
    assert_eq!(
        items
            .iter()
            .filter(|operation| operation["tool"] == "file_read")
            .count(),
        1
    );
    let conn = Connection::open(app._root.join("runtime/dominion.sqlite")).unwrap();
    let rules: i64 = conn
        .query_row(
            "SELECT count(*) FROM records WHERE kind='experience_rule'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rules, 0, "a deterministic shortcut became learning evidence");
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

#[test]
fn manual_learn_requests_semantic_extraction() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"{\"memories\":[],\"skill\":null}"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"semantic learn"}));
    let session_id = session["id"].as_str().unwrap();
    let path = app._root.join("runtime/dominion.sqlite");
    let conn = Connection::open(path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO messages(id,session_id,role,content,agent_id) VALUES(?1,?2,'user',?3,'a')",
        rusqlite::params!["learn-user", session_id, "앞으로 답변은 한국어로만 해"],
    ).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO messages(id,session_id,role,content,agent_id) VALUES(?1,?2,'assistant',?3,'a')",
        rusqlite::params!["learn-assistant", session_id, "알겠습니다"],
    ).unwrap();
    drop(conn);
    let provider = app.get("/api/providers")[0]["id"].as_str().unwrap().to_string();
    let response = app.client
        .post(format!("{}/api/sessions/{session_id}/learn", app.base))
        .json(&json!({"providerId":provider,"model":"mock"}))
        .send()
        .unwrap();
    let status = response.status();
    let queued: Value = serde_json::from_str(&response.text().unwrap()).unwrap_or(Value::Null);
    assert!(status.is_success(), "learn failed: {status} {queued}");
    let job = &queued["job"];
    assert_eq!(job["kind"], "learn", "{queued}");
    assert_eq!(job["input"]["semantic"], true, "{queued}");
    assert_eq!(job["input"]["automatic"], true, "{queued}");
}

#[test]
fn a_model_generated_intervention_runs_and_refutes_its_hypothesis() {
    let _serial = serial();
    let stream_calls = Arc::new(Mutex::new(0u32));
    let adaptations = Arc::new(Mutex::new(0u32));
    let stream_counter = stream_calls.clone();
    let adaptation_counter = adaptations.clone();
    let (port, _) = scripted_provider(move |request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        if request["stream"].as_bool() == Some(false) && system.contains("adaptation planner") {
            let mut count = adaptation_counter.lock().unwrap();
            *count += 1;
            let (hypothesis, prediction, intervention) = if *count == 1 {
                ("h1", json!({"type":"equals","from":"result","path":"/written","value":false}), "i1")
            } else {
                ("h2", json!({"type":"equals","from":"result","path":"/written","value":true}), "i2")
            };
            let content = json!({
                "hypotheses":[{
                    "id":hypothesis,"claim":"the command environment is wrong",
                    "predictions":[{"predicate":prediction}]
                }],
                "interventions":[{
                    "id":intervention,"hypothesisIds":[hypothesis],
                    "tool":"workspace_write",
                    "inputs":{"path":format!("adapt{}.txt", *count),"content":"OK"},
                    "expectedChecks":["file_exists"],"estimatedCost":0.1
                }]
            }).to_string();
            return vec![json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]})];
        }
        let mut count = stream_counter.lock().unwrap();
        *count += 1;
        match *count {
            1 | 2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("fail-{count}"),"type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"적응 결과로 완료했습니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"adapt1.txt와 adapt2.txt 파일을 만들어줘","autoLearning":false}));
    assert_eq!(
        std::fs::read_to_string(app._root.join("adapt1.txt")).unwrap_or_default(),
        "OK"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("adapt2.txt")).unwrap_or_default(),
        "OK"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let adaptations = execution["adaptations"].as_array().cloned().unwrap_or_default();
    let model_adaptation = adaptations
        .iter()
        .find(|item| item["selected"].is_object())
        .expect("model adaptation record");
    assert!(
        model_adaptation["hypotheses"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(model_adaptation["selected"]["id"].is_string());
    let refuted = model_adaptation["hypothesisResults"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["status"] == "refuted");
    assert!(refuted, "wrong hypothesis was not refuted: {model_adaptation}");
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let shells = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "shell_exec")
        .count();
    assert_eq!(
        shells, 2,
        "a retry after the environment changed should be allowed: {operations}"
    );
    let adapted_writes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "workspace_write" && operation["origin"] == "adaptation")
        .count();
    assert!(adapted_writes >= 1, "{operations}");
}

#[test]
fn consecutive_failed_adaptations_stop_the_no_progress_loop() {
    let _serial = serial();
    let stream_calls = Arc::new(Mutex::new(0u32));
    let adaptation_calls = Arc::new(Mutex::new(0u32));
    let adaptation_counter = adaptation_calls.clone();
    let counter = stream_calls.clone();
    let (port, _) = scripted_provider(move |request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        if request["stream"].as_bool() == Some(false) && system.contains("adaptation planner") {
            let mut adaptation = adaptation_counter.lock().unwrap();
            *adaptation += 1;
            let (hypothesis, intervention, command) = if *adaptation == 1 {
                ("h1", "i1", "exit 8")
            } else {
                ("h2", "i2", "exit 10")
            };
            let content = json!({
                "hypotheses":[{"id":hypothesis,"claim":"still wrong","predictions":[{"predicate":{"type":"equals","from":"result","path":"/exitCode","value":0}}]}],
                "interventions":[{"id":intervention,"hypothesisIds":[hypothesis],"tool":"shell_exec","inputs":{"command":command},"expectedChecks":["x"],"estimatedCost":0.1}]
            }).to_string();
            return vec![json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]})];
        }
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"f1","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"f2","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 9\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"진전이 없어 중단합니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let stream = app.chat(&json!({"input":"계속 시도해","autoLearning":false}));
    assert!(stream.contains("진전이 없어 중단합니다."), "{stream}");
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let adaptations = execution["adaptations"].as_array().unwrap();
    assert!(
        adaptations.iter().filter(|item| item["selected"].is_object()).count() >= 2,
        "{execution}"
    );
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let shells = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "shell_exec")
        .count();
    assert_eq!(shells, 4, "unexpected retry loop: {operations}");
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

#[test]
fn block_write_without_read_algorithm_blocks_and_then_allows_the_write() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let last_tool = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if last_tool.contains("toolSelectionBlocked") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"list-1","type":"function","function":{"name":"workspace_list","arguments":"{\"path\":\"\"}"}}
            ]},"finish_reason":"tool_calls"}]})];
        }
        if last_tool.contains("entries") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"write-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"guarded.txt\",\"content\":\"OK\"}"}}
            ]},"finish_reason":"tool_calls"}]})];
        }
        if messages.iter().any(|message| message["role"] == "tool") {
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"early-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"guarded.txt\",\"content\":\"EARLY\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let algorithm = app.post(
        "/api/algorithms",
        &json!({
            "kind":"tool.selection",
            "name":"block-write-without-read",
            "implementation":"block-write-without-read",
            "parameters":{}
        }),
    );
    let activated = app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    assert_eq!(activated["status"], "active", "{activated}");
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 만들어줘","autoLearning":false}));
    assert!(
        stream.contains("toolSelectionBlocked") || stream.contains("차단"),
        "{stream}"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("guarded.txt")).unwrap_or_default(),
        "OK"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let writes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| {
            operation["tool"] == "workspace_write"
                && operation["status"] == "completed"
        })
        .count();
    assert_eq!(writes, 1, "{operations}");
}

#[test]
fn allow_all_recovery_algorithm_retries_the_same_failed_call() {
    let _serial = serial();
    let calls = Arc::new(Mutex::new(0u32));
    let counter = calls.clone();
    let (port, _) = scripted_provider(move |request| {
        if request["stream"].as_bool() != Some(true) {
            return vec![json!({"choices":[{"delta":{"content":"{}"},"finish_reason":"stop"}]})];
        }
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 | 2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("fail-{count}"),"type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"재시도 후 완료."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let algorithm = app.post(
        "/api/algorithms",
        &json!({
            "kind":"recovery.strategy",
            "name":"allow-all",
            "implementation":"allow-all",
            "parameters":{}
        }),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let _ = app.chat(&json!({"input":"같은 실패를 재시도해","autoLearning":false}));
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let shells = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "shell_exec")
        .count();
    assert_eq!(shells, 2, "allow-all did not retry: {operations}");
}

#[test]
fn parallel_reads_combination_executes_independent_reads_in_one_round() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"두 파일을 읽었습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"read-1","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"one.txt\"}"}},
            {"index":1,"id":"read-2","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"two.txt\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    std::fs::write(app._root.join("one.txt"), "ONE").unwrap();
    std::fs::write(app._root.join("two.txt"), "TWO").unwrap();
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.combination","name":"parallel-reads","implementation":"parallel-reads","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let stream = app.chat(&json!({"input":"one.txt와 two.txt를 읽어","autoLearning":false}));
    assert!(stream.contains("두 파일을 읽었습니다"), "{stream}");
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let reads = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "workspace_read" && operation["status"] == "completed")
        .count();
    assert_eq!(reads, 2, "{operations}");
    let synergy = app.get("/api/synergy");
    let combination = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "tool.combination")
        .expect("combination statistic");
    assert_eq!(combination["uses"], 1, "{combination}");
    assert_eq!(combination["successes"], 1, "{combination}");
}

#[test]
fn parallel_reads_answer_malformed_arguments_so_no_tool_call_is_orphaned() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_error = messages.iter().any(|message| {
            message["role"] == "tool"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("invalid tool arguments"))
        });
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_error {
            return vec![json!({"choices":[{"delta":{"content":"malformed-read-was-answered"},"finish_reason":"stop"}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"missing-error"},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"read-good","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"one.txt\"}"}},
            {"index":1,"id":"read-bad","type":"function","function":{"name":"workspace_read","arguments":"{bad"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    std::fs::write(app._root.join("one.txt"), "ONE").unwrap();
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.combination","name":"parallel-reads","implementation":"parallel-reads","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let stream = app.chat(&json!({"input":"one.txt를 읽어","autoLearning":false}));
    assert!(stream.contains("malformed-read-was-answered"), "{stream}");
}

#[test]
fn parallel_reads_counts_a_failed_tool_as_a_failed_combination() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"읽기 시도 완료"},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"read-good","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"one.txt\"}"}},
            {"index":1,"id":"read-missing","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"missing.txt\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    std::fs::write(app._root.join("one.txt"), "ONE").unwrap();
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.combination","name":"parallel-reads","implementation":"parallel-reads","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let stream = app.chat(&json!({"input":"one.txt와 missing.txt를 읽어","autoLearning":false}));
    assert!(stream.contains("읽기 시도 완료"), "{stream}");
    let synergy = app.get("/api/synergy");
    let combination = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "tool.combination")
        .expect("combination statistic");
    assert_eq!(combination["uses"], 1, "{combination}");
    assert_eq!(combination["failures"], 1, "{combination}");
    assert_eq!(combination["successes"], 0, "{combination}");
}

#[test]
fn normalize_paths_arguments_run_through_the_common_dispatch() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"생성했습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"write-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\" ./sub//normalized.txt \",\"content\":\"OK\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.arguments","name":"normalize-paths","implementation":"normalize-paths","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let _ = app.chat(&json!({"input":"normalized.txt를 만들어","autoLearning":false}));
    assert_eq!(
        std::fs::read_to_string(app._root.join("sub/normalized.txt")).unwrap_or_default(),
        "OK"
    );
    let synergy = app.get("/api/synergy");
    let arguments = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "tool.arguments")
        .expect("argument statistic");
    assert_eq!(arguments["uses"], 1, "{arguments}");
    assert_eq!(arguments["successes"], 1, "{arguments}");
}

#[test]
fn room_conflict_resolution_prefers_evidence_backed_claim() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        let reply = if system.contains("활성 에이전트: Alice (id: a)") {
            "workspace_read 결과를 확인하고 작업을 완료했습니다"
        } else {
            "작업이 실패했습니다"
        };
        vec![json!({"choices":[{"delta":{"content":reply},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    for (id, name) in [("a", "Alice"), ("b", "Bob")] {
        app.post(
            "/api/agents",
            &json!({"id":id,"name":name,"systemPrompt":"독립 참가자","enabled":true}),
        );
    }
    let room = app.post(
        "/api/rooms",
        &json!({"name":"충돌 대화","agentIds":["a","b"],"routingMode":"discussion","rounds":1,"maxTurns":2,"toolsEnabled":false,"autoLearning":false}),
    );
    let id = room["id"].as_str().unwrap();
    let sent = app.post(
        &format!("/api/rooms/{id}/messages"),
        &json!({"input":"테스트 실행 결과를 알려줘","clientMessageId":"owner-conflict-001"}),
    );
    let job_id = sent["jobId"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        let status = job["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed") {
            break;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(25));
    }
    let after = app.get(&format!("/api/rooms/{id}"));
    let resolution = &after["conflictResolution"];
    assert_eq!(resolution["conflict"], true, "{after}");
    assert_eq!(resolution["outcome"], "success", "{after}");
    assert_eq!(resolution["winner"]["agentId"], "a", "{after}");
    let synergy = app.get("/api/synergy");
    let stat = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "multiagent.conflict")
        .expect("conflict stat");
    assert_eq!(stat["uses"], 1, "{stat}");
    assert_eq!(stat["successes"], 1, "{stat}");
}

#[test]
fn clause_split_decomposition_is_persisted_with_independent_quality() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_request| {
        vec![json!({"choices":[{"delta":{"content":"세 단계로 처리하겠습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"task.decomposition","name":"clause-split","implementation":"clause-split","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let _ = app.chat(&json!({
        "input":"설정 파일을 읽고 그리고 테스트를 실행한 뒤 결과를 보고해",
        "autoLearning":false
    }));
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| {
            execution["decomposition"]["kernel"].as_str() == Some("clause-split")
        })
        .expect("decomposition execution");
    let subtasks = execution["subtasks"].as_array().unwrap();
    assert_eq!(subtasks.len(), 3, "{execution}");
    assert_eq!(execution["decomposition"]["quality"]["orderPreserved"], true);
    assert!(
        execution["decomposition"]["qualityScore"].as_f64().unwrap() >= 0.75,
        "{execution}"
    );
}

#[test]
fn chat_fails_over_from_native_api_to_compat_provider_in_one_turn() {
    let _serial = serial();
    let (fallback_port, fallback_requests) = scripted_provider(|request| {
        let has_native_tools = request.get("tools").is_some();
        assert!(!has_native_tools, "compat provider received native tools: {request}");
        vec![json!({"choices":[{"delta":{"content":"대체 제공자 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(fallback_port, "compat");
    for provider in app.get("/api/providers").as_array().cloned().unwrap_or_default() {
        if let Some(id) = provider["id"].as_str() {
            app.delete(&format!("/api/providers/{id}"));
        }
    }
    let primary = app.post(
        "/api/providers",
        &json!({"name":"cloud-native","baseUrl":"http://127.0.0.1:1/v1","model":"cloud","enabled":true,"toolMode":"native"}),
    );
    let fallback = app.post(
        "/api/providers",
        &json!({"name":"local-compat","baseUrl":format!("http://127.0.0.1:{fallback_port}/v1"),"model":"local","enabled":true,"toolMode":"compat"}),
    );
    let agent = app.post("/api/agents", &json!({
        "name":"failover agent","systemPrompt":"FAILOVER_IDENTITY",
        "providerId":primary["id"]
    }));
    let stream = app.chat(&json!({
        "input":"파일 도구로 상태를 확인해",
        "agentId":agent["id"],
        "providerId":primary["id"],
        "toolsEnabled":true,
        "autoLearning":false
    }));
    assert!(stream.contains("대체 제공자 응답"), "{stream}");
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| {
            execution["effectiveProviderId"].as_str() == fallback["id"].as_str()
        })
        .expect("failover execution");
    assert!(execution["failoverAt"].is_string(), "{execution}");
    let saved = app.get("/api/agents")
        .as_array()
        .unwrap()
        .iter()
        .find(|saved| saved["id"] == agent["id"])
        .cloned()
        .expect("agent survives failover");
    assert_eq!(saved["providerId"], fallback["id"], "agent must store the provider that answered: {saved}");
    assert_eq!(saved["model"], "local", "agent must store the fallback model: {saved}");
    let captured = fallback_requests.lock().unwrap();
    assert!(!captured.is_empty(), "fallback provider was not called");
    assert!(captured.iter().all(|body| body.get("tools").is_none()));
    assert!(
        captured[0]["messages"][0]["content"]
            .as_str()
            .unwrap_or("")
            .contains("Available tools:"),
        "{}",
        captured[0]
    );
}

