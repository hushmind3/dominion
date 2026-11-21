//! adaptation program E2E contracts backed by the shared real-process fixture.
use super::*;

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


#[test]
fn a_frozen_replacement_passes_the_opt_in_guard() {
    let _serial = serial();
    let root = temp_root();
    std::fs::create_dir_all(root.join("bench")).unwrap();
    std::fs::write(root.join("bench/guarded.txt"), "start ALPHA end\n").unwrap();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"MODEL SHOULD NOT RUN"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["file_write"]}));
    app.post(
        "/api/benchmark/policies/state",
        &json!({
            "slot":"mutation.preflight",
            "name":"deterministic-guard",
            "parameters":{"kernel":"same-file-revision-v1"}
        }),
    );
    let stream = app.chat(&json!({
        "input":"edit `bench/guarded.txt` and replace `ALPHA` with `BETA` and preserve everything else.",
        "autoLearning":false
    }));
    assert_eq!(
        std::fs::read_to_string(app._root.join("bench/guarded.txt")).unwrap(),
        "start BETA end\n",
        "{stream}"
    );
    assert!(requests.lock().unwrap().is_empty(), "the model was called");
    let execution = &app.get("/api/executions")[0];
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
    assert!(
        items.iter().any(|operation| operation["tool"] == "file_read"
            && operation["fileObservation"]["sha256"].is_string()),
        "the guard could not see the deterministic read: {operations}"
    );
    let write = items
        .iter()
        .find(|operation| operation["tool"] == "file_write")
        .unwrap();
    assert_eq!(write["effectState"], "verified", "{write}");
    assert_eq!(
        write["result"]["preflightBlocked"],
        Value::Null,
        "{write}"
    );
}


