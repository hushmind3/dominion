//! Dedicated runtime contract tests; kept outside the production turn driver.
use super::*;

#[cfg(test)]
mod reasoning_controls_tests {
    use super::*;

    #[test]
    fn explicit_thinking_off_never_inherits_agent_or_provider_effort() {
        for kind in ["openai","local","codex"] {
            let (effort, thinking)=resolve_reasoning_effort(
                None,Some(false),Some("high"),Some(false),Some("max"),kind);
            assert_eq!(thinking,Some(false));
            assert_eq!(effort.as_deref(),if kind=="codex"{Some("none")}else{None});
        }
    }

    #[test]
    fn explicit_effort_none_wins_even_against_inherited_thinking() {
        let (effort, thinking)=resolve_reasoning_effort(
            Some("none"),Some(true),Some("high"),Some(false),Some("max"),"deepseek");
        assert_eq!(effort,None);
        assert_eq!(thinking,Some(false));
    }

    #[test]
    fn saved_effort_wins_over_legacy_agent_thinking_false() {
        let (effort, thinking)=resolve_reasoning_effort(
            None,None,Some("ultra"),Some(false),Some("high"),"codex");
        assert_eq!(effort.as_deref(),Some("ultra"));
        assert_eq!(thinking,Some(true));
        let (effort, thinking)=resolve_reasoning_effort(
            None,None,Some(""),Some(false),Some("high"),"codex");
        assert_eq!(effort.as_deref(),Some("high"));
        assert_eq!(thinking,Some(true));
    }

