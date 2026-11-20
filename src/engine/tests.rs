    use super::*;
    use crate::state::AppState;
    use serde_json::Value;
    use serde_json::json;
    #[test]
    fn error_evidence_survives_into_kernel_replay_gate() {
        let mut stats=TurnStats::default();
        collect_tool_event(&mut stats,&json!({
            "type":"tool_error","name":"github_api","id":"blocked",
            "arguments":{"method":"POST"},"result":{"effectUnknown":true,
                "priorOperationId":"old-write","error":"pending effect"},
        }));
        assert_eq!(stats.tool_errors,vec!["github_api"]);
        assert_eq!(stats.evidence.len(),1);
        assert_eq!(stats.evidence[0]["status"],"failed");
        assert_eq!(stats.evidence[0]["outcomeUnknown"],true);
        assert!(!crate::engine::verification::successful_evidence(&stats.evidence[0]));
        assert!(crate::engine::agent_kernel::side_effect_unknown(&stats.evidence));

        let mut denied=TurnStats::default();
        collect_tool_event(&mut denied,&json!({"type":"tool_error",
            "name":"github_api","id":"denied","dispatched":false,
            "result":{"error":"Denied by the user"}}));
        assert!(!crate::engine::agent_kernel::side_effect_unknown(&denied.evidence));
        assert_eq!(denied.tool_success.len(),0);
    }
    #[tokio::test]
    async fn model_order_tool_barrier_reads_the_file_after_write_with_parallel_reads() {
        use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
        use std::sync::{Arc, atomic::{AtomicBool,Ordering}};
        async fn fixture(
            State(observed):State<Arc<AtomicBool>>,
            Json(body):Json<Value>,
        )->axum::response::Response {
            let tool_messages=body["messages"].as_array().into_iter().flatten()
                .filter(|message|message["role"]=="tool").collect::<Vec<_>>();
            if !tool_messages.is_empty(){
                let ids=tool_messages.iter().filter_map(|message|
                    message["tool_call_id"].as_str()).collect::<Vec<_>>();
                let valid=ids==["write-first","read-second","read-third"]
                    && tool_messages[1]["content"].as_str().is_some_and(|value|
                        value.contains("new-from-write"))
                    && tool_messages[2]["content"].as_str().is_some_and(|value|
                        value.contains("new-from-write"));
                observed.store(valid,Ordering::SeqCst);
                return (axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"fixture stops after checking tool results"}))).into_response();
            }
            let calls=[
                ("write-first","workspace_write",json!({"path":"proof.txt","content":"new-from-write"})),
                ("read-second","workspace_read",json!({"path":"proof.txt"})),
                ("read-third","workspace_read_range",
                    json!({"path":"proof.txt","startLine":1,"endLine":1})),
            ].into_iter().enumerate().map(|(index,(id,name,args))|{
                json!({"index":index,"id":id,"function":{
                    "name":name,"arguments":args.to_string()
                }})
            }).collect::<Vec<_>>();
            let event=json!({"choices":[{"delta":{"tool_calls":calls},
                "finish_reason":"tool_calls"}]});
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],
                format!("data: {event}\n\ndata: [DONE]\n\n")).into_response()
        }
        let proof=Arc::new(AtomicBool::new(false));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn({
            let proof=Arc::clone(&proof);
            async move{
                axum::serve(listener,Router::new()
                    .route("/chat/completions",post(fixture)).with_state(proof))
                    .await.unwrap();
            }
        });
        let dir=std::env::temp_dir().join(format!("dominion-barriers-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        std::fs::write(dir.join("proof.txt"),"old-data").unwrap();
        state.store.put("provider",&json!({
            "id":"p","baseUrl":format!("http://{addr}"),"model":"fixture"
        }));
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("policy",&json!({"id":"default","autoApproveAll":true}));
        // Force both previously dangerous optimizer choices to execute, so
        // this regression fails if read-first ever crosses the write barrier.
        crate::engine::algorithms::seed(&state);
        for mut algorithm in state.store.all("algorithm"){
            if matches!(algorithm["kind"].as_str(),
                Some("execution.ordering"|"tool.combination")){
                let target=if algorithm["kind"]=="execution.ordering"{
                    "read-first"
                }else{"parallel-reads"};
                algorithm["status"]=json!(if algorithm["implementation"]==target{
                    "active"
                }else{"disabled"});
                state.store.put("algorithm",&algorithm);
            }
        }
        let request=chat::TurnRequest::from_value(&json!({
            "sessionId":"s","providerId":"p","input":"Update proof.txt then read it twice",
            "autoLearning":false
        })).unwrap();
        let (outcome,stats)=run_quiet_outcome(&state,&request).await;
        assert!(outcome.is_err(),"fixture must terminate with its known HTTP failure");
        assert_eq!(std::fs::read_to_string(dir.join("proof.txt")).unwrap(),
            "new-from-write");
        assert!(proof.load(Ordering::SeqCst),
            "provider must observe the write result then both post-write read results");
        assert_eq!(stats.tool_success.len(),3,"all three native calls must settle");
        assert_eq!(stats.tool_errors.len(),0);
        server.abort();
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn successful_write_evidence_survives_a_later_provider_failure() {
        use axum::{response::IntoResponse, routing::post, Json, Router};
        async fn provider(Json(body): Json<Value>) -> axum::response::Response {
            if body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
            {
                return (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"fixture failure after completed write"})),
                )
                    .into_response();
            }
            let event = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fixture-write","function":{"name":"workspace_write","arguments":json!({"path":"proof.txt","content":"proof"}).to_string()}}]},"finish_reason":"tool_calls"}]});
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                format!("data: {event}\n\ndata: [DONE]\n\n"),
            )
                .into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/chat/completions", post(provider)),
            )
            .await
            .unwrap()
        });
        let root = std::env::temp_dir().join(format!("dominion-outcome-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        state.store.put(
            "provider",
            &json!({"id":"p","baseUrl":format!("http://{addr}"),"model":"fixture"}),
        );
        state.store.put("session", &json!({"id":"s"}));
        state
            .store
            .put("policy", &json!({"id":"default","autoApproveAll":true}));
        let request=chat::TurnRequest::from_value(&json!({"sessionId":"s","providerId":"p","input":"Write the proof file","autoLearning":false})).unwrap();
        let (outcome, stats) = run_quiet_outcome(&state, &request).await;
        assert!(outcome.unwrap_err().contains("503"));
        assert_eq!(
            std::fs::read_to_string(root.join("proof.txt")).unwrap(),
            "proof"
        );
        assert_eq!(stats.tool_success, vec!["workspace_write"]);
        assert_eq!(stats.evidence.len(), 1);
        assert_eq!(stats.evidence[0]["tool"], "workspace_write");
        assert_eq!(
            state.store.all("execution_operation")[0]["status"],
            "completed"
        );
        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }
