//! experiments backup E2E contracts backed by the shared real-process fixture.
use super::*;

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
fn policy_experiment_activates_a_candidate_through_the_runner() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("You are the planner") {
            return vec![json!({"choices":[{"delta":{"content":"1. bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일 내용을 검증한다"},"finish_reason":"stop"}]})];
        }
        let has_plan = messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Internal execution plan")
        });
        if !has_plan {
            // Without a plan the model claims completion without doing work.
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        let done = messages
            .last()
            .is_some_and(|message| message["role"] == "tool")
            || messages.iter().any(|message| {
                message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("모든 검증 조건이 충족")
            });
        if done {
            vec![json!({"choices":[{"delta":{"content":"파일을 생성했습니다."},"finish_reason":"stop"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"policy-w1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_audit.txt\",\"content\":\"audit-harness\"}"}},
                {"index":1,"id":"policy-w2","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_report.txt\",\"content\":\"REPORT-OK\"}"}}
            ]},"finish_reason":"tool_calls"}]})]
        }
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"planning",
            "name":"plan-before-acting-test",
            "parameters":{"outline":true},
            "rationale":"policy evaluator test",
        }),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    assert_eq!(policy["status"], "candidate");
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy_id, "fixture":"planning_evaluation"}),
    );
    assert_eq!(request["status"], "queued");
    assert_eq!(request["kind"], "policy");

    let bench_root = std::env::temp_dir().join(format!(
        "dominion-policy-eval-{}",
        uuid::Uuid::new_v4()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_dominion-benchmark"))
        .env("DOMINION_BIN", env!("CARGO_BIN_EXE_dominion"))
        .env("BENCH_CLAIM_URL", &app.base)
        .env("BENCH_PROVIDER_BASE", format!("http://127.0.0.1:{port}/v1"))
        .env("BENCH_MODEL", "mock")
        .env("BENCH_MODE", "smoke")
        .env("BENCH_PAIRS", "10")
        .env("BENCH_ROOT", &bench_root)
        .output()
        .expect("run policy experiment");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "policy runner failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("request completed: completed"),
        "policy request not completed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let requests = app.get("/api/experiment-requests");
    let request = &requests.as_array().unwrap()[0];
    assert_eq!(request["status"], "completed", "{request}");
    assert_eq!(request["kind"], "policy");
    assert_eq!(request["independentUnits"], 10, "{request}");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .expect("policy");
    assert_eq!(
        policy["status"], "active",
        "outer gate did not activate the candidate: {policy}\nrequest: {request}"
    );
    assert_eq!(policy["evidenceGrade"], "E2");
    assert_eq!(policy["parameters"]["outline"], true);
    assert_eq!(policy["lastExperiment"]["independentUnits"], 10);
    assert_eq!(policy["semanticVerified"], true);
    assert!(
        policy["lastExperiment"]["semanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05,
        "semantic gate metric missing: {policy}"
    );
    assert!(
        policy["lastExperiment"]["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05,
        "independent semantic gate metric missing: {policy}"
    );
    let _ = std::fs::remove_dir_all(bench_root);
}


#[test]
#[ignore = "the env-gated production experiment scheduler (DOMINION_EXPERIMENT_SCHEDULER/RUNNER/INTERVAL_SECS) is not read anywhere in src; this test predates its removal."]
fn the_production_scheduler_runs_a_queued_experiment_without_manual_invocation() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"스케줄러 응답"},"finish_reason":"stop"}]})]
    });
    let provider_base = format!("http://127.0.0.1:{port}/v1");
    let extra = [
        ("DOMINION_BENCHMARK", "1"),
        ("DOMINION_EXPERIMENT_SCHEDULER", "1"),
        (
            "DOMINION_EXPERIMENT_RUNNER",
            env!("CARGO_BIN_EXE_dominion-benchmark"),
        ),
        ("DOMINION_EXPERIMENT_INTERVAL_SECS", "1"),
        ("DOMINION_EXPERIMENT_PAIRS", "1"),
        ("DOMINION_EXPERIMENT_MODEL", "mock"),
        (
            "DOMINION_EXPERIMENT_PROVIDER_BASE",
            provider_base.as_str(),
        ),
    ];
    let app = TestApp::new_with_env(port, "native", &extra);
    let agent = app.post(
        "/api/agents",
        &json!({"name":"scheduler daemon agent","model":"mock"}),
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
            "text":"scheduler daemon procedure",
            "rationale":"production scheduler test",
            "verification":verification,
        }),
    );
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"strategyId":strategy["id"], "fixture":"audit_report"}),
    );
    assert_eq!(request["status"], "queued");

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut completed = Value::Null;
    while Instant::now() < deadline {
        if let Some(record) = app
            .get("/api/experiment-requests")
            .as_array()
            .and_then(|items| items.first())
            .cloned()
        {
            if record["status"] == "completed" || record["status"] == "failed" {
                completed = record;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(
        completed["status"], "completed",
        "scheduler did not complete the request: {completed}"
    );
    let run = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(run) = app
                .get("/api/scheduler-runs")
                .as_array()
                .and_then(|items| items.first())
                .cloned()
            {
                break run;
            }
            assert!(
                Instant::now() < deadline,
                "scheduler run record was not written after the request completed"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    assert_eq!(run["status"], "success", "{run}");
    assert!(
        run["stdoutTail"]
            .as_str()
            .is_some_and(|text| text.contains("request completed")),
        "runner output missing from scheduler record: {run}"
    );
}


#[test]
fn policy_activation_is_rejected_when_the_independent_evaluator_disagrees() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let make_request = |name: &str| -> (String, String, String) {
        let policy = app.post(
            "/api/benchmark/policies",
            &json!({
                "slot":"planning","name":name,
                "parameters":{"outline":true},"rationale":"meta gate test"
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
            &json!({"runnerId":format!("meta-{name}"),"ttlSeconds":60}),
        );
        (policy_id, request_id, claimed["request"]["claimToken"].as_str().unwrap().to_string())
    };
    let good_plan = "1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다";
    let vague_plan = "1. 작업을 진행한다\n2. 완료한다";
    let evidence = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &evidence);

    // 1) Version mismatch rejects promotion even with a perfect claimed delta.
    let (policy_id, request_id, token) = make_request("version-mismatch");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"meta-1",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
            "semanticDelta":0.9,"independentSemanticDelta":0.9,
            "semanticEvaluator":{"id":"plan-quality-main","version":999},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[
                {"pair":0,"arm":"baseline","plan":""},
                {"pair":0,"arm":"treatment","plan":good_plan}
            ]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "candidate", "{policy}");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("version")),
        "{completed}"
    );

    // 2) Correct versions, but the independent score does not improve.
    let (policy_id, request_id, token) = make_request("independent-disagrees");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"meta-2",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
            "semanticDelta":0.9,"independentSemanticDelta":0.9,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[
                {"pair":0,"arm":"baseline","plan":vague_plan},
                {"pair":0,"arm":"treatment","plan":vague_plan}
            ]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "candidate", "{policy}");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("semantic gate rejected")),
        "{completed}"
    );

    // 3) Both evaluators agree, so the candidate activates.
    let (policy_id, request_id, token) = make_request("both-agree");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"meta-3",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
            "semanticDelta":0.9,"independentSemanticDelta":0.9,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[
                {"pair":0,"arm":"baseline","plan":""},
                {"pair":0,"arm":"treatment","plan":good_plan}
            ]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "active", "{policy}");
    assert!(
        policy["lastExperiment"]["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05,
        "{policy}"
    );
}