    #[test]
    fn automatic_inheritance_only_when_user_does_not_override() {
        let (effort, thinking)=resolve_reasoning_effort(
            None,None,Some("low"),None,Some("high"),"openai");
        assert_eq!(effort.as_deref(),Some("low"));
        assert_eq!(thinking,Some(true));
        let (effort, thinking)=resolve_reasoning_effort(
            Some("medium"),Some(true),Some("high"),Some(false),Some("max"),"openai");
        assert_eq!(effort.as_deref(),Some("medium"));
        assert_eq!(thinking,Some(true));
        let (effort, thinking)=resolve_reasoning_effort(
            None,None,None,Some(false),None,"deepseek");
        assert_eq!(effort,None);
        assert_eq!(thinking,Some(false));
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;
    use serde_json::json;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-outcome-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn execution(status: &str, acceptance: bool, acceptance_text: Value) -> Value {
        json!({
            "status": status,
            "acceptanceChecks":[{"type":"file_equals","path":"test.txt","content":"ok"}],
            "acceptancePassed": acceptance,
            "acceptanceText": acceptance_text,
            "strategyCompliance": []
        })
    }

    #[test]
    fn shared_failure_observation_avoids_duplicate_workspace_and_command_reads() {
        let (state,dir)=state();
        std::fs::write(dir.join("target.rs"),"real evidence").unwrap();
        let args=json!({"path":"target.rs","command":"cargo test --locked"});
        let observed=failure_observation(&state,"terminal_exec",&args,"tool failed");
        assert_eq!(observed["kind"],"tool_failure");
        assert_eq!(observed["pathState"]["path"]["exists"],true);
        assert!(observed["pathState"].get("command").is_none(),
            "a terminal command must not be treated as a filesystem path");
        assert!(observed["environmentFingerprint"].is_string());
        let first=environment_fingerprint(&state,"terminal_exec",&args);
        let second=environment_fingerprint(&state,"terminal_exec",&args);
        assert_eq!(first,second);
        assert_ne!(first,environment_fingerprint(&state,"workspace_read",&args));
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn causal_diagnostic_uses_real_tool_receipt_and_changes_repair() {
        let (state, dir) = state();
        std::fs::write(dir.join("diagnostic-source.txt"), "actual on-disk evidence").unwrap();
        let session_id = "diagnostic-session";
        let execution_id = uuid::Uuid::new_v4().to_string();
        state.store.put("session", &json!({"id":session_id}));
        state.store.put("execution", &json!({
            "id":execution_id,"sessionId":session_id,"status":"running",
            "request":"inspect the source and choose a safe repair"
        }));
        let session = json!({"id":session_id});
        let request = TurnRequest {
            client_turn_id: None, direct_chat: false,
            session_id: Some(session_id.into()),
            input: "inspect the source and choose a safe repair".into(),
            attachment_ids: Vec::new(), agent_id: None, provider_id: None,
            model: None, tools_enabled: true, tools_execution_enabled: true,
            thinking: None, reasoning_effort: None, reasoning_language: None,
            auto_learning: false, system_override: None, task_id: None,
            parent_task_id: None, task_revision: None, verification_owner: None,
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let input = json!({"path":"diagnostic-source.txt"});
        let call = ToolCall {
            id: "diagnostic-proof".into(), name: "workspace_read".into(),
            arguments: input.to_string(),
        };
        let (result, ok, _) = dispatch_tool_call(
            &state, &tx, &session, &request, session_id, &execution_id,
            &call, input, &metrics, 1, 0, 0, false,
            "adaptation_diagnostic", None, None, None,
        ).await.expect("real read must pass through the native gateway");
        assert!(ok, "{result}");
        assert_eq!(result["content"], "actual on-disk evidence");
        let operations = state.store.execution_operations(&execution_id).unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0]["origin"], "adaptation_diagnostic");
        assert_eq!(operations[0]["status"], "completed");
        assert_eq!(operations[0]["result"]["content"], result["content"]);

        let adaptation = json!({
            "hypotheses":[{"id":"missing"},{"id":"stale"}],
            "diagnostics":[{"id":"probe","tool":"workspace_read",
                "inputs":{"path":"diagnostic-source.txt"},
                "predicate":{"type":"equals","from":"result","path":"/content",
                    "value":"actual on-disk evidence"},
                "predictions":[
                    {"hypothesisId":"missing","expected":false},
                    {"hypothesisId":"stale","expected":true}]
            }],
            "interventions":[
                {"id":"make","tool":"workspace_write",
                    "inputs":{"path":"rewritten.txt","content":"new"},
                    "hypothesisIds":["missing"],"expectedProgress":2.0},
                {"id":"inspect","tool":"workspace_list",
                    "inputs":{"path":"."},
                    "hypothesisIds":["stale"],"expectedProgress":1.0}
            ]
        });
        let candidate = crate::engine::adaptation::diagnostics::choose(
            &adaptation,&[],&[],"shell_exec",&json!({})
        ).unwrap();
        let receipt = crate::engine::adaptation::diagnostics::observe(
            &candidate, &operations[0]["result"], true,
            operations[0]["id"].as_str().unwrap(),
        );
        assert_eq!(receipt["operationId"], operations[0]["id"]);
        assert_eq!(receipt["verdict"], "true");
        let refuted = crate::engine::adaptation::diagnostics::contradicted(&[receipt]);
        assert_eq!(refuted, vec!["missing"]);
        let repair = crate::engine::adaptation_loop::choose_intervention_with_context(
            &adaptation, "shell_exec", &json!({"command":"false"}),
            false, &json!({}), &refuted,
        ).unwrap();
        assert_eq!(repair["id"], "inspect");
        assert!(!dir.join("rewritten.txt").exists(),
            "diagnostic reads never execute a model-proposed write");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn missing_program_checkpoint_prevents_any_tool_dispatch(){
        let (state,dir)=state();
        let session=json!({"id":"s1"});
        let request=TurnRequest::from_value(&json!({"input":"create a file"})).unwrap();
        let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
        let metrics=std::sync::Mutex::new(Metrics::default());
        let mut budget=super::super::super::budget::BudgetLedger::new(
            MAX_ROUNDS,MAX_TOOL_CALLS,MAX_ELAPSED
        );
        let node=json!({"type":"call_tool","id":"write1","tool":"workspace_write",
            "inputs":{"path":"new.txt","content":"should not be written"}});
        let mut resolved=json!({});
        let mut checkpoint=json!({});
        let mut seq=0u64;
        let result=execute_program_tool(
            &state,&tx,&session,&request,"s1","nonexistent-execution",
            &node,&mut resolved,&mut checkpoint,&metrics,&mut budget,&mut seq,0,true,
        ).await;
        assert!(result.is_err(),"missing write-ahead record must block dispatch");
        assert!(result.unwrap_err().contains("체크포인트"));
        assert!(state.store.all("execution_operation").is_empty());
        assert!(!dir.join("new.txt").exists());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn discovered_read_uses_the_existing_real_tool_gateway() {
        let (state,dir)=state();
        std::fs::write(dir.join("observed.txt"),"original disk evidence").unwrap();
        let session_id="cap-session";
        let execution_id="cap-execution";
        state.store.put("session",&json!({"id":session_id}));
        state.store.put("execution",&json!({"id":execution_id,"sessionId":session_id,"status":"running"}));
        let request=TurnRequest {
            client_turn_id: None, direct_chat: false,
            session_id:Some(session_id.into()),input:"inspect observed.txt".into(),
            attachment_ids:Vec::new(),agent_id:None,provider_id:None,
            model:None,tools_enabled:true,tools_execution_enabled:true,
            thinking:None,reasoning_effort:None,reasoning_language:None,
            auto_learning:false,system_override:None,task_id:None,
            parent_task_id:None,task_revision:None,verification_owner:None,
        };
        let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
        let metrics=std::sync::Mutex::new(Metrics::default());
        let search_args=json!({"query":"workspace read"});
        let search_call=ToolCall {
            id:"lookup".into(),name:"dominion_tool_search".into(),
            arguments:search_args.to_string(),
        };
        let (search_result,ok,_)=dispatch_tool_call(
            &state,&tx,&json!({"id":session_id}),&request,session_id,execution_id,
            &search_call,search_args,&metrics,1,0,0,false,"model",None,None,None,
        ).await.expect("search must use the native gateway");
        assert!(ok,"{search_result}");
        assert!(search_result["matches"].as_array().unwrap().iter()
            .any(|item|item["name"]=="workspace_read"));
        let args=json!({"name":"workspace_read","arguments":{"path":"observed.txt"}});
        let call=ToolCall {
            id:"discovered".into(),name:"dominion_tool_call".into(),
            arguments:args.to_string(),
        };
        let (result,ok,_)=dispatch_tool_call(
            &state,&tx,&json!({"id":session_id}),&request,session_id,execution_id,
            &call,args,&metrics,2,0,1,false,"model",None,None,None,
        ).await.expect("discovered read must dispatch through the normal tool gateway");
        assert!(ok,"{result}");
        assert_eq!(result["content"],"original disk evidence");
        let operations=state.store.execution_operations(execution_id).unwrap();
        assert_eq!(operations.len(),2);
        assert!(operations.iter().any(|operation|
            operation["tool"]=="dominion_tool_search"&&operation["status"]=="completed"));
        assert!(operations.iter().any(|operation|
            operation["tool"]=="workspace_read"&&operation["status"]=="completed"
                &&operation["origin"]=="model"));
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finalization_hash_ignores_mutable_counters_and_timings() {
        let base = json!({
            "id":"e1","sessionId":"s","agentId":"a","request":"do it",
            "status":"completed","taskOutcome":"succeeded",
            "acceptancePassed":true,"acceptanceText":"check",
            "appliedImprovementIds":["i1"],
            "strategyCompliance":[],"finalMessageId":"m1",
            "tokens":10,"toolCalls":1,"iterations":2,
            "finishedAt":"2026-11-05T10:00:00Z","firstTokenMs":120
        });
        let operations = vec![json!({
            "id":"o1","runId":"e1","tool":"workspace_write",
            "arguments":{"path":"a.txt"},"origin":"model",
            "status":"completed","result":{"ok":true},
            "startedAt":"2026-11-05T09:59:00Z"
        })];
        let first = finalization_payload_hash(&base, &operations);
        let mut retried = base.clone();
        retried["tokens"] = json!(999);
        retried["iterations"] = json!(7);
        retried["finishedAt"] = json!("2026-11-05T10:05:00Z");
        retried["firstTokenMs"] = json!(900);
        let mut reordered = operations.clone();
        reordered.insert(
            0,
            json!({"id":"o0","runId":"e1","tool":"workspace_read","status":"completed"}),
        );
        let mut reversed = reordered.clone();
        reversed.reverse();
        assert_eq!(first, finalization_payload_hash(&retried, &operations));
        assert_eq!(
            finalization_payload_hash(&base, &reordered),
            finalization_payload_hash(&base, &reversed)
        );
        retried["taskOutcome"] = json!("failed");
        assert_ne!(first, finalization_payload_hash(&retried, &operations));
    }

    #[tokio::test]
    async fn program_resumes_from_a_durable_checkpoint_without_repeating_completed_tools() {
        let (state, dir) = state();
        state.store.put("policy", &json!({"id":"default","autoApproveAll":true}));
        let execution_id = "exec-resume";
        state.store.put(
            "execution",
            &json!({
                "id": execution_id,
                "status": "running",
                "taskId": execution_id,
                "sessionId": "s1",
                "request": "fallback.txt 파일을 만들어줘",
                "programCheckpoint": {
                    "schemaVersion":1,
                    "programRevision":"p-resume",
                    "status":"running",
                    "frameStack":[],
                    "nextIndex":1,
                    "resolvedInputs":{"try":{"exitCode":7}},
                    "completedOperationIds":["try"],
                    "pendingOperationIds":[]
                }
            }),
        );
        let session = json!({"id":"s1","roomId":null});
        let request = TurnRequest {
            client_turn_id: None, direct_chat: false,
            session_id: Some("s1".into()),
            input: "fallback.txt 파일을 만들어줘".into(),
            attachment_ids: Vec::new(),
            agent_id: None,
            provider_id: None,
            model: None,
            tools_enabled: true,
            tools_execution_enabled: true,
            thinking: None,
            reasoning_effort: None,
            reasoning_language: None,
            auto_learning: false,
            system_override: None,
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            verification_owner: None,
        };
        let program = json!({
            "revision":"p-resume",
            "nodes":[
                {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
                {"type":"branch","id":"check",
                 "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
                 "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
                 "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]}
            ]
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let mut budget = super::super::super::budget::BudgetLedger::new(
            MAX_ROUNDS,
            MAX_TOOL_CALLS,
            MAX_ELAPSED,
        );
        let mut op_seq = 0u64;
        let generation = state.cancellation_generation("s1");
        run_program(
            &state, &tx, &session, &request, "s1", execution_id, &program,
            &metrics, &mut budget, &mut op_seq, generation,
        )
        .await
        .expect("resume program");
        assert_eq!(
            std::fs::read_to_string(dir.join("fallback.txt")).unwrap_or_default(),
            "FALLBACK"
        );
        let operations = state
            .store
            .all("execution_operation")
            .into_iter()
            .filter(|operation| operation["runId"] == execution_id)
            .collect::<Vec<_>>();
        assert_eq!(operations.len(), 1, "completed try node was re-executed: {operations:?}");
        assert_eq!(operations[0]["tool"], "workspace_write");
        let execution = state.store.get("execution", execution_id).unwrap();
        assert_eq!(execution["programCheckpoint"]["status"], "completed");
        assert!(execution["programCheckpoint"]["completedOperationIds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "try"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_partially_completed_loop_resumes_with_distinct_iteration_ids() {
        let (state, dir) = state();
        state.store.put("policy", &json!({"id":"default","autoApproveAll":true}));
        state.store.put("execution", &json!({
            "id":"loop-resume","taskId":"loop-resume","sessionId":"s1",
            "status":"running","request":"반복 작성",
            "programCheckpoint":{
                "schemaVersion":1,"programRevision":"p-loop","status":"running",
                "frameStack":[],"nextIndex":0,
                "resolvedInputs":{"condition":true,"write":{"path":"repeat.txt"}},
                "completedOperationIds":["repeat:0:write"],
                "pendingOperationIds":[],"loopProgress":{}
            }
        }));
        let session = json!({"id":"s1"});
        let request = TurnRequest {
            client_turn_id: None, direct_chat: false,
            session_id: Some("s1".into()), input: "반복 작성".into(),
            attachment_ids: Vec::new(), agent_id: None, provider_id: None,
            model: None, tools_enabled: true, tools_execution_enabled: true,
            thinking: None, reasoning_effort: None, reasoning_language: None,
            auto_learning: false, system_override: None, task_id: None,
            parent_task_id: None, task_revision: None, verification_owner: None,
        };
        let program = json!({
            "revision":"p-loop",
            "nodes":[{
                "type":"loop","id":"repeat","maxIterations":2,
                "predicate":{"type":"equals","from":"condition","value":true},
                "body":[{"type":"call_tool","id":"write",
                    "tool":"workspace_write",
                    "inputs":{"path":"repeat.txt","content":"REPEAT"}}]
            }]
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let mut budget = super::super::super::budget::BudgetLedger::new(
            MAX_ROUNDS, MAX_TOOL_CALLS, MAX_ELAPSED,
        );
        let mut op_seq = 0;
        run_program(&state, &tx, &session, &request, "s1", "loop-resume",
            &program, &metrics, &mut budget, &mut op_seq,
            state.cancellation_generation("s1")).await.unwrap();
        let execution = state.store.get("execution", "loop-resume").unwrap();
        let checkpoint = &execution["programCheckpoint"];
        assert_eq!(checkpoint["loopProgress"]["repeat"], 2);
        assert_eq!(checkpoint["status"], "completed");
        assert!(checkpoint["resolvedInputs"]["write"]["path"].as_str()
            .is_some_and(|path| path.ends_with("repeat.txt")));
        let completed = checkpoint["completedOperationIds"].as_array().unwrap();
        assert!(completed.iter().any(|x| x == "repeat:0:write"));
        assert!(completed.iter().any(|x| x == "repeat:1:write"));
        let ops = state.store.all("execution_operation");
        assert_eq!(ops.len(), 1, "resumed iteration zero was replayed: {ops:?}");
        assert_eq!(std::fs::read_to_string(dir.join("repeat.txt")).unwrap(), "REPEAT");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_failed_program_revision_does_not_mark_the_program_completed() {
        let (state, dir) = state();
        state.store.put("policy", &json!({"id":"default","autoApproveAll":true}));
        let execution_id = "exec-revision-failure";
        state.store.put(
            "execution",
            &json!({
                "id": execution_id, "status": "running",
                "taskId": execution_id, "sessionId": "s1",
                "request": "실패 프로그램"
            }),
        );
        let session = json!({"id":"s1"});
        let request = TurnRequest {
            client_turn_id: None, direct_chat: false,
            session_id: Some("s1".into()),
            input: "실패 프로그램".into(),
            attachment_ids: Vec::new(),
            agent_id: None,
            provider_id: None,
            model: None,
            tools_enabled: true,
            tools_execution_enabled: true,
            thinking: None,
            reasoning_effort: None,
            reasoning_language: None,
            auto_learning: false,
            system_override: None,
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            verification_owner: None,
        };
        let program = json!({
            "revision":"p-revision-failure",
            "nodes":[
                {"type":"call_tool","id":"try","tool":"shell_exec",
                 "inputs":{"command":"exit 7"},"saveAs":"try","onFailure":"revise"}
            ]
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let mut budget = super::super::super::budget::BudgetLedger::new(
            MAX_ROUNDS, MAX_TOOL_CALLS, MAX_ELAPSED,
        );
        let mut op_seq = 0u64;
        let generation = state.cancellation_generation("s1");
        run_program(
            &state, &tx, &session, &request, "s1", execution_id, &program,
            &metrics, &mut budget, &mut op_seq, generation,
        )
        .await
        .expect("program runs and reports the unresolved failure");
        let execution = state.store.get("execution", execution_id).unwrap();
        assert_eq!(
            execution["programCheckpoint"]["status"], "needs_review",
            "{execution}"
        );
        assert!(!execution["programCheckpoint"]["programFailure"].is_null());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_records_the_whole_applied_bundle_and_real_context() {
        let (state, dir) = state();
        let task_id = "task-snapshot";
        crate::engine::episode::start_root(
            &state,
            crate::engine::episode::RootSpec {
                task_id,
                intent: "bench 작업",
                family: "chat",
                source: "owner",
                source_message_id: None,
                scope: json!({"partition":"local","sessionId":"session-1"}),
                mandatory: Vec::new(),
            },
        );
        let request = TurnRequest {
            client_turn_id: None, direct_chat: false,
            session_id: Some("session-1".into()),
            input: "bench 작업".into(),
            attachment_ids: Vec::new(),
            agent_id: None,
            provider_id: Some("p".into()),
            model: Some("m".into()),
            tools_enabled: true,
            tools_execution_enabled: true,
            thinking: None,
            reasoning_effort: None,
            reasoning_language: None,
            auto_learning: false,
            system_override: None,
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            verification_owner: None,
        };
        let strategies = vec![
            json!({
                "id":"s1","revision":"r1","kind":"strategy","authority":"adaptive",
                "text":"first","assistanceMode":"harness_assisted",
                "verification":{"type":"procedure","appliesTo":{"requestContains":["bench"]}}
            }),
            json!({
                "id":"s2","revision":"r2","kind":"strategy","authority":"adaptive",
                "text":"second","assistanceMode":"model_guided",
                "verification":{"type":"none","appliesTo":{}}
            }),
            // Legacy record without a revision must still be recorded as an
            // applied arm instead of silently disappearing from the bundle.
            json!({"id":"s3","kind":"tool_policy","authority":"adaptive","text":"legacy"}),
        ];
        let context_items = vec![
            json!({"kind":"system_prompt","digest":"d1"}),
            json!({"kind":"history","count":0}),
        ];
        record_selection_snapshot(
            &state,
            "exec-1",
            task_id,
            "session-1",
            &request,
            &strategies,
            context_items,
            "watermark-1".into(),
            json!({"providerId":"p","model":"m","toolMode":"native"}),
        )
        .unwrap();

        let snapshots = state.store.all("selection_snapshot");
        assert_eq!(snapshots.len(), 1);
        let snapshot = &snapshots[0];
        let arms = snapshot["policyBundle"]["arms"].as_array().unwrap();
        assert_eq!(arms.len(), 3, "every applied arm must be recorded");
        assert_eq!(arms[0]["armId"], "s1");
        assert_eq!(arms[1]["armId"], "s2");
        assert_eq!(arms[2]["armId"], "s3");
        assert!(!arms[2]["revision"].as_str().unwrap_or("").is_empty());
        assert_eq!(arms[0]["role"], "policy");
        assert_eq!(arms[1]["role"], "advice");
        assert_eq!(snapshot["policyBundle"]["selection"], "deterministic");
        assert_eq!(snapshot["policyBundle"]["probability"], 1.0);
        assert_eq!(snapshot["decisions"][0]["selected"], "bundle:exec-1");
        assert_eq!(
            snapshot["decisions"][0]["context"]["appliedArmIds"],
            json!(["s1", "s2", "s3"])
        );
        assert_eq!(snapshot["memoryWatermark"], "watermark-1");
        assert_eq!(snapshot["contextItems"].as_array().unwrap().len(), 2);
        assert_eq!(snapshot["completeness"]["reproducible"], true);
        assert_eq!(snapshot["model"]["model"], "m");
        assert!(!snapshot["snapshotHash"].as_str().unwrap_or("").is_empty());

        let decisions = state.store.all("strategy_decision");
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0]["selected"], "bundle:exec-1");
        assert_eq!(decisions[0]["candidates"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn compliance_summary_prefers_violation_over_followed() {
        use crate::engine::adaptation::types::Compliance;
        let mixed = vec![
            json!({
                "strategyId":"a","applicable":true,
                "result":{"triggered":true,"followed":true}
            }),
            json!({
                "strategyId":"b","applicable":true,
                "result":{"triggered":true,"followed":false}
            }),
        ];
        assert_eq!(compliance_summary(&mixed), Compliance::Violated);
        assert_eq!(compliance_summary(&mixed[..1]), Compliance::Followed);

        let violated_and_unknown = vec![
            json!({
                "strategyId":"a","applicable":true,
                "result":{"verified":true,"triggered":true,"followed":false}
            }),
            json!({
                "strategyId":"b","applicable":true,
                "result":{"verified":false,"triggered":true,"status":"unknown"}
            }),
        ];
        assert_eq!(
            compliance_summary(&violated_and_unknown),
            Compliance::Violated,
            "unknown must not mask a confirmed violation"
        );
        let detail = compliance_detail(&violated_and_unknown);
        assert_eq!(detail["summary"], "violated");
        assert_eq!(detail["violated"], json!(["a"]));
        assert_eq!(detail["unknown"], json!(["b"]));
        assert_eq!(detail["total"], 2);

        assert_eq!(
            compliance_summary(&[json!({
                "strategyId":"c","applicable":true,
                "result":{"triggered":true,"status":"unknown"}
            })]),
            Compliance::Unknown
        );
        assert_eq!(
            compliance_summary(&[json!({
                "strategyId":"d","applicable":true,
                "result":{"triggered":false}
            })]),
            Compliance::NotTriggered
        );
        assert_eq!(compliance_summary(&[]), Compliance::NotApplicable);
    }

    #[test]
    fn completed_is_not_automatically_succeeded() {
        assert_eq!(
            determine_task_outcome(&execution("completed", false, Value::Null)),
            TaskOutcome::AnsweredUnverified
        );
        assert_eq!(
            determine_task_outcome(&execution("completed", false, json!("check"))),
            TaskOutcome::Failed
        );
        assert_eq!(
            determine_task_outcome(&execution("completed", true, json!("check"))),
            TaskOutcome::Succeeded
        );
    }

    #[test]
    fn standing_violation_forces_failure() {
        let mut value = execution("completed", true, json!("check"));
        value["strategyCompliance"] = json!([{
            "strategyId":"s1","authority":"standing","applicable":true,
            "result":{"verified":true,"triggered":true,"followed":false}
        }]);
        assert_eq!(determine_task_outcome(&value), TaskOutcome::Failed);
    }

    #[test]
    fn simple_deterministic_tasks_skip_the_planner() {
        let checks = Some((
            "create report".to_string(),
            json!([{"type":"file_changed","path":"report.txt"}]),
        ));
        assert!(!needs_plan("report.txt 파일을 만들어줘", &checks));
        assert!(needs_plan("파일을 만들고 그리고 내용도 확인해", &checks));
        assert!(needs_plan("report.txt 파일을 만들어줘", &None));
        assert!(needs_plan(&"가".repeat(300), &checks));
    }

    #[test]
    fn adaptive_strategy_violation_still_allows_task_success() {
        let mut value = execution("completed", true, json!("check"));
        value["strategyCompliance"] = json!([{
            "strategyId":"s1","authority":"adaptive","applicable":true,
            "result":{"verified":true,"triggered":true,"followed":false}
        }]);
        assert_eq!(determine_task_outcome(&value), TaskOutcome::Succeeded);
    }

    #[test]
    fn success_experience_is_idempotent() {
        let (state, dir) = state();
        let execution = json!({
            "id":"exec-once","sessionId":"s","agentId":null,
            "acceptanceText":"create a verified file",
            "finalMessageId":"m1","finalAnswer":"verified",
            "toolsUsed":["workspace_write"]
        });
        assert!(record_success_experience(
            &state,
            &execution,
            "exec-once",
            TaskOutcome::Succeeded
        )
        .is_some());
        assert!(record_success_experience(
            &state,
            &execution,
            "exec-once",
            TaskOutcome::Succeeded
        )
        .is_none());
        let experiences = state.store.all("experience");
        assert_eq!(experiences.len(), 1);
        assert_eq!(experiences[0]["occurrences"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancelled_and_failed_are_not_success() {
        assert_eq!(
            determine_task_outcome(&execution("cancelled", true, json!("check"))),
            TaskOutcome::Cancelled
        );
        assert_eq!(
            determine_task_outcome(&execution("failed", true, json!("check"))),
            TaskOutcome::Failed
        );
    }
}

#[cfg(test)]
mod harness_tool_selection_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_discovery_selects_core_or_query_relevant_tools() {
        let definitions = vec![
            json!({"type":"function","function":{"name":"workspace_write","description":"write"}}),
            json!({"type":"function","function":{"name":"web_search","description":"search the web"}}),
            json!({"type":"function","function":{"name":"github_api","description":"github"}}),
        ];
        let core = select_tool_definitions(
            definitions.clone(),
            "검색",
            &json!({"implementation":"core"}),
        );
        assert_eq!(core.len(), 2);
        assert!(core.iter().all(|tool| tool_name(tool) != "github_api"));
        let relevant = select_tool_definitions(
            definitions,
            "github issue",
            &json!({"implementation":"query-relevant","parameters":{"extra":2}}),
        );
        assert!(relevant.iter().any(|tool| tool_name(tool) == "workspace_write"));
        assert!(relevant.iter().any(|tool| tool_name(tool) == "github_api"));
    }

    #[test]
    fn room_tool_selection_is_zero_for_casual_and_exact_for_actions() {
        let definitions = vec![
            json!({"type":"function","function":{"name":"workspace_read","description":"Read a workspace file"}}),
            json!({"type":"function","function":{"name":"workspace_write","description":"Write a workspace file"}}),
            json!({"type":"function","function":{"name":"shell_exec","description":"Run a shell command"}}),
            json!({"type":"function","function":{"name":"web_search","description":"Search the web"}}),
            json!({"type":"function","function":{"name":"memory_store","description":"Store a memory"}}),
            json!({"type":"function","function":{"name":"memory_search","description":"Search memory"}}),
            json!({"type":"function","function":{"name":"mcp_github_search","description":"Search GitHub issues and pull requests"}}),
        ];
        let empty = std::collections::HashSet::new();
        assert!(select_room_tools(definitions.clone(), "안녕", &empty, false).is_empty());
        assert!(select_room_tools(definitions.clone(), "오늘 날씨는 어때 보여?", &empty, false).is_empty());
        assert!(select_room_tools(definitions.clone(), "책 읽는 건 좋아해?", &empty, false).is_empty());
        assert!(select_room_tools(
            definitions.clone(),
            "내가 첫 턴에 뭐라고 했는지 기억해?",
            &empty,
            false
        )
        .iter()
        .all(|tool| !matches!(tool_name(tool), "memory_store" | "memory_search")));
        let memory = select_room_tools(definitions.clone(), "메모리에 저장해", &empty, false);
        assert!(memory.iter().any(|tool| tool_name(tool) == "memory_store"), "{memory:?}");
        let files = select_room_tools(definitions.clone(), "파일을 읽고 저장해", &empty, false);
        let file_names = files.iter().map(tool_name).collect::<Vec<_>>();
        assert!(file_names.contains(&"workspace_read"), "{file_names:?}");
        assert!(file_names.contains(&"workspace_write"), "{file_names:?}");
        assert!(!file_names.contains(&"mcp_github_search"), "{file_names:?}");
        let mcp = select_room_tools(definitions.clone(), "github 이슈를 검색해", &empty, false);
        assert!(mcp.iter().any(|tool| tool_name(tool) == "mcp_github_search"));
        let mut recent = std::collections::HashSet::new();
        recent.insert("shell_exec".to_string());
        let continued = select_room_tools(definitions.clone(), "계속", &recent, false);
        assert!(continued.iter().any(|tool| tool_name(tool) == "shell_exec"));
        let accepted = select_room_tools(definitions, "작업", &empty, true);
        assert!(!accepted.is_empty());
    }

    #[test]
    fn room_acceptance_fallback_never_exposes_every_discovered_mcp_schema() {
        let mut definitions = vec![
            json!({"type":"function","function":{"name":"workspace_list"}}),
            json!({"type":"function","function":{"name":"workspace_read"}}),
            json!({"type":"function","function":{"name":"workspace_write"}}),
            json!({"type":"function","function":{"name":"shell_exec"}}),
        ];
        for index in 0..60 {
            definitions.push(json!({
                "type":"function",
                "function":{"name":format!("mcp_misc_{index}"),"description":"opaque adapter"}
            }));
        }
        let selected = select_room_tools(
            definitions,
            "작업",
            &std::collections::HashSet::new(),
            true,
        );
        let names = selected.iter().map(tool_name).collect::<Vec<_>>();
        assert_eq!(selected.len(), 4, "the first step must not load 60 unrelated schemas");
        assert!(names.contains(&"workspace_read"));
        assert!(names.contains(&"workspace_write"));
        assert!(names.contains(&"shell_exec"));
        assert!(!names.iter().any(|name| name.starts_with("mcp_misc_")));
    }

    #[test]
    fn fallback_without_native_tools_is_also_bounded() {
        let definitions = (0..32)
            .map(|index| json!({"type":"function","function":{"name":format!("mcp_extra_{index}")}}))
            .collect::<Vec<_>>();
        let selected = select_room_tools(
            definitions,
            "작업",
            &std::collections::HashSet::new(),
            true,
        );
        assert_eq!(selected.len(), 8);
    }

    #[test]
    fn progressive_disclosure_preserves_search_and_call_but_bounds_large_catalogs() {
        let mut available=crate::engine::tools::definitions();
        for index in 0..90 {
            available.push(json!({"type":"function","function":{
                "name":format!("mcp__catalog__item_{index:03}"),
                "description":"Unrelated third party integration",
                "parameters":{"type":"object","properties":{}}
            }}));
        }
        let shown=tool_catalog::progressive_tools(available.clone(),"read code and inspect files",false);
        let names=shown.iter().map(tool_name).collect::<Vec<_>>();
        assert!(names.contains(&"dominion_tool_search"));
        assert!(names.contains(&"dominion_tool_call"));
        assert!(names.contains(&"workspace_read"));
        assert!(shown.len()<=16,"large catalogs should not flood small models");
        assert!(shown.len()<available.len()/3);
        assert_eq!(names.iter().filter(|name|**name=="dominion_tool_search").count(),1);
    }

    #[test]
    fn casual_private_chat_keeps_native_tools_and_lazy_discovery_for_mcp() {
        let mut all = vec![
            json!({"type":"function","function":{"name":"workspace_read","description":"Read files"}}),
            json!({"type":"function","function":{"name":"workspace_write","description":"Write files"}}),
        ];
        for index in 0..60 {
            all.push(json!({"type":"function","function":{
                "name":format!("mcp__external__tool_{index}"),
                "description":"Long third-party schema ".repeat(60),
                "parameters":{"type":"object","properties":{"large":{"type":"string","description":"x".repeat(300)}}}
            }}));
        }
        let casual = tool_catalog::progressive_tools(all.clone(), "안녕", false);
        let names = casual.iter().map(tool_name).collect::<Vec<_>>();
        assert_eq!(&names[..2], ["dominion_tool_search", "dominion_tool_call"],
            "the discovery gateway leads the catalog");
        assert!(names.contains(&"workspace_read") && names.contains(&"workspace_write"),
            "native core tools stay directly callable on an ordinary turn: {names:?}");
        assert!(names.iter().all(|name| !name.starts_with("mcp__")),
            "the MCP tail still requires discovery: {names:?}");
        let bytes = |defs: &[Value]| defs.iter().map(|d|d.to_string().len()).sum::<usize>();
        assert!(bytes(&casual) * 10 < bytes(&all),
            "casual conversation should avoid full MCP parameter schemas");

        let action = tool_catalog::progressive_tools(all.clone(), "파일을 읽고 수정해", false);
        assert!(action.iter().any(|d| tool_name(d) == "workspace_read"));
        assert!(action.iter().any(|d| tool_name(d) == "workspace_write"));

        // An explicit acceptance must preserve tool access even for "응".
        let resumed = tool_catalog::progressive_tools(all, "응", true);
        assert!(resumed.iter().any(|d| tool_name(d) == "workspace_read"));
    }

    #[test]
    fn casual_room_predicate_skips_only_non_action_turns() {
        assert!(!tool_catalog::room_tool_request("안녕"));
        assert!(!tool_catalog::room_tool_request("오늘 기분은 어때?"));
        assert!(tool_catalog::room_tool_request("파일 읽어서 수정해"));
        assert!(tool_catalog::room_tool_request("계속"));
    }

    #[test]
    fn small_catalog_retains_native_tool_contracts() {
        let definitions=crate::engine::tools::definitions().into_iter()
            .filter(|definition|matches!(tool_name(definition),
                "workspace_read"|"workspace_write"|"dominion_tool_search"|"dominion_tool_call"))
            .collect::<Vec<_>>();
        let shown=tool_catalog::progressive_tools(definitions,"write file",false);
        assert_eq!(shown.len(),4);
        assert!(shown.iter().any(|definition|tool_name(definition)=="workspace_write"));
    }

    #[test]
    fn path_arguments_are_normalized_only_when_the_kernel_is_active() {
        let raw = json!({"path":"  ./sub//file.txt  ","content":"x"});
        let untouched = normalize_tool_arguments(raw.clone(), "none");
        assert_eq!(untouched["path"], "  ./sub//file.txt  ");
        let normalized = normalize_tool_arguments(raw, "normalize-paths");
        assert_eq!(normalized["path"], "sub/file.txt");
        assert_eq!(normalized["content"], "x");
    }

    #[test]
    fn read_first_ordering_puts_observation_before_mutation() {
        assert!(tool_is_read_only("workspace_read"));
        assert!(tool_is_read_only("web_search"));
        assert!(!tool_is_read_only("workspace_write"));
        assert!(!tool_is_read_only("shell_exec"));
        let mut calls = vec![
            ToolCall { id: "1".into(), name: "workspace_write".into(), arguments: "{}".into() },
            ToolCall { id: "2".into(), name: "workspace_read".into(), arguments: "{}".into() },
        ];
        calls.sort_by_key(|call| !tool_is_read_only(&call.name));
        assert_eq!(calls[0].name, "workspace_read");
        assert_eq!(calls[1].name, "workspace_write");
    }

    #[test]
    fn tool_schemas_fit_the_selected_provider_budget() {
        let tools = (0..30)
            .map(|index| json!({"type":"function","function":{"name":format!("tool{index}"),"description":"x".repeat(100)}}))
            .collect::<Vec<_>>();
        let kept = trim_tool_definitions(tools, 2_000);
        assert!(!kept.is_empty());
        assert!(kept.len() < 30);
        let total = kept
            .iter()
            .map(|tool| serde_json::to_string(tool).unwrap().len())
            .sum::<usize>();
        assert!(total <= 2_000 || kept.len() == 1);
    }

    #[test]
    fn a_long_prompt_gets_a_bounded_unique_contract_intent() {
        let input = "가".repeat(10_000);
        let intent = bounded_contract_intent(&input);
        assert!(intent.chars().count() <= 4096);
        assert!(intent.ends_with(']'));
        assert_ne!(intent, bounded_contract_intent(&format!("{input}x")));
        assert_eq!(bounded_contract_intent("short"), "short");
    }
}
