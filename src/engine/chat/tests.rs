//! Contract tests exercise the public turn path and local provider fixtures.
use super::{
    context::history,
    protocol::SseBuffer,
    provider::{apply_reasoning_options, select_provider},
    runtime::{finalize_execution, tool_succeeded},
};
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

use super::*;

fn temp_state() -> AppState {
    let root = std::env::temp_dir().join(format!("dominion-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let config = crate::config::Config {
        port: 0,
        data_dir: root.clone(),
        web_dir: root.join("web"),
        workspace: root,
    };
    let store = crate::db::Store::open(&config.db_path()).unwrap();
    let vault = crate::vault::Vault::open(&config.key_path());
    AppState::new(config, store, vault)
}

#[test]
fn post_turn_learning_requires_explicit_opt_in() {
    let ordinary = TurnRequest::from_value(&json!({"input":"test"})).unwrap();
    assert!(!ordinary.auto_learning);
    let enabled = TurnRequest::from_value(&json!({"input":"test","autoLearning":true})).unwrap();
    assert!(enabled.auto_learning);
}

#[test]
fn split_korean_utf8_and_crlf_events_are_lossless() {
    let bytes = "data: {\"text\":\"장기기억\"}\r\n\r\ndata: [DONE]\n\n".as_bytes();
    let mut buffer = SseBuffer::default();
    let mut events = Vec::new();
    for byte in bytes {
        buffer.push(&[*byte]).unwrap();
        while let Some(event) = buffer.pop().unwrap() {
            events.push(event);
        }
    }
    assert_eq!(
        events,
        vec!["data: {\"text\":\"장기기억\"}\r\n\r\n", "data: [DONE]\n\n"]
    );
}

#[test]
fn deepseek_controls_use_its_actual_protocol_and_never_send_none_effort() {
    for (thinking, effort, expected) in [
        (Some(false), Some("high"), "disabled"),
        (None, Some("none"), "disabled"),
        (Some(true), Some("minimal"), "enabled"),
    ] {
        let mut body = json!({});
        apply_reasoning_options(&mut body, "deepseek", thinking, effort);
        assert_eq!(body["thinking"]["type"], expected);
        if expected == "disabled" {
            assert!(body.get("reasoning_effort").is_none());
        } else {
            assert_eq!(body["reasoning_effort"], "low");
        }
        assert!(body.get("chat_template_kwargs").is_none());
    }
    let mut body = json!({});
    apply_reasoning_options(&mut body, "deepseek", Some(true), Some("medium"));
    assert_eq!(body["reasoning_effort"], "high");
}

#[tokio::test]
async fn real_chat_request_has_bound_image_common_prompt_native_reasoning_and_timing() {
    use axum::{extract::State, routing::post, Json, Router};
    use base64::Engine;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    async fn model(
        State(captured): State<std::sync::Arc<std::sync::Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        captured.lock().unwrap().push(body);
        use axum::response::IntoResponse;
        let events = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"content":"fixture-answer","reasoning_content":"This is the original provider reasoning in English, retained without a hidden translation request."}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"total_tokens":17}})
        );
        (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            events,
        )
            .into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/chat/completions", post(model))
        .with_state(captured.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let state = temp_state();
    state.store.put("provider",&json!({"id":"p","baseUrl":format!("http://{addr}"),"model":"deepseek-fixture","enabled":true,
        "capabilities":{"vision":true}}));
    state
        .store
        .put("session", &json!({"id":"s","providerId":"p"}));
    state.store.put(
        "assistant_settings",
        &json!({"id":"global","instructions":"공통 지침 fixture-global","enabled":true}),
    );
    let file=crate::engine::attachments::upload(&state,&json!({"sessionId":"s","name":"photo.png","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture")})).unwrap();
    let request=TurnRequest::from_value(&json!({"sessionId":"s","input":"사진을 확인해","attachmentIds":[file["id"]],"toolsEnabled":false,"thinking":false,"reasoningEffort":"none","reasoningLanguage":"ko","autoLearning":false})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let saved = tokio::time::timeout(Duration::from_secs(20), run_turn(&state, &request, &tx))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(saved["reasoning"]
        .as_str()
        .unwrap()
        .contains("original provider reasoning"));
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let body = &requests[0];
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("reasoning_effort").is_none());
    assert!(body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("fixture-global"));
    let owner = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "user")
        .unwrap();
    assert_eq!(owner["content"][1]["type"], "image_url");
    assert!(owner["content"][1]["image_url"]["url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    let user = state
        .store
        .messages("s")
        .into_iter()
        .find(|message| message["role"] == "user")
        .unwrap();
    assert_eq!(user["metadata"]["attachments"][0]["id"], file["id"]);
    let execution = state.store.all("execution").pop().unwrap();
    assert_eq!(execution["tokens"], 17);
    assert!(execution["firstTokenMs"].is_u64());
    assert!(execution["modelElapsedMs"].is_u64());
    server.abort();
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn failed_commands_signals_and_mcp_errors_are_not_successes() {
    assert!(!tool_succeeded(
        &json!({"exitCode":7,"stdout":"claimed success"})
    ));
    assert!(!tool_succeeded(&json!({"exitCode":null})));
    assert!(!tool_succeeded(&json!({"isError":true,"content":[]})));
    assert!(!tool_succeeded(&json!({"status":403})));
    assert!(tool_succeeded(&json!({"exitCode":0})));
}

#[test]
fn cancelled_execution_finalizes_unknown_tool_outcomes() {
    let state = temp_state();
    state
        .store
        .put("execution", &json!({"id":"e","status":"running"}));
    state.store.put(
        "execution_operation",
        &json!({"id":"op","runId":"e","status":"running","tool":"shell_exec"}),
    );
    finalize_execution(
        &state,
        "e",
        &Err("실행이 중단되었습니다.".into()),
        &std::sync::Mutex::new(Metrics::default()),
    );
    assert_eq!(
        state.store.get("execution", "e").unwrap()["status"],
        "cancelled"
    );
    let operation = state.store.get("execution_operation", "op").unwrap();
    assert_eq!(operation["status"], "interrupted");
    assert_eq!(operation["outcomeUnknown"], true);
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn continuation_keeps_the_selected_model_only_for_the_same_provider_and_agent() {
    let state = temp_state();
    for id in ["p", "other"] {
        state.store.put("provider",&json!({"id":id,"baseUrl":"http://127.0.0.1:1/v1","model":"provider-default","enabled":true}));
    }
    let session = json!({"providerId":"p","model":"my-model","agentId":null});
    let mut request = TurnRequest::from_value(&json!({"input":"continue"})).unwrap();
    assert_eq!(
        select_provider(&state, &request, &session).unwrap().model,
        "my-model"
    );
    request.provider_id = Some("other".into());
    assert_eq!(
        select_provider(&state, &request, &session).unwrap().model,
        "provider-default"
    );
    request.model = Some("explicit".into());
    assert_eq!(
        select_provider(&state, &request, &session).unwrap().model,
        "explicit"
    );
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn room_history_keeps_speakers_distinct_and_drops_orphan_tool_results() {
    let state = temp_state();
    state.store.put("agent", &json!({"id":"a","name":"설계자"}));
    state.store.put("agent", &json!({"id":"b","name":"검토자"}));
    for message in [
        json!({"role":"tool","agent_id":"b","tool_call_id":"orphan","content":"orphan secret"}),
        json!({"role":"assistant","agent_id":"a","content":"설계 의견"}),
        json!({"role":"assistant","agent_id":"b","content":"검토 의견"}),
        json!({"role":"assistant","agent_id":"b","content":"검증하겠습니다","tool_calls":[{"id":"c1","type":"function","function":{"name":"workspace_list","arguments":"{}"}}]}),
        json!({"role":"tool","agent_id":"b","content":"{}","tool_call_id":"c1"}),
        json!({"role":"assistant","agent_id":"b","content":"중단됨","tool_calls":[{"id":"missing"}]}),
    ] {
        let mut message = message;
        message["session_id"] = json!("room");
        state.store.write_message(&message).unwrap();
    }
    let messages = history(&state, "room", Some("b"), None).unwrap();
    assert_eq!(messages[0]["role"], "user");
    assert!(messages[0]["content"].as_str().unwrap().contains("설계자"));
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(
        messages.iter().filter(|row| row["role"] == "tool").count(),
        1
    );
    assert!(messages.last().unwrap().get("tool_calls").is_none());
    assert!(!json!(messages).to_string().contains("orphan secret"));
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn parses_xml_style_tool_calls() {
    let (calls, cleaned) = parse_text_tool_calls(
            "확인하겠습니다.\n<tool_call>{\"name\":\"workspace_read\",\"arguments\":{\"path\":\"a.txt\"}}</tool_call>",
        );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "workspace_read");
    assert!(calls[0].1.contains("a.txt"));
    assert!(!cleaned.contains("tool_call"));
}

#[test]
fn parses_fenced_json_tool_calls() {
    let (calls, _) = parse_text_tool_calls(
        "```json\n{\"tool\":\"shell_exec\",\"args\":{\"command\":\"ls\"}}\n```",
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "shell_exec");
}

#[test]
fn ignores_normal_text() {
    let (calls, cleaned) = parse_text_tool_calls("그냥 일반 답변입니다.");
    assert!(calls.is_empty());
    assert_eq!(cleaned, "그냥 일반 답변입니다.");
}

#[test]
fn parses_dsml_bash_call_and_strips_it() {
    let text = "<｜｜DSML｜｜ calls> <｜｜DSML｜｜ invoke name=\"bash\"> <｜｜DSML｜｜ parameter name=\"command\"> ls -la </｜｜DSML｜｜ parameter> </｜｜DSML｜｜ invoke> </｜｜DSML｜｜ calls>";
    let known = std::collections::HashSet::from(["shell_exec".to_string()]);
    let (calls, cleaned) = super::protocol::parse_text_tool_calls_known(text, &known);
    assert_eq!(calls.len(), 1, "{calls:?} {cleaned:?}");
    assert_eq!(calls[0].0, "shell_exec");
    assert!(calls[0].1.contains("ls -la"), "{calls:?}");
    assert!(!cleaned.contains("invoke"), "{cleaned}");
    assert!(!cleaned.contains("bash"), "{cleaned}");
}

#[test]
fn unknown_dsml_tool_is_stripped_not_executed() {
    let text = "<｜｜DSML｜｜ calls> <｜｜DSML｜｜ invoke name=\"unknown_tool\"> <｜｜DSML｜｜ parameter name=\"x\"> y </｜｜DSML｜｜ parameter> </｜｜DSML｜｜ invoke> </｜｜DSML｜｜ calls>";
    let known = std::collections::HashSet::from(["shell_exec".to_string()]);
    let (calls, cleaned) = super::protocol::parse_text_tool_calls_known(text, &known);
    assert!(calls.is_empty(), "{calls:?}");
    assert!(!cleaned.contains("invoke"), "{cleaned}");
    assert!(!cleaned.contains("unknown_tool"), "{cleaned}");
}

#[test]
fn concurrent_tool_evidence_statistics_do_not_lose_worker_updates() {
    let state = temp_state();
    std::thread::scope(|scope| {
        for index in 0..8 {
            let store = state.store.clone();
            scope.spawn(move || {
                for _ in 0..50 {
                    store
                        .increment_counters(
                            "tool_stat",
                            "fixture_tool",
                            &[
                                "attempts",
                                if index % 2 == 0 {
                                    "successes"
                                } else {
                                    "failures"
                                },
                            ],
                        )
                        .unwrap();
                }
            });
        }
    });
    let stat = state.store.get("tool_stat", "fixture_tool").unwrap();
    assert_eq!(stat["attempts"], 400);
    assert_eq!(stat["successes"], 200);
    assert_eq!(stat["failures"], 200);
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn parses_bare_json_and_stringified_arguments_for_small_models() {
    let known = ["workspace_write".to_string()]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let (calls, cleaned) = super::protocol::parse_text_tool_calls_known(
        "{\"tool\":\"workspace_write\",\"args\":{\"path\":\"a.txt\",\"content\":\"x\"}}",
        &known,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "workspace_write");
    assert!(calls[0].1.contains("a.txt"));
    assert!(cleaned.is_empty());

    let (calls, _) = super::protocol::parse_text_tool_calls_known(
        "{\"name\":\"workspace_write\",\"arguments\":\"{\\\"path\\\":\\\"b.txt\\\",\\\"content\\\":\\\"y\\\"}\"}",
        &known,
    );
    assert_eq!(calls.len(), 1);
    assert!(calls[0].1.contains("b.txt"));
}

#[test]
fn parses_function_wrapper_and_missing_closing_tag() {
    let known = ["workspace_read".to_string()]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let (calls, _) = super::protocol::parse_text_tool_calls_known(
        "<tool_call>{\"function\":{\"name\":\"workspace_read\",\"arguments\":{\"path\":\"a.txt\"}}}",
        &known,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "workspace_read");
    assert!(calls[0].1.contains("a.txt"));
}

#[test]
fn durable_history_restores_out_of_order_results_by_call_identity() {
    let state = temp_state();
    let transcript = [
        json!({"role":"assistant","content":"","tool_calls":[
            {"id":"a","type":"function","function":{"name":"workspace_read","arguments":"{}"}},
            {"id":"b","type":"function","function":{"name":"workspace_read","arguments":"{}"}}
        ]}),
        json!({"role":"tool","tool_call_id":"b","content":"second observation"}),
        json!({"role":"tool","tool_call_id":"a","content":"first observation"}),
    ];
    for mut message in transcript {
        message["session_id"] = json!("out-of-order");
        state.store.write_message(&message).unwrap();
    }
    let restored = history(&state, "out-of-order", None, None).unwrap();
    assert_eq!(restored.len(), 3);
    assert_eq!(restored[1]["tool_call_id"], "a");
    assert_eq!(restored[1]["content"], "first observation");
    assert_eq!(restored[2]["tool_call_id"], "b");
    assert_eq!(restored[2]["content"], "second observation");
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[tokio::test]
async fn stop_before_native_turn_starts_does_not_clear_newer_cancellation() {
    let state=temp_state();
    state.store.put("session",&json!({"id":"prestop"})).unwrap();
    let admitted=state.cancellation_generation("prestop");
    state.cancel_session("prestop");
    let request=TurnRequest::from_value(&json!({
        "sessionId":"prestop","input":"this turn must never start",
        "toolsEnabled":false,"autoLearning":false
    })).unwrap();
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
    let outcome=super::runtime::run_turn_http(&state,&request,&tx,Some(admitted)).await;
    assert!(outcome.unwrap_err().contains("중단"));
    assert!(state.is_cancelled("prestop"));
    assert!(state.store.all("execution").is_empty(),
        "a cancelled-before-start turn cannot acquire a model execution");
    assert!(state.store.messages("prestop").is_empty());
    let _=std::fs::remove_dir_all(&state.config.data_dir);
}

#[tokio::test]
async fn user_stop_preempts_silent_model_stream_and_next_turn_succeeds() {
    use axum::{extract::State, response::{IntoResponse,Response,sse::{Event,Sse}}, routing::post,
        Router};
    use std::{convert::Infallible,sync::{Arc,atomic::{AtomicUsize,Ordering}}};
    async fn model(State(count):State<Arc<AtomicUsize>>)->Response {
        let attempt=count.fetch_add(1,Ordering::SeqCst);
        if attempt==0 {
            // 200 OK and an SSE body, but no token, no heartbeat, no DONE.
            // Only cancellation can release this provider request.
            return Sse::new(futures_util::stream::pending::<Result<Event,Infallible>>())
                .into_response();
        }
        let bytes=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"content":"new turn works"}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14}}));
        ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],bytes).into_response()
    }
    let counter=Arc::new(AtomicUsize::new(0));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let app=Router::new().route("/chat/completions",post(model))
        .with_state(counter.clone());
    let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
    let state=temp_state();
    state.store.put("provider",&json!({"id":"silent-model",
        "kind":"openai","baseUrl":format!("http://{address}"),
        "model":"silent-test","enabled":true})).unwrap();
    state.store.put("session",&json!({"id":"stop-test","providerId":"silent-model"})).unwrap();
    let request=TurnRequest::from_value(&json!({"sessionId":"stop-test",
        "input":"Say something","providerId":"silent-model",
        "toolsEnabled":false,"toolsExecutionEnabled":false,"autoLearning":false,
        "thinking":false})).unwrap();
    let admitted=state.cancellation_generation("stop-test");
    let run_state=state.clone();let run_request=request.clone();
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
    let task=tokio::spawn(async move{
        super::runtime::run_turn_http(&run_state,&run_request,&tx,Some(admitted)).await
    });
    tokio::time::timeout(Duration::from_secs(4),async{
        while counter.load(Ordering::SeqCst)<1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("the model must be reached before cancellation");
    let started=std::time::Instant::now();
    let ack=crate::api::sessions::cancel_session(
        axum::extract::State(state.clone()),
        axum::extract::Path("stop-test".to_string()),
    ).await.0;
    assert_eq!(ack["stopped"],true,"cancel must confirm durable turn teardown: {ack}");
    assert!(started.elapsed()<Duration::from_secs(3),"silent request must be preempted");
    assert!(task.await.unwrap().unwrap_err().contains("중단"));
    let cancelled=state.store.all("execution");
    assert_eq!(cancelled.len(),1);
    assert_eq!(cancelled[0]["status"],"cancelled");
    assert!(state.store.messages("stop-test").iter().all(|m|m["role"]!="assistant"),
        "a later provider completion must never save an assistant answer");

    // Same session can be resumed without carrying a stale cancellation mark.
    let (tx2,_rx2)=tokio::sync::mpsc::unbounded_channel();
    let next=tokio::time::timeout(Duration::from_secs(4),
        run_turn(&state,&request,&tx2)).await.unwrap().unwrap().unwrap();
    assert_eq!(next["content"],"new turn works");
    assert_eq!(counter.load(Ordering::SeqCst),2);
    assert_eq!(state.store.all("execution").iter()
        .filter(|e|e["status"]=="cancelled").count(),1);
    server.abort();
    let _=std::fs::remove_dir_all(&state.config.data_dir);
}

#[tokio::test]
async fn ordinary_chat_runs_native_tools_through_the_imported_rust_kernel() {
    use axum::{extract::State, routing::post, Json, Router};
    use axum::response::IntoResponse;
    use std::sync::{Arc, Mutex};
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    async fn model(State(requests): State<Arc<Mutex<Vec<Value>>>>, Json(body): Json<Value>)
        -> axum::response::Response {
        let first = {
            let mut captured = requests.lock().unwrap();
            captured.push(body);
            captured.len() == 1
        };
        let delta = if first { json!({"tool_calls":[
            {"index":0,"id":"call-a","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"a.txt\"}"}},
            {"index":1,"id":"call-b","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"b.txt\"}"}}
        ]}) } else { json!({"content":"두 파일의 실제 내용을 확인했습니다."}) };
        let response = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":delta}]}),
            json!({"choices":[{"delta":{},"finish_reason":if first {"tool_calls"} else {"stop"}}],
                "usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}));
        ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],response).into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().route("/chat/completions", post(model)).with_state(requests.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let state = temp_state();
    std::fs::write(state.config.workspace.join("a.txt"), "alpha").unwrap();
    std::fs::write(state.config.workspace.join("b.txt"), "beta").unwrap();
    state.store.put("provider", &json!({"id":"kernel-model","baseUrl":format!("http://{address}"),
        "model":"native-tool-fixture","kind":"openai","enabled":true}));
    state.store.put("session", &json!({"id":"kernel-chat","providerId":"kernel-model"}));
    crate::engine::algorithms::seed(&state);
    let parallel = state.store.all("algorithm").into_iter().find(|row|
        row["kind"] == "tool.combination" && row["implementation"] == "parallel-reads").unwrap();
    crate::engine::algorithms::activate(&state, parallel["id"].as_str().unwrap()).unwrap();
    let request = TurnRequest::from_value(&json!({"sessionId":"kernel-chat",
        "input":"a.txt와 b.txt 파일을 읽어서 상황을 설명해줘","autoLearning":false})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let saved = tokio::time::timeout(Duration::from_secs(5), run_turn(&state, &request, &tx))
        .await.unwrap().unwrap().unwrap();
    assert_eq!(saved["metadata"]["runtime"], "dominion-core");
    let execution = state.store.all("execution").pop().unwrap();
    assert_eq!(execution["kernelState"]["native"]["status"], "completed");
    assert_eq!(execution["kernelState"]["receipts"].as_array().unwrap().len(), 2);
    let phases=execution["kernelState"]["native"]["events"].as_array().unwrap().iter()
        .filter(|event|event["kind"]=="phase")
        .filter_map(|event|event["message"].as_str()).collect::<Vec<_>>();
    assert_eq!(phases,["initialize","prepare_model","model","tools","verify_tools",
        "prepare_model","model","verify","complete"]);
    assert_eq!(execution["kernelState"]["modelCalls"],2);
    assert_eq!(execution["kernelState"]["chargedTokens"],40);
    assert_eq!(execution["budget"]["tools"], 2);
    assert!(execution["kernelState"]["native"]["in_flight"].is_null());
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 2);
    let evidence = captured[1]["messages"].as_array().unwrap().iter()
        .filter(|message| message["role"] == "tool").collect::<Vec<_>>();
    assert_eq!(evidence.len(), 2);
    assert!(evidence.iter().any(|message| message["content"].as_str().unwrap().contains("alpha")));
    assert!(evidence.iter().any(|message| message["content"].as_str().unwrap().contains("beta")));
    server.abort();
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}

#[test]
fn korean_turn_infers_reasoning_language_and_respects_explicit_original() {
    let mut request = TurnRequest::from_value(&json!({"input":"바탕화면에 메모 파일 만들어줘"})).unwrap();
    assert_eq!(request.effective_reasoning_language(), Some("ko"));
    request.reasoning_language = Some("original".into());
    assert_eq!(request.effective_reasoning_language(), None);
    request.input = "Create a note".into();
    request.reasoning_language = None;
    assert_eq!(request.effective_reasoning_language(), None);
}

#[test]
fn completed_compound_shell_file_creation_is_accepted_without_another_reviewer() {
    let state = temp_state();
    let checks = crate::engine::verification::prepare_acceptance_checks(&state, "memo.txt 파일 만들어줘");
    assert!(crate::engine::verification::check_acceptance(&state, "memo.txt 파일 만들어줘", &checks, &[]).len() > 0);
    std::fs::write(state.config.workspace.join("memo.txt"), "").unwrap();
    let evidence = vec![json!({"tool":"shell_exec","arguments":{"command":"cd workspace && touch memo.txt && ls -l memo.txt"},
        "status":"completed","result":{"exitCode":0,"stdout":"memo.txt","stderr":""}})];
    assert!(crate::engine::verification::check_acceptance(&state, "memo.txt 파일 만들어줘", &checks, &evidence).is_empty());
    let read_only = vec![json!({"tool":"workspace_list","result":{"entries":["memo.txt"]}})];
    assert!(crate::engine::verification::check_acceptance(&state, "memo.txt 파일 만들어줘", &checks, &read_only).is_empty());
    std::fs::remove_file(state.config.workspace.join("memo.txt")).unwrap();
    assert!(!crate::engine::verification::check_acceptance(&state, "memo.txt 파일 만들어줘", &checks, &read_only).is_empty());
    assert!(!crate::engine::verification::check_acceptance(&state, "memo.txt 파일 만들어줘", &json!([]), &evidence).is_empty());
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
}
