//! preflight tools E2E contracts backed by the shared real-process fixture.
use super::*;

#[test]
fn file_preflight_blocks_blind_and_stale_writes_then_accepts_a_fresh_read() {
    let _serial = serial();
    let root = temp_root();
    let target = root.join("guarded.txt");
    std::fs::write(&target, "original").unwrap();
    let step = std::sync::atomic::AtomicUsize::new(0);
    let (port, _) = scripted_provider(move |_| {
        let index = step.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if index == 2 {
            std::fs::write(&target, "changed outside the run").unwrap();
        }
        let (name, args) = match index {
            0 | 2 | 4 => ("workspace_write", json!({"path":"guarded.txt","content":"final"})),
            1 | 3 => ("workspace_read", json!({"path":"guarded.txt"})),
            _ => return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})],
        };
        vec![json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"id":format!("preflight-{index}"),"type":"function",
            "function":{"name":name,"arguments":args.to_string()}
        }]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"preflight-regression",
        "parameters":{"kernel":"same-file-revision-v1"}
    }));
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 수정해.","autoLearning":false}));
    assert_eq!(std::fs::read_to_string(app._root.join("guarded.txt")).unwrap(), "final", "{stream}");
    let execution = &app.get("/api/executions")[0];
    assert_eq!(execution["taskOutcome"], "answered_unverified", "{execution}");
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let writes = operations.as_array().unwrap().iter().filter(|operation| operation["tool"] == "workspace_write").collect::<Vec<_>>();
    assert_eq!(writes.len(), 3, "{operations}");
    assert_eq!(writes.iter().filter(|operation| operation["result"]["preflightBlocked"] == true).count(), 2);
    assert_eq!(writes.iter().filter(|operation| operation["status"] == "completed"
        && operation["result"]["observedSha256"] == format!("{:x}", Sha256::digest(b"changed outside the run"))).count(), 1);
}


#[test]
fn preflight_v2_reuses_a_rejected_write_observation_without_a_helper_call() {
    let _serial = serial();
    let root = temp_root();
    std::fs::write(root.join("guarded.txt"), "mode=old\nowner=keep\n").unwrap();
    let (port, requests) = scripted_provider(|request| {
        let last_tool = request["messages"].as_array().unwrap().iter().rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str());
        let (id, path, content) = match last_tool {
            None => ("escape", "/guarded.txt", "blind"),
            Some(result) if result.contains("invalidWorkspacePath") => ("blind", "guarded.txt", "blind"),
            Some(result) if result.contains("currentContent") && result.contains("owner=keep") => {
                ("retry", "guarded.txt", "mode=new\nowner=keep\n")
            }
            _ => return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})],
        };
        vec![json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"id":id,"type":"function","function":{"name":"workspace_write",
                "arguments":json!({"path":path,"content":content}).to_string()}
        }]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"preflight-inline-read",
        "parameters":{"kernel":"same-file-revision-v2"}
    }));
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 수정해.","autoLearning":false}));
    assert_eq!(std::fs::read_to_string(app._root.join("guarded.txt")).unwrap(), "mode=new\nowner=keep\n", "{stream}");
    let execution = &app.get("/api/executions")[0];
    assert_eq!(execution["taskOutcome"], "answered_unverified", "{execution}");
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let operations = operations.as_array().unwrap();
    assert_eq!(operations.len(), 3, "{operations:?}");
    assert!(operations.iter().any(|op| op["result"]["preflightBlocked"] == true
        && op["fileObservation"]["sha256"].is_string()));
    assert!(requests.lock().unwrap().iter().all(|request| request["stream"] == true));
}


