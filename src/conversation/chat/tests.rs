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
fn ordinary_chat_never_enables_background_learning_implicitly() {
    let default=TurnRequest::from_value(&json!({"input":"Read the file"})).unwrap();
    assert!(!default.auto_learning);
    let explicit=TurnRequest::from_value(
        &json!({"input":"Remember the verified lesson","autoLearning":true})
    ).unwrap();
    assert!(explicit.auto_learning,"explicit owner learning remains opt-in");
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
    // Chat and Native share a parsed SSE decoder: line endings are normalized
    // to LF, while the complete Korean JSON payload and event order survive.
    assert_eq!(events, vec![
        "data: {\"text\":\"장기기억\"}\n\n",
        "data: [DONE]\n\n",
    ]);
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
    state.store.put("provider",&json!({"id":"p","baseUrl":format!("http://{addr}"),"model":"deepseek-fixture","enabled":true}));
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
    // Windows release CI runs this local HTTP fixture alongside hundreds of
    // filesystem/SQLite tests. A loaded runner can delay task scheduling,
    // even though no external provider is involved. Preserve the full response,
    // image, reasoning and timing assertions below; only relax the harness
    // deadline on Windows (production request timeouts are unchanged).
    let fixture_deadline = if cfg!(windows) { 45 } else { 12 };
    let saved = tokio::time::timeout(
        Duration::from_secs(fixture_deadline), run_turn(&state, &request, &tx))
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

#[test]
fn parallel_reads_preserve_mutation_barriers() {
    let calls = ["workspace_read", "workspace_list", "workspace_write",
                 "workspace_read", "file_read", "shell_exec"]
        .iter()
        .enumerate()
        .map(|(index, name)| ToolCall {
            id: format!("call-{index}"),
            name: (*name).into(),
            arguments: "{}".into(),
        })
        .collect::<Vec<_>>();
    assert_eq!(runtime::adjacent_read_batch_end(&calls, 0, 4), 2);
    assert_eq!(runtime::adjacent_read_batch_end(&calls, 1, 4), 2);
    assert_eq!(runtime::adjacent_read_batch_end(&calls, 2, 4), 3);
    assert_eq!(runtime::adjacent_read_batch_end(&calls, 3, 4), 5);
    assert_eq!(runtime::adjacent_read_batch_end(&calls, 5, 4), 6);
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
    assert!(!crate::engine::verification::check_acceptance(&state, "memo.txt 파일 만들어줘", &checks, &read_only).is_empty());
    assert!(!crate::engine::verification::check_acceptance(&state, "memo.txt 파일 만들어줘", &json!([]), &evidence).is_empty());
    let _ = std::fs::remove_dir_all(&state.config.data_dir);
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
        "input":"현재 상황을 설명해줘","autoLearning":false})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let saved = tokio::time::timeout(Duration::from_secs(5), run_turn(&state, &request, &tx))
        .await.unwrap().unwrap().unwrap();
    assert_eq!(saved["metadata"]["runtime"], "dominion-agent-sdk");
    let execution = state.store.all("execution").pop().unwrap();
    assert_eq!(execution["kernelState"]["status"], "completed");
    assert_eq!(execution["kernelState"]["receipts"].as_array().unwrap().len(), 2);
    assert_eq!(execution["budget"]["tools"], 2);
    assert!(execution["kernelState"]["inFlight"].as_array().unwrap().is_empty());
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



