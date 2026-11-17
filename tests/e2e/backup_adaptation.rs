//! backup adaptation integration scenarios, using the shared real-binary fixture.
use super::*;

#[test]
fn scheduled_backup_uploads_to_a_remote_target() {
    let _serial = serial();
    let (provider_port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"원격 백업"},"finish_reason":"stop"}]})]
    });
    let (remote_port, received) = remote_backup_sink(false);
    let remote_url = format!("http://127.0.0.1:{remote_port}/backups");
    let extra = [
        ("DOMINION_BACKUP_INTERVAL_SECS", "1"),
        ("DOMINION_BACKUP_KEEP", "2"),
        ("DOMINION_BACKUP_REMOTE_URL", remote_url.as_str()),
        ("DOMINION_BACKUP_REMOTE_RETRIES", "1"),
    ];
    let app = TestApp::new_with_env(provider_port, "native", &extra);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"원격 백업 작업","autoLearning":false}));
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut uploaded = false;
    while Instant::now() < deadline {
        if received.lock().unwrap().iter().any(|(path, hash)| {
            path.ends_with(".sqlite.enc") && !hash.is_empty()
        }) {
            uploaded = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(uploaded, "backup was not uploaded to the remote sink");
    let audit = app.get("/api/audit");
    assert!(
        audit.as_array().unwrap().iter().any(|event| {
            event["event"] == "backup.remote_uploaded"
        }),
        "remote upload audit missing: {audit}"
    );
}

#[test]
fn a_failed_remote_upload_keeps_the_local_snapshot() {
    let _serial = serial();
    let (provider_port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"원격 실패"},"finish_reason":"stop"}]})]
    });
    let (remote_port, _) = remote_backup_sink(true);
    let remote_url = format!("http://127.0.0.1:{remote_port}/backups");
    let extra = [
        ("DOMINION_BACKUP_INTERVAL_SECS", "1"),
        ("DOMINION_BACKUP_KEEP", "2"),
        ("DOMINION_BACKUP_REMOTE_URL", remote_url.as_str()),
        ("DOMINION_BACKUP_REMOTE_RETRIES", "0"),
        ("DOMINION_BACKUP_REMOTE_TIMEOUT_SECS", "2"),
    ];
    let app = TestApp::new_with_env(provider_port, "native", &extra);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"원격 실패 작업","autoLearning":false}));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut failed = false;
    while Instant::now() < deadline {
        let audit = app.get("/api/audit");
        if audit
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "backup.remote_failed")
        {
            failed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(failed, "remote failure audit missing");
    let backups = app._root.join("runtime/backups");
    let snapshots = std::fs::read_dir(&backups)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry.path().extension().and_then(|value| value.to_str()) == Some("enc")
                })
                .count()
        })
        .unwrap_or(0);
    assert!(snapshots > 0, "local snapshot was removed after remote failure");
}

#[test]
fn only_one_runner_can_settle_and_promote_an_experiment() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"planning","name":"cas-settle",
            "parameters":{"outline":true},"rationale":"settle CAS"
        }),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy_id,"fixture":"planning_evaluation"}),
    );
    let request_id = request["id"].as_str().unwrap().to_string();
    let claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"cas-runner","ttlSeconds":60}),
    );
    let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
    let good_plan = "1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다";
    let evidence = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &evidence);
    let body = json!({
        "claimToken": token,
        "experimentId":"cas-exp",
        "verdict":evidence["verdict"],
        "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
        "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
        "trials":evidence["trials"],
        "semanticDelta":0.5,"independentSemanticDelta":0.5,
        "semanticEvaluator":{"id":"plan-quality-main","version":1},
        "independentEvaluator":{"id":"plan-quality-independent","version":1},
        "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "plans":[
            {"pair":0,"arm":"baseline","plan":""},
            {"pair":0,"arm":"treatment","plan":good_plan}
        ]
    });
    let base = app.base.clone();
    let url = format!("{base}/api/experiment-requests/{request_id}/complete");
    let mut handles = Vec::new();
    for _ in 0..2 {
        let client = app.client.clone();
        let url = url.clone();
        let body = body.clone();
        handles.push(std::thread::spawn(move || {
            client.post(url).json(&body).send().unwrap().status().as_u16()
        }));
    }
    let mut statuses = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    statuses.sort();
    assert_eq!(statuses, vec![200, 409], "settlement was not exclusive");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "active", "{policy}");
    let requests = app.get("/api/experiment-requests");
    assert_eq!(requests.as_array().unwrap()[0]["status"], "completed");
}

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