#[test]
fn preflight_inline_observation_allows_retry_without_a_separate_read() {
    let _serial = serial();
    let root = temp_root();
    let target = root.join("inline.txt");
    std::fs::write(&target, "original").unwrap();
    let step = std::sync::atomic::AtomicUsize::new(0);
    let (port, _) = scripted_provider(move |_| {
        let index = step.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if index < 2 {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{
                "index":0,"id":format!("inline-{index}"),"type":"function",
                "function":{"name":"workspace_write","arguments":"{\"path\":\"inline.txt\",\"content\":\"final\"}"}
            }]},"finish_reason":"tool_calls"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"inline-preflight",
        "parameters":{"kernel":"same-file-revision-v2"}
    }));
    let stream = app.chat(&json!({"input":"inline.txt 파일을 수정해.","autoLearning":false}));
    assert_eq!(std::fs::read_to_string(app._root.join("inline.txt")).unwrap(), "final", "{stream}");
    let execution = &app.get("/api/executions")[0];
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let writes = operations.as_array().unwrap().iter()
        .filter(|operation| operation["tool"] == "workspace_write")
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 2, "{operations}");
    let blocked = writes.iter()
        .find(|operation| operation["result"]["preflightBlocked"] == true)
        .expect("blocked write");
    assert_eq!(blocked["result"]["currentContent"], "original", "{operations}");
    let completed = writes.iter()
        .find(|operation| operation["status"] == "completed" && operation["result"]["written"] == true)
        .expect("completed write");
    assert_eq!(
        completed["result"]["observedSha256"],
        format!("{:x}", Sha256::digest(b"original")),
        "{operations}"
    );
}


#[test]
fn preflight_guard_stays_selected_after_provider_failover() {
    let _serial = serial();
    let root = temp_root();
    std::fs::write(root.join("guarded.txt"), "old").unwrap();
    let (working_port, _) = scripted_provider(|request| {
        let result = request["messages"].as_array().unwrap().iter().rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str());
        let (id, content) = match result {
            None => ("blind", "discard"),
            Some(result) if result.contains("currentContent") => ("retry", "final"),
            _ => return vec![json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]})],
        };
        vec![json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"id":id,"type":"function","function":{"name":"workspace_write","arguments":
                json!({"path":"guarded.txt","content":content}).to_string()}
        }]},"finish_reason":"tool_calls"}]})]
    });
    let dead_port = free_port();
    let app = TestApp::from_root(dead_port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    let primary = app.get("/api/providers")[0]["id"].as_str().unwrap().to_owned();
    let primary_base = format!("http://127.0.0.1:{dead_port}/v1");
    let fallback_base = format!("http://127.0.0.1:{working_port}/v1");
    for (id, base, model) in [(&primary[..], &primary_base, "primary"), ("fallback", &fallback_base, "fallback")] {
        app.post("/api/providers", &json!({"id":id,"name":model,"baseUrl":base,
            "model":model,"local":true,"enabled":true,"toolMode":"native"}));
    }
    let guard = app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"shared-file-guard","parameters":{"kernel":"same-file-revision-v2"},
    }));
    let revision = &guard["active"]["revision"];
    assert!(revision.is_string(), "{guard}");
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 수정해.","providerId":primary,"autoLearning":false}));
    assert!(stream.contains("provider_failover"), "{stream}");
    assert_eq!(std::fs::read_to_string(app._root.join("guarded.txt")).unwrap(), "final", "{stream}");
    let run = &app.get("/api/executions")[0];
    assert_eq!(run["effectiveModel"], "fallback", "{run}");
    assert_eq!(&run["preflightPolicy"]["revision"], revision, "{run}");
    let operations = app.get(&format!("/api/executions/{}/operations", run["id"].as_str().unwrap()));
    assert!(operations.as_array().unwrap().iter().any(|op| op["result"]["preflightBlocked"] == true
        && &op["result"]["preflightRevision"] == revision));
}