#[tokio::test]
async fn chat_progressively_discovers_and_executes_hidden_read_tool() {
    use axum::{extract::State, routing::post, Json, Router};
    use axum::response::IntoResponse;
    use std::sync::{Arc,Mutex};
    let requests=Arc::new(Mutex::new(Vec::<Value>::new()));
    async fn model(
        State(requests):State<Arc<Mutex<Vec<Value>>>>,
        Json(body):Json<Value>
    )->axum::response::Response {
        let step={
            let mut recorded=requests.lock().unwrap();
            recorded.push(body);
            recorded.len()
        };
        let (delta,reason)=match step {
            1 => (json!({"tool_calls":[{
                "index":0,"id":"search-1","type":"function",
                "function":{"name":"dominion_tool_search",
                "arguments":json!({"query":"workspace read range","limit":8}).to_string()}
            }]}),"tool_calls"),
            2 => (json!({"tool_calls":[{
                "index":0,"id":"read-1","type":"function",
                "function":{"name":"dominion_tool_call",
                "arguments":json!({"name":"workspace_read_range",
                    "arguments":{"path":"a.txt","startLine":1}}).to_string()}
            }]}),"tool_calls"),
            _ => (json!({"content":"The first line was read using the discovered range tool."}),"stop"),
        };
        let response=format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":delta}]}),
            json!({"choices":[{"delta":{},"finish_reason":reason}],
                "usage":{"prompt_tokens":20,"completion_tokens":12,"total_tokens":32}})
        );
        ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],response).into_response()
    }
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr=listener.local_addr().unwrap();
    let app=Router::new().route("/chat/completions",post(model)).with_state(requests.clone());
    let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
    let state=temp_state();
    std::fs::write(state.config.workspace.join("a.txt"),"alpha from range\nbeta").unwrap();
    state.store.put("provider",&json!({
        "id":"discovery-provider","kind":"openai","enabled":true,
        "baseUrl":format!("http://{addr}"),"model":"mock-discovery"
    })).unwrap();
    state.store.put("session",&json!({
        "id":"discovery-session","providerId":"discovery-provider"
    })).unwrap();
    crate::engine::algorithms::seed(&state);
    let core=state.store.all("algorithm").into_iter().find(|a|
        a["kind"]=="tool.discovery"&&a["implementation"]=="core").unwrap();
    crate::engine::algorithms::activate(&state,core["id"].as_str().unwrap()).unwrap();
    let request=TurnRequest::from_value(&json!({
        "sessionId":"discovery-session",
        "input":"Inspect the first line in a.txt with a range-read tool",
        "autoLearning":false
    })).unwrap();
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
    let result=tokio::time::timeout(Duration::from_secs(12),
        run_turn(&state,&request,&tx)).await.unwrap().unwrap().unwrap();
    assert!(result["content"].as_str().unwrap_or("").contains("first line"),"{result}");
    let captured=requests.lock().unwrap();
    assert_eq!(captured.len(),3,"tool discovery must complete in three model turns");
    let first_tool_names=captured[0]["tools"].as_array().unwrap().iter()
        .filter_map(|t|t["function"]["name"].as_str())
        .collect::<Vec<_>>();
    assert!(first_tool_names.contains(&"dominion_tool_search"));
    assert!(first_tool_names.contains(&"dominion_tool_call"));
    assert!(!first_tool_names.contains(&"workspace_read_range"));
    let search_output=captured[1]["messages"].as_array().unwrap().iter()
        .filter(|m|m["role"]=="tool")
        .filter_map(|m|m["content"].as_str())
        .any(|text|text.contains("workspace_read_range"));
    assert!(search_output,"search must disclose the actual callable schema");
    let real_observation=captured[2]["messages"].as_array().unwrap().iter()
        .filter(|m|m["role"]=="tool")
        .filter_map(|m|m["content"].as_str())
        .any(|text|text.contains("alpha from range"));
    assert!(real_observation,"the discovered tool must execute and yield real file evidence");
    let ops=state.store.all("execution_operation");
    assert!(ops.iter().any(|op|op["tool"]=="dominion_tool_search"&&op["status"]=="completed"));
    assert!(ops.iter().any(|op|op["tool"]=="workspace_read_range"&&op["status"]=="completed"));
    assert!(!ops.iter().any(|op|op["tool"]=="dominion_tool_call"));
    drop(captured);
    server.abort();
    let _=std::fs::remove_dir_all(&state.config.data_dir);
}