#[test]
fn a_typed_execution_program_takes_the_failure_branch() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"프로그램 결과로 완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"e2e-program-1",
        "nodes":[
            {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
            {"type":"branch","id":"check",
             "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
             "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
             "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"e2e-program","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");

    let stream = app.chat(&json!({"input":"fallback.txt 파일을 만들어줘","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("fallback.txt")).unwrap_or_default(),
        "FALLBACK"
    );
    assert!(!app._root.join("primary.txt").exists());
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let checkpoint = &execution["programCheckpoint"];
    assert_eq!(checkpoint["status"], "completed", "{execution}");
    assert_eq!(checkpoint["programRevision"], "e2e-program-1");
    let completed = checkpoint["completedOperationIds"].as_array().unwrap();
    assert!(completed.iter().any(|item| item == "try"));
    assert!(completed.iter().any(|item| item == "fallback"));
    assert!(
        checkpoint["pendingOperationIds"].as_array().unwrap().is_empty(),
        "{checkpoint}"
    );
}

/// Builds a verifiable evidence bundle: 10 pairs, baseline fails, treatment succeeds.

#[test]
fn regressed_experiment_rolls_back_the_candidate() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"planning","name":"regression-candidate",
            "parameters":{"outline":true},"rationale":"regression rollback test"
        }),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy_id, "fixture":"planning_evaluation"}),
    );
    let request_id = request["id"].as_str().unwrap().to_string();
    let claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"regression-runner","ttlSeconds":60}),
    );
    let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
    let evidence = evidence_bundle(10, true, false);
    seed_evidence_executions(&app, &evidence);
    assert_eq!(evidence["verdict"], "regressed", "{evidence}");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"regressed-1",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],
            "independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],
            "manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
        }),
    );
    assert_eq!(completed["request"]["status"], "completed", "{completed}");
    assert_eq!(completed["request"]["verdict"], "regressed", "{completed}");
    assert_eq!(
        completed["request"]["promotionStatus"], "rolled_back",
        "{completed}"
    );
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|policy| policy["id"] == policy_id.as_str())
        .expect("policy");
    assert_eq!(policy["status"], "rolled_back", "{policy}");
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
fn completion_rejects_fake_or_incomplete_evidence() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let make = |name: &str| -> (String, String) {
        let policy = app.post(
            "/api/benchmark/policies",
            &json!({"slot":"planning","name":name,"parameters":{"outline":true},"rationale":"evidence"}),
        );
        let policy_id = policy["id"].as_str().unwrap().to_string();
        let request = app.post(
            "/api/benchmark/experiment-requests",
            &json!({"policyId":policy_id,"fixture":"planning_evaluation"}),
        );
        let request_id = request["id"].as_str().unwrap().to_string();
        let claimed = app.post(
            "/api/experiment-requests/claim",
            &json!({"runnerId":name,"ttlSeconds":60}),
        );
        let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
        (policy_id, format!("{request_id}|{token}"))
    };
    let good_plan = "1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다";

    // 1) Claimed success numbers that do not match the raw failures.
    let (policy_id, target) = make("fake-numbers");
    let (request_id, token) = target.split_once('|').unwrap();
    let fake = evidence_bundle(10, false, false);
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken":token,"experimentId":"fake-exp",
            "verdict":"preliminary_support","delta":1.0,"independentUnits":10,
            "manifest":fake["manifest"],"manifestHash":fake["manifestHash"],"trials":fake["trials"],
            "semanticDelta":0.5,"independentSemanticDelta":0.5,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[{"pair":0,"arm":"baseline","plan":""},{"pair":0,"arm":"treatment","plan":good_plan}]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("evidence rejected")),
        "{completed}"
    );
    let policies = app.get("/api/policies");
    assert_eq!(
        policies
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == policy_id.as_str())
            .unwrap()["status"],
        "candidate"
    );

    // 2) Missing arm for a planned pair.
    let (_policy_id, target) = make("incomplete-trials");
    let (request_id, token) = target.split_once('|').unwrap();
    let mut incomplete = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &incomplete);
    let trials = incomplete["trials"].as_array_mut().unwrap();
    let index = trials.iter().position(|t| t["pair"] == 9 && t["arm"] == "treatment").unwrap();
    trials.remove(index);
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken":token,"experimentId":"incomplete-exp",
            "verdict":incomplete["verdict"],"delta":incomplete["delta"],
            "independentUnits":incomplete["independentUnits"],"manifest":incomplete["manifest"],
            "manifestHash":incomplete["manifestHash"],"trials":incomplete["trials"],
            "semanticDelta":0.5,"independentSemanticDelta":0.5,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[{"pair":0,"arm":"baseline","plan":""},{"pair":0,"arm":"treatment","plan":good_plan}]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("missing an arm") || text.contains("incomplete")),
        "{completed}"
    );
}


