pub mod autonomy;
pub mod execution_scope;
pub mod agent_kernel;
pub mod workspace_scope;
pub mod goal_control;
pub mod adaptation_loop;
pub mod distill;
pub mod effect;
pub mod agent_growth;
pub mod algorithms;
pub mod algorithm_eval;
pub mod attachments;
pub mod backup_keys;
pub mod bridge;
pub mod chat;
pub mod browser;
pub mod compact;
pub mod channel_adapter;
pub mod decompose;
pub mod debate;
pub mod dev;
pub mod discord;
pub mod adaptation;
pub mod episode;
pub mod experience;
pub mod procedure;
pub mod program;
pub mod verification;
pub mod reflect;
pub mod gateway;
pub mod github_work;
pub mod model_cost;
pub mod resource_lock;
pub mod evaluation;
pub mod cli_model;
pub mod improve;
pub mod code_index;
pub mod recovery;
pub mod reasoning;
pub mod remote_backup;
pub mod jobs;
pub mod learning;
pub mod lease;
pub mod llm;
pub mod mcp;
pub mod maintenance;
pub mod memory;
pub mod models;
pub mod codex_oauth;
pub mod oauth;
pub mod personas;
pub mod plan_quality;
pub mod policy;
pub mod policy_patch;
pub mod preflight_fixture;
pub mod principle;
pub mod router;
pub mod route_calibration;
pub mod rooms;
pub mod room_knowledge;
pub mod room_state;
pub mod run_state;
pub mod scheduler;
pub mod skills;
pub mod telegram;
pub mod tools;
pub mod unsafe_audit;
pub mod vault_rotation;
pub mod wake;

use crate::state::AppState;
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Default, Clone)]
pub struct TurnStats {
    pub tool_success: Vec<String>,
    pub tool_errors: Vec<String>,
    pub evidence: Vec<Value>,
}

/// Runs a turn and collects tool evidence for the goal harness.
pub async fn run_quiet_outcome(
    state: &AppState,
    request: &chat::TurnRequest,
) -> (Result<Option<Value>, String>, TurnStats) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let collector = tokio::spawn(async move {
        let mut stats = TurnStats::default();
        while let Some(event) = receiver.recv().await {
            let name = event
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            match event.get("type").and_then(|value| value.as_str()) {
                Some("tool_end") if !name.is_empty() => {
                    stats.tool_success.push(name.to_string());
                    stats.evidence.push(serde_json::json!({
                        "tool": name,
                        "callId": event.get("id"),
                        "arguments": event.get("arguments"),
                        "result": event.get("result"),
                    }));
                }
                Some("tool_error") if !name.is_empty() => stats.tool_errors.push(name.to_string()),
                _ => {}
            }
        }
        stats
    });
    let result = chat::run_turn_background(state, request, &sender).await;
    drop(sender);
    let stats = collector.await.unwrap_or_default();
    (result, stats)
}

/// Background resume that never records a second owner message.
pub async fn run_quiet_resume(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let drain = tokio::spawn(async move {
        while receiver.recv().await.is_some() {}
    });
    let result = chat::run_turn_resume(state, request, &sender).await;
    drop(sender);
    let _ = drain.await;
    result
}

/// Compatibility wrapper; harnesses that need evidence after a model/runtime
/// error use run_quiet_outcome instead of discarding completed tool work.
pub async fn run_quiet_with_stats(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<(Option<Value>, TurnStats), String> {
    let (result, stats) = run_quiet_outcome(state, request).await;
    result.map(|message| (message, stats))
}

/// Runs a turn with nobody listening on the event stream (background jobs).
/// The channel stays open so tool execution is not treated as cancelled.
pub async fn run_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    run_quiet_with_stats(state, request)
        .await
        .map(|(message, _)| message)
}

/// A room worker owns generation and persistence independently of open tabs.
/// Forward transient progress to connected UIs; the stored message is truth.
pub async fn run_room_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
    source_message_id: &str,
) -> Result<Option<Value>, String> {
    let session_id = request
        .session_id
        .clone()
        .ok_or("그룹 세션이 필요합니다.")?;
    let session = state
        .store
        .get("session", &session_id)
        .ok_or("그룹 세션을 찾을 수 없습니다.")?;
    let room_id = session["roomId"]
        .as_str()
        .ok_or("그룹 대화 연결이 없습니다.")?
        .to_string();
    let agent_id = request.agent_id.clone();
    let turn_id = uuid::Uuid::new_v4().to_string();
    state.bus.emit(serde_json::json!({"type":"room_event","roomId":room_id,"sessionId":session_id,"agentId":agent_id,"turnId":turn_id,"event":{"type":"turn_start","sourceMessageId":source_message_id}}));
    let bus = state.bus.clone();
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let stream_room = room_id.clone();
    let stream_session = session_id.clone();
    let stream_agent = agent_id.clone();
    let stream_turn = turn_id.clone();
    let collector = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            bus.emit(serde_json::json!({"type":"room_event","roomId":stream_room,"sessionId":stream_session,"agentId":stream_agent,"turnId":stream_turn,"event":event}));
        }
    });
    let result = chat::run_room_turn(state, request, &sender, source_message_id).await;
    drop(sender);
    let _ = collector.await;
    let event = match &result {
        Ok(Some(message)) => {
            serde_json::json!({"type":"assistant_saved","messageId":message["id"]})
        }
        Ok(None) => serde_json::json!({"type":"done"}),
        Err(error) => serde_json::json!({"type":"error","message":error,"terminal":true}),
    };
    state.bus.emit(serde_json::json!({"type":"room_event","roomId":room_id,"sessionId":session_id,"agentId":agent_id,"turnId":turn_id,"event":event}));
    result
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    use serde_json::json;
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
}