#[tokio::test]
async fn room_model_call_records_component_breakdown_and_exact_provider_usage() {
    use axum::{extract::State, routing::post, Json, Router};
    use axum::response::IntoResponse;
    use std::sync::{Arc,Mutex};
    let posted=Arc::new(Mutex::new(Vec::<Value>::new()));
    async fn model(State(posted):State<Arc<Mutex<Vec<Value>>>>,
        Json(body):Json<Value>)->axum::response::Response {
        posted.lock().unwrap().push(body);
        let response=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"index":0,"delta":{"content":"발언 완료"}}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":410,"completion_tokens":23,
                    "total_tokens":433,"prompt_tokens_details":{"cached_tokens":110}}}));
        ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],response).into_response()
    }
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr=listener.local_addr().unwrap();
    let app=Router::new().route("/chat/completions",post(model))
        .with_state(posted.clone());
    let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
    let state=temp_state();
    state.store.put("provider",&json!({"id":"room-provider","kind":"openai",
        "model":"room-fixture","baseUrl":format!("http://{addr}"),
        "enabled":true})).unwrap();
    state.store.put("agent",&json!({"id":"speaker","name":"테스터",
        "systemPrompt":"참가자 고유 지침은 보존한다"})).unwrap();
    state.store.put("room",&json!({"id":"room-context",
        "agentIds":["speaker"],"contextBudgetTokens":1,"toolsEnabled":false})).unwrap();
    state.store.put("session",&json!({"id":"room:context",
        "roomId":"room-context","providerId":"room-provider",
        "agentId":"speaker"})).unwrap();
    state.store.write_message(&json!({"id":"source-room",
        "session_id":"room:context","role":"user","agent_id":null,
        "content":"이전 대화를 이어 한 문장으로 말해줘",
        "metadata":{"authorKind":"owner"}})).unwrap();
    let request=TurnRequest::from_value(&json!({
        "sessionId":"room:context","agentId":"speaker",
        "providerId":"room-provider","input":"이전 대화를 이어 한 문장으로 말해줘",
        "toolsEnabled":false,"toolsExecutionEnabled":false,
        "autoLearning":false
    })).unwrap();
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
    let saved=tokio::time::timeout(Duration::from_secs(10),
        run_room_turn(&state,&request,&tx,"source-room"))
        .await.unwrap().unwrap().unwrap();
    assert_eq!(saved["content"],"발언 완료");
    assert_eq!(posted.lock().unwrap().len(),1);
    let execution=state.store.all("execution").into_iter().find(|execution|
        execution["sessionId"]=="room:context").unwrap();
    assert_eq!(execution["contextPolicy"],"room-required-evidence-over-soft-target");
    let breakdown=&execution["inputContext"]["modelCallComponents"][0];
    assert_eq!(breakdown["round"],1);
    assert_eq!(breakdown["providerPromptTokens"],410);
    assert_eq!(breakdown["providerOutputTokens"],23);
    assert_eq!(breakdown["providerCacheReadTokens"],110);
    assert_eq!(breakdown["providerUsageExact"],true);
    assert!(breakdown["prepared"]["systemTokens"].as_u64().unwrap()>0);
    assert!(breakdown["prepared"]["sharedRoomStateTokens"].as_u64().unwrap()>0);
    assert!(breakdown["prepared"]["ownerTurnTokens"].as_u64().unwrap()>0);
    assert!(breakdown["prepared"]["estimatedInputTokens"].as_u64().unwrap()>0);
    let serialized=breakdown.to_string();
    assert!(!serialized.contains("이전 대화를 이어 한 문장으로 말해줘"),
        "private owner instruction must not enter the token accounting ledger");
    assert!(!serialized.contains("참가자 고유 지침은 보존한다"));
    server.abort();
    crate::test_support::cleanup_dir(&state.config.data_dir);
}


