//! Approvals, process restart, backups, key rotation and durable execution.
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
fn scheduled_backup_creates_a_snapshot_without_a_request() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"백업 테스트"},"finish_reason":"stop"}]})]
    });
    let offsite = std::env::temp_dir().join(format!(
        "dominion-offsite-{}",
        uuid::Uuid::new_v4()
    ));
    let app = TestApp::new_with_env(
        port,
        "native",
        &[
            ("DOMINION_BACKUP_INTERVAL_SECS", "1"),
            ("DOMINION_BACKUP_KEEP", "2"),
            ("DOMINION_BACKUP_OFFSITE_DIR", offsite.to_str().unwrap()),
        ],
    );
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"백업 대상 작업","autoLearning":false}));
    let directory = app._root.join("runtime/backups");
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut snapshots = 0;
    while Instant::now() < deadline {
        snapshots = std::fs::read_dir(&directory)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        entry
                            .path()
                            .extension()
                            .and_then(|value| value.to_str())
                            == Some("enc")
                    })
                    .count()
            })
            .unwrap_or(0);
        if snapshots > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(snapshots > 0, "scheduler did not create an encrypted snapshot");
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut offsite_files = 0;
    while Instant::now() < deadline {
        offsite_files = std::fs::read_dir(&offsite)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        entry.path().extension().and_then(|value| value.to_str())
                            == Some("enc")
                    })
                    .count()
            })
            .unwrap_or(0);
        if offsite_files > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(offsite_files > 0, "offsite copy was not created");
    let _ = std::fs::remove_dir_all(offsite);
}

#[test]
fn backup_key_rotation_reencrypts_and_restores() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"키 회전 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let stream = app.chat(&json!({"input":"키 회전 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    let backup = app.post("/api/system/backup", &json!({}));
    let backup_path = backup["path"].as_str().unwrap().to_string();

    // Rotation is maintenance-only.
    let early = app
        .client
        .post(format!("{}/api/system/backup-keys/rotate", app.base))
        .json(&json!({"keepOld":false}))
        .send()
        .unwrap();
    assert_eq!(early.status(), reqwest::StatusCode::CONFLICT);

    app.post("/api/system/maintenance", &json!({"enabled":true}));
    let before = app.get("/api/system/backup-keys");
    assert_eq!(before["activeVersion"], 1, "{before}");
    let rotated = app.post(
        "/api/system/backup-keys/rotate",
        &json!({"keepOld":false}),
    );
    assert_eq!(rotated["ok"], true, "{rotated}");
    assert_eq!(rotated["activeVersion"], 2, "{rotated}");
    assert_eq!(rotated["previousVersion"], 1, "{rotated}");
    assert_eq!(rotated["reencrypted"], 1, "{rotated}");
    assert_eq!(rotated["retainedVersions"], json!([2]), "{rotated}");
    assert!(!app._root.join("runtime/backup-keys/key-1.key").exists());

    // The re-encrypted snapshot restores under the new key.
    let restored = app.post(
        "/api/system/restore",
        &json!({"path":backup_path,"confirm":true}),
    );
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(restored["executions"], 1, "{restored}");
    let executions = app.get("/api/executions");
    assert_eq!(executions.as_array().unwrap().len(), 1);
    assert_eq!(executions.as_array().unwrap()[0]["status"], "completed");

    let kept = app.post(
        "/api/system/backup-keys/rotate",
        &json!({"keepOld":true}),
    );
    assert_eq!(kept["activeVersion"], 3, "{kept}");
    assert_eq!(kept["retainedVersions"], json!([2,3]), "{kept}");
    let status = app.get("/api/system/backup-keys");
    assert_eq!(status["activeVersion"], 3, "{status}");
    assert_eq!(status["versions"], json!([2,3]), "{status}");
    app.post("/api/system/maintenance", &json!({"enabled":false}));
}

