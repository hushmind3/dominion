pub mod effect;
pub mod chat;
pub mod capability;
pub mod verification;
pub mod recovery;
pub mod lease;
pub mod llm;
pub mod policy;
pub mod router;
pub mod run_state;
pub mod tools;

// Backwards-compatible product API: implementation moved to src/addons/.
// Execution correctness remains in the standalone dominion-core crate.
pub use crate::addons::{
    compact, goal_control, obligation_repair, program, reasoning, delegation_worktree, jobs, wake, job_control,
    agent_growth, distill, experience, learning, memory, principle, reflect, room_knowledge, skills, adaptation, adaptation_loop, algorithm_eval, algorithms, debate, decompose, episode, improve, plan_quality, policy_patch, preflight_fixture, procedure, attachments, bridge, browser, channel_adapter, cli_model, code_index, codex_oauth, dev, discord, extensions, gateway, mcp, models, oauth, personas, room_state, rooms, telegram, backup_keys, maintenance, remote_backup, vault_rotation
};

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
    let (sender, receiver) = mpsc::unbounded_channel::<Value>();
    let drain = tokio::spawn(drain_quiet_events(receiver));
    let result = chat::run_turn_resume(state, request, &sender).await;
    drop(sender);
    let _ = drain.await;
    result
}

/// Drain events without materializing transient tool arguments/results in a
/// second TurnStats vector. Only explicit evidence callers use the collector.
async fn drain_quiet_events(mut receiver: mpsc::UnboundedReceiver<Value>) {
    while receiver.recv().await.is_some() {}
}

/// Runs a turn with nobody listening on the event stream (background jobs).
/// The channel stays open so tool execution is not treated as cancelled.
pub async fn run_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    let (sender, receiver) = mpsc::unbounded_channel::<Value>();
    let drain = tokio::spawn(drain_quiet_events(receiver));
    let result = chat::run_turn_background(state, request, &sender).await;
    drop(sender);
    let _ = drain.await;
    result
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
        let error=outcome.unwrap_err();
        assert!(error.contains("503"),"{error}");
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


pub mod execution_budget;

pub mod sandbox;

pub mod workspace_scope;
