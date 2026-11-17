//! run recovery E2E contracts backed by the shared real-process fixture.
use super::*;

#[test]
fn approval_is_operation_bound_and_survives_restart() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let finished = request["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .is_some_and(|message| message["role"] == "tool");
        if finished {
            vec![
                json!({"choices":[{"delta":{"content":"저장했습니다."},"finish_reason":"stop"}]}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"durable-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"durable.txt\",\"content\":\"DURABLE-OK\"}"}}]},"finish_reason":"tool_calls"}]}),
            ]
        }
    });
    let app = TestApp::new(port, "native");
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"durable.txt 파일을 만들어줘",
        "toolsEnabled":true,
        "autoLearning":false
    }));
    let pending = app.wait_pending_approval(20);
    assert_eq!(pending["status"], "pending");
    assert!(pending["runId"].as_str().is_some_and(|value| !value.is_empty()));
    assert!(pending["operationId"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(
        pending["argsDigest"].as_str().map(str::len),
        Some(32),
        "approval must bind the exact tool arguments"
    );
    assert!(
        !root.join("durable.txt").exists(),
        "tool ran before approval"
    );
    let session_id = pending["sessionId"].as_str().unwrap().to_string();
    let task_id = pending["taskId"].as_str().unwrap().to_string();
    let task_revision = pending["taskRevision"].as_str().unwrap().to_string();
    assert!(!task_id.is_empty());
    assert!(
        pending["operationKey"]
            .as_str()
            .is_some_and(|value| value.starts_with("tool:")),
        "approval must bind the logical operation"
    );
    drop(app);
    let _ = handle.join();

    // Restart on the same data root: the pending approval is still there and
    // can be decided without a live waiter.
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let pending = app.wait_pending_approval(10);
    let id = pending["id"].as_str().unwrap().to_string();
    let resolved = app.post("/api/approvals", &json!({"id":id,"allow":true}));
    assert_eq!(resolved["resolved"], true, "{resolved}");
    assert_eq!(resolved["resumed"], false, "{resolved}");
    assert!(!root.join("durable.txt").exists());

    // Counterexample: the same session/tool/arguments in a *different* task
    // must not inherit this approval.
    let other = app.chat_async(&json!({
        "sessionId":session_id,
        "input":"durable.txt 파일을 만들어줘",
        "toolsEnabled":true,
        "autoLearning":false
    }));
    let other_pending = app.wait_pending_approval(10);
    assert_ne!(
        other_pending["id"].as_str().unwrap_or(""),
        id,
        "a different task reused an unrelated approval"
    );
    app.post(
        "/api/approvals",
        &json!({"id":other_pending["id"],"allow":false}),
    );
    // The scripted model retries after a denial; deny those retries too so the
    // unfinished task cannot silently reuse the original approval.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !other.is_finished() {
        if let Some(pending) = app
            .get("/api/approvals")
            .as_array()
            .and_then(|items| items.first())
        {
            app.post(
                "/api/approvals",
                &json!({"id":pending["id"],"allow":false}),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = other.join();
    assert!(!root.join("durable.txt").exists());
    assert!(
        app.get("/api/approvals").as_array().unwrap().is_empty(),
        "the unrelated task left an unresolved approval"
    );

    // A retry that explicitly resumes the original task/revision/operation
    // consumes the orphaned approval exactly once.
    let stream = app.chat(&json!({
        "sessionId":session_id,
        "taskId":task_id,
        "taskRevision":task_revision,
        "input":"durable.txt 파일을 만들어줘",
        "toolsEnabled":true,
        "autoLearning":false
    }));
    assert_eq!(
        std::fs::read_to_string(root.join("durable.txt")).unwrap_or_default(),
        "DURABLE-OK",
        "{stream}"
    );
    assert!(app.get("/api/approvals").as_array().unwrap().is_empty());
}


#[test]
fn closing_the_sse_client_does_not_cancel_background_work() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(600));
        vec![
            json!({"choices":[{"delta":{"content":"백그라운드에서 완료했습니다."},"finish_reason":"stop"}]}),
        ]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let response = app
        .client
        .post(format!("{}/api/chat", app.base))
        .json(&json!({"input":"백그라운드 작업","autoLearning":false}))
        .send()
        .unwrap();
    // Simulate the desktop/web UI closing the SSE connection immediately.
    drop(response);

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut finished = None;
    while Instant::now() < deadline {
        if let Some(execution) = app
            .get("/api/executions")
            .as_array()
            .and_then(|items| items.first())
            .cloned()
        {
            match execution["status"].as_str() {
                Some("completed") => {
                    finished = Some(execution);
                    break;
                }
                Some("cancelled") => {
                    panic!("client disconnect cancelled background work: {execution}")
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let execution = finished.expect("background turn did not finish");
    assert_eq!(
        execution["taskOutcome"], "answered_unverified",
        "{execution}"
    );
    let session_id = execution["sessionId"].as_str().unwrap();
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    assert!(
        messages.as_array().unwrap().iter().any(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("백그라운드에서 완료"))
        }),
        "assistant reply was not persisted: {messages}"
    );
}


#[test]
fn a_clean_interrupted_run_is_woken_and_resumed_after_restart() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(1_200));
        vec![
            json!({"choices":[{"delta":{"content":"재개 완료."},"finish_reason":"stop"}]}),
        ]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"재개 테스트 작업",
        "autoLearning":false
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.get("/api/executions/active").as_array().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "turn did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(app);
    let _ = handle.join();

    // Restart on the same data root. The dead run has no side effects and no
    // pending approval, so startup wake must enqueue exactly one resume job.
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut wake = Value::Null;
    while Instant::now() < deadline {
        if let Some(record) = app
            .get("/api/run-wakes")
            .as_array()
            .and_then(|items| items.first())
            .cloned()
        {
            wake = record;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(wake["decision"], "queued", "{wake}");
    let job_id = wake["jobId"].as_str().expect("wake job").to_string();

    let deadline = Instant::now() + Duration::from_secs(25);
    let mut completed = false;
    while Instant::now() < deadline {
        if let Some(job) = app
            .get("/api/jobs")
            .as_array()
            .and_then(|items| items.iter().find(|job| job["id"] == job_id.as_str()))
        {
            if matches!(job["status"].as_str(), Some("completed" | "failed")) {
                completed = job["status"] == "completed";
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(completed, "resume job did not complete");
    let executions = app.get("/api/executions");
    assert!(
        executions.as_array().unwrap().len() >= 2,
        "resume did not create a new attempt: {executions}"
    );
    let session_id = wake["sessionId"].as_str().unwrap();
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    let user_messages = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .count();
    assert_eq!(
        user_messages, 1,
        "resume must not duplicate the owner message: {messages}"
    );
    assert!(!requests.lock().unwrap().is_empty());
}


#[test]
fn load_matrix_completes_parallel_sessions_without_cross_talk() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        std::thread::sleep(Duration::from_millis(80));
        let input = request["messages"]
            .as_array()
            .and_then(|messages| {
                messages
                    .iter()
                    .rev()
                    .find(|message| message["role"] == "user")
            })
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        vec![json!({"choices":[{"delta":{"content":format!("응답:{input}")},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let mut handles = Vec::new();
    for index in 0..6 {
        handles.push(app.chat_async(&json!({
            "input":format!("동시 작업 {index}"),
            "autoLearning":false
        })));
    }
    for handle in handles {
        let stream = handle.join().unwrap();
        assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    }
    let executions = app.get("/api/executions");
    assert_eq!(executions.as_array().unwrap().len(), 6);
    assert!(executions
        .as_array()
        .unwrap()
        .iter()
        .all(|execution| execution["status"] == "completed"));
    assert!(app.get("/api/executions/active").as_array().unwrap().is_empty());
}


#[test]
fn crash_drill_leaves_no_running_execution_after_restart() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(900));
        vec![json!({"choices":[{"delta":{"content":"크래시 드릴 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let root = app._root.clone();
    let mut handles = Vec::new();
    for index in 0..4 {
        handles.push(app.chat_async(&json!({
            "input":format!("크래시 드릴 {index}"),
            "autoLearning":false
        })));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.get("/api/executions/active").as_array().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "runs did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Let each turn persist its owner message before the crash so the wake
    // policy has a real prompt to resume.
    std::thread::sleep(Duration::from_millis(150));
    drop(app);
    for handle in handles {
        let _ = handle.join();
    }

    let app = TestApp::from_root(port, "native", root, &[]);
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let executions = app.get("/api/executions");
        let running = executions
            .as_array()
            .unwrap()
            .iter()
            .filter(|execution| execution["status"] == "running")
            .count();
        let jobs = app.get("/api/jobs");
        let busy = jobs
            .as_array()
            .unwrap()
            .iter()
            .filter(|job| matches!(job["status"].as_str(), Some("queued" | "running")))
            .count();
        if running == 0 && busy == 0 {
            let executions = executions.as_array().unwrap();
            assert!(!executions.is_empty(), "no execution was recorded");
            assert!(
                executions.iter().all(|execution| matches!(
                    execution["status"].as_str(),
                    Some("interrupted" | "completed" | "failed" | "cancelled")
                )),
                "dangling status: {executions:?}"
            );
            assert!(
                !app.get("/api/run-wakes").as_array().unwrap().is_empty(),
                "crash recovery did not record wake decisions"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "recovery did not settle: running={running} busy={busy}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}


#[test]
fn live_restore_requires_maintenance_and_preserves_backup_executions() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"복구 드릴 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let stream = app.chat(&json!({"input":"복구 드릴 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    let executions = app.get("/api/executions");
    assert_eq!(executions.as_array().unwrap().len(), 1);
    let session_id = executions.as_array().unwrap()[0]["sessionId"]
        .as_str()
        .unwrap()
        .to_string();

    let backup = app.post("/api/system/backup", &json!({}));
    assert_eq!(backup["ok"], true, "{backup}");
    assert_eq!(backup["encrypted"], true, "{backup}");
    let backup_path = backup["path"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&backup_path).is_file());

    // A later turn must disappear after restoring the older snapshot.
    let _ = app.chat(&json!({"input":"백업 이후 작업","autoLearning":false}));
    assert_eq!(app.get("/api/executions").as_array().unwrap().len(), 2);

    // Restore is rejected outside maintenance mode.
    let early = app
        .client
        .post(format!("{}/api/system/restore", app.base))
        .json(&json!({"path":backup_path,"confirm":true}))
        .send()
        .unwrap();
    assert_eq!(early.status(), reqwest::StatusCode::CONFLICT);

    let enabled = app.post("/api/system/maintenance", &json!({"enabled":true}));
    assert_eq!(enabled["maintenance"], true);
    let blocked = app
        .client
        .post(format!("{}/api/sessions", app.base))
        .json(&json!({"title":"should be blocked"}))
        .send()
        .unwrap();
    assert_eq!(blocked.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert!(app.get("/api/executions").as_array().is_some());

    let restored = app.post(
        "/api/system/restore",
        &json!({"path":backup_path,"confirm":true}),
    );
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(restored["executions"], 1, "{restored}");
    let executions = app.get("/api/executions");
    assert_eq!(
        executions.as_array().unwrap().len(),
        1,
        "live restore did not roll back to the snapshot: {executions}"
    );
    assert_eq!(executions.as_array().unwrap()[0]["status"], "completed");
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    assert!(
        messages.as_array().unwrap().iter().any(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("복구 드릴 응답"))
        }),
        "restored transcript missing: {messages}"
    );

    let disabled = app.post("/api/system/maintenance", &json!({"enabled":false}));
    assert_eq!(disabled["maintenance"], false);
    let session = app.post("/api/sessions", &json!({"title":"after restore"}));
    assert!(session["id"].as_str().is_some_and(|value| !value.is_empty()));
}


#[test]
fn the_isolated_runner_claims_and_completes_a_queued_experiment() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let provider_id = app.get("/api/providers")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let agent = app.post(
        "/api/agents",
        &json!({"name":"scheduler agent","providerId":provider_id,"model":"mock"}),
    );
    let agent_id = agent["id"].as_str().unwrap();
    let verification = json!({
        "type":"procedure",
        "appliesTo":{"paths":["bench/harness_report.txt"]},
        "nodes":[
            {"id":"audit","tool":"workspace_write","arguments":{"path":"bench/harness_audit.txt","content":"audit-harness"}},
            {"id":"report","tool":"workspace_write","arguments":{"path":"bench/harness_report.txt","content":"REPORT-OK"}}
        ],
        "edges":[{"from":"audit","to":"report"}],
        "after":[{"kind":"file_contains","path":"bench/harness_audit.txt","text":"audit-harness"}]
    });
    let strategy = app.post(
        "/api/benchmark/strategies",
        &json!({
            "agentId":agent_id,
            "assistanceMode":"harness_assisted",
            "text":"harness 절차로 audit 후 report를 실행한다",
            "rationale":"scheduler integration test",
            "verification":verification,
        }),
    );
    let strategy_id = strategy["id"].as_str().unwrap().to_string();
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"strategyId":strategy_id, "fixture":"audit_report"}),
    );
    assert_eq!(request["status"], "queued", "{request}");

    let bench_root = std::env::temp_dir().join(format!(
        "dominion-scheduler-{}",
        uuid::Uuid::new_v4()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_dominion-benchmark"))
        .env("DOMINION_BIN", env!("CARGO_BIN_EXE_dominion-rust"))
        .env("BENCH_CLAIM_URL", &app.base)
        .env(
            "BENCH_PROVIDER_BASE",
            format!("http://127.0.0.1:{port}/v1"),
        )
        .env("BENCH_MODEL", "mock")
        .env("BENCH_MODE", "smoke")
        .env("BENCH_PAIRS", "10")
        .env("BENCH_FIXTURE", "audit_report")
        .env("BENCH_ROOT", &bench_root)
        .output()
        .expect("run benchmark scheduler");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "runner failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("request completed: completed"),
        "runner did not complete the request\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let requests = app.get("/api/experiment-requests");
    let request = &requests.as_array().unwrap()[0];
    assert_eq!(request["status"], "completed", "{request}");
    assert!(
        request["experimentId"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "{request}"
    );
    assert_eq!(request["independentUnits"], 10, "{request}");
    let improvements = app.get("/api/improvements");
    let improvement = improvements
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == strategy_id.as_str())
        .expect("strategy");
    assert_eq!(
        improvement["evidenceGrade"], "E2",
        "experiment did not promote: {improvement}"
    );
    assert_eq!(improvement["validatedBy"], "isolated_experiment");
    assert_eq!(improvement["rollout"], "canary");
    let _ = std::fs::remove_dir_all(bench_root);
}


#[test]
fn experiment_claim_is_token_scoped_and_can_be_released() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let agent = app.post(
        "/api/agents",
        &json!({"name":"claim agent","model":"mock"}),
    );
    let verification = json!({
        "type":"procedure",
        "appliesTo":{"paths":["bench/harness_report.txt"]},
        "nodes":[
            {"id":"audit","tool":"workspace_write","arguments":{"path":"bench/harness_audit.txt","content":"audit-harness"}},
            {"id":"report","tool":"workspace_write","arguments":{"path":"bench/harness_report.txt","content":"REPORT-OK"}}
        ],
        "edges":[{"from":"audit","to":"report"}],
        "after":[{"kind":"file_contains","path":"bench/harness_audit.txt","text":"audit-harness"}]
    });
    let strategy = app.post(
        "/api/benchmark/strategies",
        &json!({
            "agentId":agent["id"],
            "assistanceMode":"harness_assisted",
            "text":"claim lifecycle",
            "rationale":"claim lifecycle test",
            "verification":verification,
        }),
    );
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"strategyId":strategy["id"], "fixture":"audit_report"}),
    );
    let request_id = request["id"].as_str().unwrap().to_string();

    let claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"test-runner","ttlSeconds":60}),
    );
    assert_eq!(claimed["request"]["id"], request_id.as_str());
    assert_eq!(claimed["request"]["status"], "claimed");
    let token = claimed["request"]["claimToken"].as_str().unwrap();
    assert!(!token.is_empty());

    // Wrong token cannot complete or release.
    let wrong = app
        .client
        .post(format!(
            "{}/api/experiment-requests/{request_id}/complete",
            app.base
        ))
        .json(&json!({"claimToken":"wrong","verdict":"inconclusive"}))
        .send()
        .unwrap();
    assert_eq!(wrong.status(), reqwest::StatusCode::CONFLICT);
    let wrong_release = app
        .client
        .post(format!(
            "{}/api/experiment-requests/{request_id}/release",
            app.base
        ))
        .json(&json!({"claimToken":"wrong"}))
        .send()
        .unwrap();
    assert_eq!(wrong_release.status(), reqwest::StatusCode::CONFLICT);

    let released = app.post(
        &format!("/api/experiment-requests/{request_id}/release"),
        &json!({"claimToken":token,"reason":"runner stopped"}),
    );
    assert_eq!(released["request"]["status"], "queued");
    let reclaimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"second-runner","ttlSeconds":60}),
    );
    assert_eq!(reclaimed["request"]["status"], "claimed");
    assert_ne!(
        reclaimed["request"]["claimToken"].as_str().unwrap_or(""),
        token
    );
    // A crashed scheduler runner can release every claim it owns in one call.
    let released = app.post(
        "/api/experiment-requests/release-runner",
        &json!({"runnerId":"second-runner"}),
    );
    assert_eq!(released["released"], 1, "{released}");
    let requests = app.get("/api/experiment-requests");
    assert_eq!(requests.as_array().unwrap()[0]["status"], "queued");
}