#[test]
fn reflection_provider_is_explicit_and_exclusive() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let first = app.post(
        "/api/providers",
        &json!({"name":"teacher-a","baseUrl":"http://127.0.0.1:9/v1","model":"model-a","reflection":true,"enabled":true}),
    );
    assert_eq!(first["reflection"], true, "{first}");
    assert_eq!(first["model"], "model-a", "{first}");
    let first_id = first["id"].as_str().unwrap().to_string();
    let second = app.post(
        "/api/providers",
        &json!({"name":"teacher-b","baseUrl":"http://127.0.0.1:9/v1","model":"model-b","reflection":true,"enabled":true}),
    );
    assert_eq!(second["reflection"], true, "{second}");
    let second_id = second["id"].as_str().unwrap().to_string();
    let providers = app.get("/api/providers");
    let items = providers.as_array().unwrap();
    let a = items.iter().find(|p| p["id"] == first_id.as_str()).unwrap();
    let b = items.iter().find(|p| p["id"] == second_id.as_str()).unwrap();
    assert_eq!(a["reflection"], false, "old reflection provider was not cleared: {providers}");
    assert_eq!(b["reflection"], true, "{providers}");
    assert_eq!(b["model"], "model-b", "{providers}");

    let response = app
        .client
        .post(format!("{}/api/providers", app.base))
        .json(&json!({"name":"broken","baseUrl":"http://127.0.0.1:9/v1","reflection":true}))
        .send()
        .unwrap();
    assert_eq!(
        response.status().as_u16(),
        400,
        "a reflection provider without a model must be rejected"
    );
}

#[test]
fn secret_key_rotation_preserves_provider_secrets_and_restores() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"secret 회전 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.post(
        "/api/providers",
        &json!({
            "name":"key rotation provider",
            "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"mock",
            "apiKey":"sk-rotation-test",
            "enabled":true
        }),
    );
    let provider_id = provider["id"].as_str().unwrap().to_string();
    assert_eq!(provider["hasKey"], true, "{provider}");
    let stream = app.chat(&json!({"input":"키 회전 전 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    let backup = app.post("/api/system/backup", &json!({}));
    let backup_path = backup["path"].as_str().unwrap().to_string();

    let early = app
        .client
        .post(format!("{}/api/system/secret-key/rotate", app.base))
        .json(&json!({"keepOld":false}))
        .send()
        .unwrap();
    assert_eq!(early.status(), reqwest::StatusCode::CONFLICT);

    let key_path = app._root.join("runtime/secret.key");
    let before = std::fs::read(&key_path).unwrap();
    app.post("/api/system/maintenance", &json!({"enabled":true}));
    let response = app
        .client
        .post(format!("{}/api/system/secret-key/rotate", app.base))
        .json(&json!({"keepOld":false}))
        .send()
        .unwrap();
    let status = response.status();
    let text = response.text().unwrap();
    assert!(status.is_success(), "secret rotation failed {status}: {text}");
    let rotated: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rotated["ok"], true, "{rotated}");
    assert!(
        rotated["secretsReencrypted"].as_u64().unwrap_or(0) >= 1,
        "{rotated}"
    );
    assert_eq!(rotated["previousKeys"], 0, "{rotated}");
    assert_eq!(rotated["oldKeyRetained"], false, "{rotated}");
    let after = std::fs::read(&key_path).unwrap();
    assert_ne!(before, after, "secret.key was not replaced");
    let status = app.get("/api/system/secret-key");
    assert_eq!(status["previousKeys"], 0, "{status}");
    assert!(status["oldKeyFiles"].as_array().unwrap().is_empty(), "{status}");

    // Provider secrets and encrypted backups still work after rotation.
    let providers = app.get("/api/providers");
    let provider = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == provider_id.as_str())
        .unwrap();
    assert_eq!(provider["hasKey"], true, "{provider}");
    let restored = app.post(
        "/api/system/restore",
        &json!({"path":backup_path,"confirm":true}),
    );
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(restored["executions"], 1, "{restored}");
    let stream = app.chat(&json!({"input":"키 회전 후 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");

    // Explicit old-key retention keeps one fallback file.
    let kept = app.post(
        "/api/system/secret-key/rotate",
        &json!({"keepOld":true}),
    );
    assert_eq!(kept["previousKeys"], 1, "{kept}");
    assert_eq!(kept["oldKeyRetained"], true, "{kept}");
    let status = app.get("/api/system/secret-key");
    assert_eq!(status["previousKeys"], 1, "{status}");
    assert_eq!(status["oldKeyFiles"].as_array().unwrap().len(), 1, "{status}");
    app.post("/api/system/maintenance", &json!({"enabled":false}));
}

/// Minimal HTTP sink for remote backup PUTs. Success mode echoes the
/// x-dominion-sha256 header; failure mode returns 500.
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