#[test]
fn per_bot_growth_changes_only_that_bots_next_prompt() {
    let _serial = serial();
    let calls = Arc::new(Mutex::new(0u32));
    let counter = calls.clone();
    let (port, requests) = scripted_provider(move |_| {
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 | 2 | 4 | 5 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("fail-{count}"),"type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let agent_a = app.post("/api/agents", &json!({"name":"growth-a","systemPrompt":"A"}));
    let agent_b = app.post("/api/agents", &json!({"name":"growth-b","systemPrompt":"B"}));
    let a_id = agent_a["id"].as_str().unwrap();
    let b_id = agent_b["id"].as_str().unwrap();
    let session_a = app.post("/api/sessions", &json!({"title":"a","agentId":a_id}));
    let a_session = session_a["id"].as_str().unwrap();
    for index in 1..=2 {
        let _ = app.chat(&json!({
            "sessionId": a_session,
            "agentId": a_id,
            "input": format!("A 실패 작업 {index}"),
            "autoLearning": false
        }));
    }
    let growth = app.get(&format!("/api/agents/{a_id}/growth"));
    assert!(growth["runs"].as_u64().unwrap_or(0) >= 2, "{growth}");
    assert!(!growth["guidance"].as_array().unwrap_or(&Vec::new()).is_empty(), "{growth}");
    let _ = app.chat(&json!({
        "sessionId": a_session,
        "agentId": a_id,
        "input": "A 세 번째 작업",
        "autoLearning": false
    }));
    let session_b = app.post("/api/sessions", &json!({"title":"b","agentId":b_id}));
    let b_session = session_b["id"].as_str().unwrap();
    let _ = app.chat(&json!({
        "sessionId": b_session,
        "agentId": b_id,
        "input": "B 첫 작업",
        "autoLearning": false
    }));
    let requests = requests.lock().unwrap();
    let prompt_for = |needle: &str| -> String {
        requests
            .iter()
            .find_map(|request| {
                let messages = request["messages"].as_array()?;
                let has_user = messages.iter().any(|message| {
                    message["role"] == "user"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.contains(needle))
                });
                if !has_user {
                    return None;
                }
                Some(
                    messages
                        .iter()
                        .filter_map(|message| message["content"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })
            .unwrap_or_default()
    };
    let a_prompt = prompt_for("A 세 번째 작업");
    assert!(a_prompt.contains("Agent growth"), "A growth missing: {a_prompt}");
    assert!(
        a_prompt.contains("tool_failed") || a_prompt.contains("shell_exec"),
        "A failure guidance missing: {a_prompt}"
    );
    let b_prompt = prompt_for("B 첫 작업");
    assert!(
        !b_prompt.contains("Agent growth"),
        "B inherited A's growth: {b_prompt}"
    );
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



/// Real HTTP/SSE failover through the Rust gateway: do not send a model's
/// forbidden tool protocol to a fallback endpoint just because it is online.
#[test]
fn failover_skips_incompatible_local_model_and_selects_declared_text_tools(){
    let _serial=serial();
    let (fallback_port,requests)=scripted_provider(|body|{
        assert_eq!(body["model"],"capability-compatible");
        assert!(body.get("tools").is_none(),"compat model received native tool schema");
        vec![json!({"choices":[{"delta":{"content":"호환 모델 답변"},"finish_reason":"stop"}]})]
    });
    let app=TestApp::new(fallback_port,"compat");
    for provider in app.get("/api/providers").as_array().cloned().unwrap_or_default(){
        if let Some(id)=provider["id"].as_str(){
            app.delete(&format!("/api/providers/{id}"));
        }
    }
    let primary=app.post("/api/providers",&json!({
        "name":"primary-unreachable","baseUrl":"http://127.0.0.1:1/v1",
        "model":"primary","enabled":true,"toolMode":"native"
    }));
    let endpoint=format!("http://127.0.0.1:{fallback_port}/v1");
    let incompatible=app.post("/api/providers",&json!({
        "name":"local-no-tools","baseUrl":endpoint,"model":"capability-denied",
        "enabled":true,"toolMode":"compat",
        "capabilities":{"text":true,"nativeToolCalls":false,"textToolCalls":false}
    }));
    assert!(incompatible["id"].is_string());
    let compatible=app.post("/api/providers",&json!({
        "name":"local-text-tools","baseUrl":endpoint,"model":"capability-compatible",
        "enabled":true,"toolMode":"compat",
        "capabilities":{"text":true,"nativeToolCalls":false,"textToolCalls":true}
    }));
    let agent=app.post("/api/agents",&json!({
        "name":"model-fallback-probe","systemPrompt":"KEEP_AGENT_IDENTITY",
        "providerId":primary["id"]
    }));
    let stream=app.chat(&json!({
        "input":"workspace_list 도구로 작업 환경을 확인해",
        "agentId":agent["id"],"providerId":primary["id"],
        "toolsEnabled":true,"autoLearning":false
    }));
    assert!(stream.contains("호환 모델 답변"),"{stream}");
    let captured=requests.lock().unwrap();
    assert!(!captured.is_empty());
    assert!(captured.iter().all(|body|
        body["model"]=="capability-compatible"&&body.get("tools").is_none()));
    let executions=app.get("/api/executions");
    assert!(executions.as_array().unwrap().iter().any(|run|
        run["effectiveProviderId"]==compatible["id"]));
}