#[test]
#[ignore = "the env-gated production experiment scheduler (DOMINION_EXPERIMENT_SCHEDULER/RUNNER/INTERVAL_SECS) is not read anywhere in src; this test predates its removal."]
fn policy_search_selects_the_best_revision_through_the_scheduler() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("You are the planner") {
            return vec![json!({"choices":[{"delta":{"content":"1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다"},"finish_reason":"stop"}]})];
        }
        let has_plan = messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Internal execution plan")
        });
        if !has_plan {
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        let done = messages
            .last()
            .is_some_and(|message| message["role"] == "tool")
            || messages.iter().any(|message| {
                message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("모든 검증 조건이 충족")
            });
        if done {
            vec![json!({"choices":[{"delta":{"content":"파일을 생성했습니다."},"finish_reason":"stop"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"search-w1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_audit.txt\",\"content\":\"audit-harness\"}"}},
                {"index":1,"id":"search-w2","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_report.txt\",\"content\":\"REPORT-OK\"}"}}
            ]},"finish_reason":"tool_calls"}]})]
        }
    });
    let provider_base = format!("http://127.0.0.1:{port}/v1");
    let extra = [
        ("DOMINION_BENCHMARK", "1"),
        ("DOMINION_EXPERIMENT_SCHEDULER", "1"),
        ("DOMINION_EXPERIMENT_RUNNER", env!("CARGO_BIN_EXE_dominion-benchmark")),
        ("DOMINION_EXPERIMENT_INTERVAL_SECS", "1"),
        ("DOMINION_EXPERIMENT_PAIRS", "10"),
        ("DOMINION_EXPERIMENT_MODEL", "mock"),
        ("DOMINION_EXPERIMENT_PROVIDER_BASE", provider_base.as_str()),
    ];
    let app = TestApp::new_with_env(port, "native", &extra);
    let search = app.post(
        "/api/benchmark/policy-search",
        &json!({
            "slot":"planning",
            "candidates":[
                {"name":"outline-all","parameters":{"outline":true}},
                {"name":"outline-only-long","parameters":{"outline":true,"minChars":1000}}
            ]
        }),
    );
    let search_id = search["searchId"].as_str().unwrap().to_string();
    let candidates = search["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["candidate"]["name"].as_str().unwrap().to_string(),
                item["candidate"]["id"].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let outline_all = candidates
        .iter()
        .find(|(name, _)| name == "outline-all")
        .unwrap()
        .1
        .clone();

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut selection = Value::Null;
    loop {
        let requests = app.get("/api/experiment-requests");
        let completed = requests
            .as_array()
            .unwrap()
            .iter()
            .filter(|request| request["status"] == "completed")
            .count();
        if let Some(record) = app
            .get("/api/policy-searches")
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == search_id.as_str()))
            .cloned()
        {
            selection = record;
            if completed >= 2 && !selection["selectedPolicyId"].is_null() {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "search did not settle: completed={completed} selection={selection}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(
        selection["selectedPolicyId"], outline_all.as_str(),
        "wrong revision selected: {selection}"
    );
    let policies = app.get("/api/policies");
    let policies = policies.as_array().unwrap();
    let selected = policies
        .iter()
        .find(|policy| policy["id"] == outline_all.as_str())
        .unwrap();
    assert_eq!(selected["status"], "active", "{selected}");
    assert!(
        selected["lastExperiment"]["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05
    );
    let long_only = policies
        .iter()
        .find(|policy| policy["name"] == "outline-only-long")
        .unwrap();
    assert_ne!(
        long_only["status"], "active",
        "candidate without a plan must not be selected: {long_only}"
    );
    assert_eq!(selection["candidates"].as_array().unwrap().len(), 2);
}


#[test]
fn policy_patches_activate_rollback_and_reject_stale_bases() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let activate = |name: &str, parameters: Value| -> Value {
        app.post(
            "/api/benchmark/policies/state",
            &json!({"slot":"planning","name":name,"parameters":parameters}),
        )["active"]
            .clone()
    };
    let base = activate("patch-base", json!({"outline":true}));
    let base_id = base["id"].as_str().unwrap().to_string();
    let patch = app.post(
        "/api/benchmark/policy-patches",
        &json!({
            "slot":"planning",
            "targetPolicyId":base_id,
            "body":{"parameters":{"minChars":120}},
            "rationale":"threshold patch"
        }),
    );
    let patch_id = patch["id"].as_str().unwrap().to_string();
    assert_eq!(patch["status"], "candidate");

    let weak = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch_id,"verdict":"preliminary_support","delta":0.4,
            "semanticDelta":0.4,"independentSemanticDelta":0.4,"independentUnits":4
        }),
    );
    assert_eq!(weak["status"], "candidate", "{weak}");

    let good = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch_id,"verdict":"preliminary_support","delta":0.4,
            "semanticDelta":0.4,"independentSemanticDelta":0.3,"independentUnits":8
        }),
    );
    assert_eq!(good["status"], "active", "{good}");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|policy| policy["id"] == base_id.as_str())
        .unwrap();
    assert_eq!(policy["parameters"]["minChars"], 120, "{policy}");
    assert_eq!(policy["appliedPatchId"], patch_id.as_str());

    let rollback = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch_id,"verdict":"regressed","delta":-0.3,
            "semanticDelta":0.0,"independentSemanticDelta":0.0,"independentUnits":8
        }),
    );
    assert_eq!(rollback["status"], "rolled_back", "{rollback}");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|policy| policy["id"] == base_id.as_str())
        .unwrap();
    assert!(policy["parameters"]["minChars"].is_null(), "{policy}");
    assert_eq!(policy["parameters"]["outline"], true);

    // A patch is invalidated when the target policy moves to a new revision.
    let base2 = activate("patch-base-2", json!({"outline":true}));
    let patch2 = app.post(
        "/api/benchmark/policy-patches",
        &json!({
            "slot":"planning","targetPolicyId":base2["id"],
            "body":{"parameters":{"minChars":60}},"rationale":"stale base"
        }),
    );
    let patch2_id = patch2["id"].as_str().unwrap().to_string();
    let _ = activate("patch-base-3", json!({"outline":true,"minChars":10}));
    let rejected = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch2_id,"verdict":"preliminary_support","delta":0.4,
            "semanticDelta":0.4,"independentSemanticDelta":0.3,"independentUnits":8
        }),
    );
    assert_eq!(rejected["status"], "rejected", "{rejected}");
    assert!(rejected["reason"]
        .as_str()
        .is_some_and(|text| text.contains("base revision")));
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
fn the_recovery_slot_changes_retry_behavior_only_after_a_valid_experiment() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"recovery",
            "name":"fewer-acceptance-retries",
            "parameters":{"maxAcceptanceRetries":0,"maxProcedureRepairs":0},
            "rationale":"recovery slot test"
        }),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    assert_eq!(policy["status"], "candidate");

    // Underpowered experiment cannot activate the slot.
    let underpowered = app.post(
        "/api/benchmark/policies/experiment",
        &json!({
            "policyId":policy_id,"experimentId":"recovery-small",
            "verdict":"preliminary_support","delta":0.2,
            "semanticDelta":0.2,"independentSemanticDelta":0.2,
            "independentUnits":4,"oracleVersion":"v1"
        }),
    );
    assert_eq!(underpowered["status"], "candidate", "{underpowered}");
    let stream = app.chat(&json!({
        "input":"bench/harness_report.txt 파일을 만들어줘",
        "autoLearning":false
    }));
    assert!(
        stream.contains("acceptance_retry"),
        "candidate policy must not change behavior: {stream}"
    );

    let activated = app.post(
        "/api/benchmark/policies/experiment",
        &json!({
            "policyId":policy_id,"experimentId":"recovery-powered",
            "verdict":"preliminary_support","delta":0.2,
            "semanticDelta":0.2,"independentSemanticDelta":0.2,
            "independentUnits":8,"oracleVersion":"v1"
        }),
    );
    assert_eq!(activated["status"], "active", "{activated}");
    let stream = app.chat(&json!({
        "input":"bench/harness_report.txt 파일을 만들어줘",
        "autoLearning":false
    }));
    assert!(
        !stream.contains("acceptance_retry"),
        "active recovery policy did not change the retry branch: {stream}"
    );
    let executions = app.get("/api/executions");
    assert!(
        executions
            .as_array()
            .unwrap()
            .iter()
            .any(|execution| execution["status"] == "failed"),
        "{executions}"
    );
    let policies = app.get("/api/policies");
    let active = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(active["status"], "active");
    assert_eq!(active["slot"], "recovery");
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