#[tokio::test]
async fn cancel_api_interrupts_silent_http_generation_and_allows_same_session_retry() {
    use axum::{body::{Body,Bytes},extract::{Path,State},routing::post,Router};
    use axum::http::{header::CONTENT_TYPE,Response};
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    use std::convert::Infallible;
    use futures_util::stream;
    let requests=Arc::new(AtomicUsize::new(0));
    async fn model(State(requests):State<Arc<AtomicUsize>>)->Response<Body>{
        let step=requests.fetch_add(1,Ordering::SeqCst);
        if step==0 {
            // Send HTTP headers but no SSE data. No token event will wake a
            // read loop; owner cancellation must preempt the pending socket.
            Response::builder().header(CONTENT_TYPE,"text/event-stream")
                .body(Body::from_stream(stream::pending::<Result<Bytes,Infallible>>()))
                .unwrap()
        }else {
            let s=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"index":0,"delta":{"content":"SECOND_RUN_OK"}}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}));
            Response::builder().header(CONTENT_TYPE,"text/event-stream")
                .body(Body::from(s)).unwrap()
        }
    }
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr=listener.local_addr().unwrap();
    let app=Router::new().route("/chat/completions",post(model))
        .with_state(requests.clone());
    let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
    let state=temp_state();
    state.store.put("provider",&json!({"id":"cancel-model","kind":"openai",
        "model":"cancel-model","baseUrl":format!("http://{addr}"),"enabled":true})).unwrap();
    state.store.put("session",&json!({"id":"cancel-ui","providerId":"cancel-model"})).unwrap();
    let input=json!({"sessionId":"cancel-ui","providerId":"cancel-model",
        "input":"Read this request","toolsEnabled":false,
        "toolsExecutionEnabled":false,"autoLearning":false});
    let request=TurnRequest::from_value(&input).unwrap();
    let model_state=state.clone();
    let task_request=request.clone();
    let (tx,_rx)=tokio::sync::mpsc::unbounded_channel();
    let task=tokio::spawn(async move {
        run_turn(&model_state,&task_request,&tx).await
    });
    tokio::time::timeout(Duration::from_secs(8),async {
        while requests.load(Ordering::SeqCst)==0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("The real HTTP request must have reached the stalled provider");
    let running=state.store.all("execution").into_iter().find(|e|
        e["sessionId"]=="cancel-ui"&&e["status"]=="running").unwrap();
    let first_id=running["id"].as_str().unwrap().to_owned();

    let response=crate::api::sessions::cancel_session(
        State(state.clone()),Path("cancel-ui".into())).await;
    assert_eq!(response.0["cancelled"],true);
    assert_eq!(response.0["settled"],true,
        "Cancellation endpoint must not release UI before the Rust run finalizes");
    let result=tokio::time::timeout(Duration::from_secs(2),task).await
        .expect("Cancelling must preempt a provider that never sends SSE")
        .unwrap();
    assert!(result.is_err(),"A cancelled turn cannot report a saved answer");
    let cancelled=state.store.get("execution",&first_id).unwrap();
    // The provider accepted the HTTP request but never supplied a completed
    // event. Owner cancellation preempts the socket, not the unknown paid
    // generation: retain its durable fence for manual reconciliation.
    assert_eq!(cancelled["status"],"needs_review");
    assert_eq!(cancelled["phase"],"needs_review");
    assert_eq!(cancelled["ownerCancellationRequested"],true);
    assert_eq!(cancelled["kernelState"]["modelInFlight"],true);
    assert!(!state.store.messages("cancel-ui").iter().any(|m|
        m["role"]=="assistant"));
    assert!(!state.store.all("execution_operation").iter().any(|op|
        op["runId"]==first_id && op["status"]=="completed"));
    // An explicit NEW owner turn may proceed without automatically replaying
    // the previous uncertain generation. The original review fence is immutable.
    let (second_tx,_second_rx)=tokio::sync::mpsc::unbounded_channel();
    let second=tokio::time::timeout(Duration::from_secs(8),
        run_turn(&state,&request,&second_tx)).await.unwrap().unwrap().unwrap();
    assert_eq!(second["content"],"SECOND_RUN_OK");
    assert_eq!(requests.load(Ordering::SeqCst),2);
    assert_eq!(state.store.get("execution",&first_id).unwrap()["status"],"needs_review");
    assert_eq!(state.store.messages("cancel-ui").iter().filter(|m|
        m["role"]=="assistant").count(),1);
    server.abort();
    crate::test_support::cleanup_dir(&state.config.data_dir);
}

#[tokio::test]
async fn ordinary_chat_cannot_append_to_channel_bound_transcript() {
    let state = temp_state();
    for (id, binding) in [
        ("bound-room", json!({"roomId":"r","kind":"room"})),
        ("bound-channel", json!({"channelContext":{"kind":"discord","channelId":"ch"}})),
        ("legacy-room", json!({"kind":"room"})),
    ] {
        let mut record = binding;
        record["id"] = json!(id);
        state.store.put("session", &record).unwrap();
        let request = TurnRequest::from_value(&json!({
            "sessionId": id, "input": "일반 대화로 외부 기록을 덮어쓰면 안 됩니다"
        })).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = run_turn(&state, &request, &tx).await;
        assert!(result.is_err(), "owner Chat must reject {id}");
        assert!(result.unwrap_err().contains("그룹·외부 채널"));
        assert!(state.store.messages(id).is_empty());
    }
}