#[test]
fn a_crashed_settlement_recovers_without_reapplying_promotion_and_stale_claims_fail() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({"slot":"planning","name":"crash-settle","parameters":{"outline":true},"rationale":"crash"}),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy_id,"fixture":"planning_evaluation"}),
    );
    let request_id = request["id"].as_str().unwrap().to_string();
    let _claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"crash-runner","ttlSeconds":60}),
    );
    let root = app._root.clone();
    {
        let conn = Connection::open(root.join("runtime/dominion.sqlite")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let data: String = conn
            .query_row(
                "SELECT data FROM records WHERE kind='experiment_request' AND id=?1",
                [request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let mut value: Value = serde_json::from_str(&data).unwrap();
        value["status"] = json!("settling");
        value["settleToken"] = json!("stale-settle");
        value["settleExpiresAtMs"] = json!(0);
        value["promotionIntent"] = json!({
            "kind":"policy","targetId":policy_id,"experimentId":"recovered-exp"
        });
        conn.execute(
            "UPDATE records SET data=?2 WHERE kind='experiment_request' AND id=?1",
            rusqlite::params![request_id, value.to_string()],
        )
        .unwrap();
        let policy_data: String = conn
            .query_row(
                "SELECT data FROM records WHERE kind='adaptation_policy' AND id=?1",
                [policy_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let mut policy_value: Value = serde_json::from_str(&policy_data).unwrap();
        policy_value["status"] = json!("active");
        policy_value["revision"] = json!("recovered-revision");
        policy_value["lastExperiment"] = json!({
            "experimentId":"recovered-exp","independentUnits":10,
            "semanticDelta":0.3,"independentSemanticDelta":0.3,
            "verdict":"preliminary_support"
        });
        conn.execute(
            "UPDATE records SET data=?2 WHERE kind='adaptation_policy' AND id=?1",
            rusqlite::params![policy_id, policy_value.to_string()],
        )
        .unwrap();
    }
    // Trigger settling recovery through the claim path. When no request is
    // queued the endpoint returns 204, so raw client is used here.
    let _ = app
        .client
        .post(format!("{}/api/experiment-requests/claim", app.base))
        .json(&json!({"runnerId":"recovery-check","ttlSeconds":60}))
        .send();

    let requests = app.get("/api/experiment-requests");
    let recovered = &requests.as_array().unwrap()[0];
    assert_eq!(recovered["status"], "completed", "{recovered}");
    assert!(
        recovered["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("recovered after restart")),
        "{recovered}"
    );
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["revision"], "recovered-revision", "promotion was reapplied");

    // Stale claim: expiry rejects completion.
    let policy2 = app.post(
        "/api/benchmark/policies",
        &json!({"slot":"planning","name":"stale-claim","parameters":{"outline":true},"rationale":"stale"}),
    );
    let policy2_id = policy2["id"].as_str().unwrap().to_string();
    let request2 = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy2_id,"fixture":"planning_evaluation"}),
    );
    let request2_id = request2["id"].as_str().unwrap().to_string();
    let claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"stale-runner","ttlSeconds":60}),
    );
    let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
    {
        let conn = Connection::open(root.join("runtime/dominion.sqlite")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        conn.execute(
            "UPDATE records SET data=json_set(data,'$.claimExpiresAtMs',0) WHERE kind='experiment_request' AND id=?1",
            [request2_id.as_str()],
        )
        .unwrap();
    }
    let evidence = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &evidence);
    let response = app
        .client
        .post(format!("{}/api/experiment-requests/{request2_id}/complete", app.base))
        .json(&json!({
            "claimToken":token,"experimentId":"stale-exp",
            "verdict":evidence["verdict"],"delta":evidence["delta"],
            "independentUnits":evidence["independentUnits"],"manifest":evidence["manifest"],
            "manifestHash":evidence["manifestHash"],"trials":evidence["trials"],
            "semanticDelta":0.5,"independentSemanticDelta":0.5,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"task","plans":[{"pair":0,"arm":"baseline","plan":""},{"pair":0,"arm":"treatment","plan":"1. 검증"}]
        }))
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);

    // Restart preserves the recovered settlement and does not re-promote.
    let root = app._root.clone();
    drop(app);
    let app = TestApp::from_root(port, "native", root, &[]);
    let requests = app.get("/api/experiment-requests");
    let recovered = requests
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == request_id.as_str())
        .expect("recovered request");
    assert_eq!(recovered["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["revision"], "recovered-revision");
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
    let _ = app.chat(&json!({"input":"adapt1.txt와 adapt2.txt 파일을 확인해줘","autoLearning":false}));
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
fn a_program_consumes_sequence_verify_loop_and_model_decision_nodes() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"프로그램 노드를 모두 소비했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"multi-node-program",
        "nodes":[
            {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
            {"type":"branch","id":"check",
             "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
             "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
             "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]},
            {"type":"sequence","id":"seq","nodes":[
                {"type":"call_tool","id":"seq1","tool":"workspace_write","inputs":{"path":"seq.txt","content":"SEQ"}}
            ]},
            {"type":"verify","id":"verify","checks":[
                {"predicate":{"type":"exists","from":"seq1","path":"/path"}}
            ]},
            {"type":"loop","id":"loop","maxIterations":2,
             "predicate":{"type":"not","predicate":{"type":"exists","from":"loop_done"}},
             "body":[{"type":"call_tool","id":"loop1","tool":"workspace_write","inputs":{"path":"loop.txt","content":"LOOP"},"saveAs":"loop_done"}]},
            {"type":"model_decision","id":"model"}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"multi-node","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let stream = app.chat(&json!({"input":"fallback.txt 파일을 만들어줘","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    for (path, expected) in [
        ("fallback.txt", "FALLBACK"),
        ("seq.txt", "SEQ"),
        ("loop.txt", "LOOP"),
    ] {
        assert_eq!(
            std::fs::read_to_string(app._root.join(path)).unwrap_or_default(),
            expected,
            "{path} was not written"
        );
    }
    assert!(!app._root.join("primary.txt").exists());
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let checkpoint = &execution["programCheckpoint"];
    assert_eq!(checkpoint["status"], "paused", "{checkpoint}");
    assert_eq!(checkpoint["loopIterations"], 1, "{checkpoint}");
    assert_eq!(checkpoint["parallelMode"], Value::Null);
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let tools = operations
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|operation| operation["tool"].as_str())
        .collect::<Vec<_>>();
    assert!(tools.contains(&"shell_exec"));
    assert!(tools.contains(&"workspace_write"));
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


#[test]
fn a_mid_adaptation_restart_restores_hypotheses_and_does_not_repeat_completed_steps() {
    let _serial = serial();
    let stream_calls = Arc::new(Mutex::new(0u32));
    let stream_counter = stream_calls.clone();
    let (port, _) = scripted_provider(move |request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        if request["stream"].as_bool() == Some(false) && system.contains("adaptation planner") {
            return vec![json!({"choices":[{"delta":{"content":json!({
                "hypotheses":[{
                    "id":"h1",
                    "claim":"the shell environment cannot run this command",
                    "predictions":[{"predicate":{"type":"equals","from":"result","path":"/exitCode","value":0}}]
                }],
                "interventions":[{
                    "id":"i1","hypothesisIds":["h1"],"tool":"workspace_write",
                    "inputs":{"path":"adapt.txt","content":"ADAPT"},
                    "expectedChecks":["file_exists"],"estimatedCost":0.1
                }]
            }).to_string()},"finish_reason":"stop"}]})];
        }
        let mut count = stream_counter.lock().unwrap();
        *count += 1;
        match *count {
            1 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fail-1","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            2 => {
                // Hold the model call open so the test can kill the process
                // after the adaptation tool has already run.
                std::thread::sleep(Duration::from_millis(3_000));
                vec![json!({"choices":[{"delta":{"content":"첫 실행 모델 응답"},"finish_reason":"stop"}]})]
            }
            _ => vec![json!({"choices":[{"delta":{"content":"재개 실행 완료."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"adapt-restart-program",
        "nodes":[
            {"type":"call_tool","id":"step1","tool":"workspace_write","inputs":{"path":"step1.txt","content":"ONE"},"saveAs":"step1"},
            {"type":"model_decision","id":"pause"},
            {"type":"call_tool","id":"step2","tool":"workspace_write","inputs":{"path":"step2.txt","content":"TWO"},"saveAs":"step2"},
            {"type":"model_decision","id":"pause2"}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"adapt-restart","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"step1.txt와 step2.txt 파일을 확인해줘",
        "autoLearning":false
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("adapt.txt").exists() {
        assert!(Instant::now() < deadline, "adaptation did not run");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Wait until the adaptation record (hypotheses + selected intervention) is
    // durable before killing the process.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let executions = app.get("/api/executions");
        let recorded = executions.as_array().unwrap().iter().any(|execution| {
            execution["adaptations"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| {
                    item["selected"]["id"] == "i1"
                        && item["hypotheses"].as_array().is_some_and(|h| !h.is_empty())
                }))
        });
        if recorded {
            break;
        }
        assert!(Instant::now() < deadline, "adaptation record was not persisted");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(150));
    drop(app);
    let _ = handle.join();

    let app = TestApp::from_root(port, "native", root.clone(), &[("DOMINION_BENCHMARK", "1")]);
    let deadline = Instant::now() + Duration::from_secs(25);
    let resumed = loop {
        let executions = app.get("/api/executions");
        if let Some(resumed) = executions.as_array().unwrap().iter().find(|execution| {
            execution["resumedFromExecutionId"].is_string()
                && execution["programCheckpoint"]["status"] == "completed"
        }) {
            break resumed.clone();
        }
        assert!(
            Instant::now() < deadline,
            "resumed execution did not restore: {executions}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    // The saved hypothesis and chosen intervention are restored on the new
    // execution, not just left behind on the interrupted one.
    let adaptation = resumed["adaptations"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["selected"]["id"] == "i1"))
        .expect("restored adaptation record");
    assert_eq!(adaptation["hypotheses"][0]["id"], "h1", "{adaptation}");
    assert_eq!(
        adaptation["selected"]["tool"], "workspace_write",
        "{adaptation}"
    );
    assert_eq!(
        resumed["adaptationCheckpoint"]["selectedIntervention"],
        "i1",
        "{resumed}"
    );
    assert!(
        resumed["adaptationCheckpoint"]["failureSignature"].is_string(),
        "{resumed}"
    );
    // The completed program step is not re-executed and the adaptation stays
    // a single side effect.
    let executions = app.get("/api/executions");
    let operations = executions
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|execution| {
            let id = execution["id"].as_str().unwrap_or("");
            app.get(&format!("/api/executions/{id}/operations"))
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let count = |path: &str| {
        operations
            .iter()
            .filter(|operation| {
                operation["tool"] == "workspace_write"
                    && operation["arguments"]["path"] == path
            })
            .count()
    };
    assert_eq!(count("step1.txt"), 1, "step1 was repeated: {operations:?}");
    assert_eq!(count("adapt.txt"), 1, "adaptation was repeated: {operations:?}");
    assert_eq!(std::fs::read_to_string(root.join("step1.txt")).unwrap_or_default(), "ONE");
    assert_eq!(std::fs::read_to_string(root.join("adapt.txt")).unwrap_or_default(), "ADAPT");
}


#[test]
fn a_paused_program_resumes_after_restart_without_repeating_completed_steps() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(1_800));
        vec![json!({"choices":[{"delta":{"content":"재개 완료."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"restart-program",
        "nodes":[
            {"type":"call_tool","id":"step1","tool":"workspace_write","inputs":{"path":"step1.txt","content":"ONE"},"saveAs":"step1"},
            {"type":"model_decision","id":"pause"},
            {"type":"branch","id":"branch-after-restart",
             "predicate":{"type":"exists","from":"step1","path":"/path"},
             "then":[{"type":"call_tool","id":"step2","tool":"workspace_write","inputs":{"path":"step2.txt","content":"TWO"},"saveAs":"step2"}],
             "else":[{"type":"call_tool","id":"wrong","tool":"workspace_write","inputs":{"path":"wrong.txt","content":"STALE"}}]}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"restart-program","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"step1.txt와 step2.txt 파일을 만들어줘",
        "autoLearning":false
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("step1.txt").exists() {
        assert!(Instant::now() < deadline, "program did not write step1");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
    drop(app);
    let _ = handle.join();

    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let deadline = Instant::now() + Duration::from_secs(25);
    while !root.join("step2.txt").exists() {
        assert!(
            Instant::now() < deadline,
            "resumed program did not write step2: {}",
            app.get("/api/executions")
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let executions = app.get("/api/executions");
        if executions.as_array().unwrap().iter().any(|execution| {
            execution["programCheckpoint"]["status"] == "completed"
                && execution["resumedFromExecutionId"].is_string()
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "resumed execution did not complete: {executions}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("step1.txt")).unwrap_or_default(),
        "ONE"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("step2.txt")).unwrap_or_default(),
        "TWO"
    );
    assert!(!root.join("wrong.txt").exists(), "restart took a branch without durable observations");
    let executions = app.get("/api/executions");
    assert!(
        executions.as_array().unwrap().iter().any(|execution| {
            execution["resumedFromExecutionId"].is_string()
                && execution["programCheckpoint"]["resolvedInputs"]["step1"]["path"].is_string()
        }),
        "completed program observation disappeared across process restart: {executions}"
    );
    let step1_writes = executions
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|execution| {
            let id = execution["id"].as_str().unwrap_or("");
            app.get(&format!("/api/executions/{id}/operations"))
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|operation| {
            operation["tool"] == "workspace_write"
                && operation["arguments"]["path"] == "step1.txt"
        })
        .count();
    assert_eq!(
        step1_writes, 1,
        "completed program step was re-executed after restart"
    );
}


#[test]
fn unsafe_nested_program_mutations_run_sequentially_with_truthful_checkpoint() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"병렬 프로그램 완료."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"parallel-nested-program",
        "nodes":[
            {"type":"call_tool","id":"seed","tool":"workspace_write",
             "inputs":{"path":"seed.txt","content":"SEED"},"saveAs":"seed"},
            {"type":"parallel","id":"par","nodes":[
                {"type":"sequence","id":"s1","nodes":[
                    {"type":"call_tool","id":"p1","tool":"shell_exec",
                     "inputs":{"command":"sleep 2.0"},"saveAs":"p1"},
                    {"type":"branch","id":"b1",
                     "predicate":{"type":"exists","from":"seed","path":"/path"},
                     "then":[{"type":"call_tool","id":"p1b","tool":"workspace_write",
                              "inputs":{"path":"p1b.txt","content":"P1B"}}],
                     "else":[{"type":"call_tool","id":"p1e","tool":"workspace_write",
                              "inputs":{"path":"p1e.txt","content":"P1E"}}]}
                ]},
                {"type":"sequence","id":"s2","nodes":[
                    {"type":"call_tool","id":"p2","tool":"shell_exec",
                     "inputs":{"command":"sleep 2.0"},"saveAs":"p2"},
                    {"type":"loop","id":"l2","maxIterations":1,
                     "predicate":{"type":"not","predicate":{"type":"exists","from":"p2b"}},
                     "body":[{"type":"call_tool","id":"p2b","tool":"workspace_write",
                              "inputs":{"path":"p2b.txt","content":"P2B"},"saveAs":"p2b"}]}
                ]}
            ]},
            {"type":"verify","id":"verify","checks":[
                {"predicate":{"type":"exists","from":"seed","path":"/path"}},
                {"predicate":{"type":"exists","from":"p1b","path":"/path"}},
                {"predicate":{"type":"exists","from":"p2b","path":"/path"}}
            ]}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"parallel-nested","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let stream = app.chat(&json!({"input":"병렬 프로그램을 실행해","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    assert!(app._root.join("seed.txt").exists());
    assert_eq!(
        std::fs::read_to_string(app._root.join("p1b.txt")).unwrap_or_default(),
        "P1B"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("p2b.txt")).unwrap_or_default(),
        "P2B"
    );
    assert!(!app._root.join("p1e.txt").exists());
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let shells = operations.as_array().unwrap().iter()
        .filter(|operation| operation["tool"] == "shell_exec").collect::<Vec<_>>();
    assert_eq!(shells.len(), 2, "{operations}");
    let timestamp = |operation: &Value, key: &str| {
        chrono::DateTime::parse_from_rfc3339(operation[key].as_str().unwrap()).unwrap()
    };
    assert!(
        timestamp(shells[0], "finishedAt") <= timestamp(shells[1], "startedAt")
            || timestamp(shells[1], "finishedAt") <= timestamp(shells[0], "startedAt"),
        "effectful shell commands must not race across unsafe branches: {operations}"
    );
    let checkpoint = &execution["programCheckpoint"];
    assert_eq!(checkpoint["status"], "completed", "{checkpoint}");
    assert_eq!(checkpoint["parallelMode"], "sequential", "{checkpoint}");
    assert_eq!(
        checkpoint["resolvedInputs"]["lastVerify"]["ok"], true,
        "verify node did not run: {checkpoint}"
    );
    assert!(checkpoint["pendingOperationIds"].as_array().unwrap().is_empty());
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

