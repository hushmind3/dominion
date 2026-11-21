//! Long context, WebSocket/HTTP Codex protocol, file generation acceptance.
use super::*;

fn measure_estimate(text: &str) -> usize {
    let ascii = text.chars().filter(char::is_ascii).count();
    ascii.div_ceil(4) + text.chars().count() - ascii
}
#[test]
fn measure_room_input_growth_across_three_turns() {
    let _serial = serial();
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reply_counter = counter.clone();
    let (port, requests) = scripted_provider(move |_| {
        let n = reply_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        vec![json!({
            "choices":[{"delta":{"content":format!("응답 {n}")},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":100,"completion_tokens":5,"total_tokens":105}
        })]
    });
    let root = temp_root();
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let persona = "지침 문장입니다. ".repeat(220);
    let mut ids = Vec::new();
    for index in 1..=9 {
        let agent = app.post("/api/agents", &json!({
            "id":format!("m{index}"),
            "name":format!("참가자{index}"),
            "systemPrompt":format!("{persona}나는 참가자{index}입니다.")
        }));
        ids.push(agent["id"].as_str().unwrap().to_string());
    }
    app.post("/api/rooms", &json!({
        "id":"measure-room","name":"측정 방","agentIds":ids,
        "routingMode":"discussion","rounds":1,"maxTurns":10,
        "toolsEnabled":false,"autoLearning":false,"contextBudgetTokens":2000
    }));
    let mut seen = 0usize;
    for (round, text) in ["안녕", "오늘 기분은 어때", "점심은 뭐 먹을까"].iter().enumerate() {
        let sent = app.post("/api/rooms/measure-room/messages", &json!({"input":text}));
        let job_id = sent["jobId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let job = app.get(&format!("/api/jobs/{job_id}"));
            if job["status"] == "completed" { break; }
            assert_ne!(job["status"], "failed", "{job}");
            assert!(Instant::now() < deadline, "timeout: {job}");
            std::thread::sleep(Duration::from_millis(30));
        }
        let bodies = requests.lock().unwrap();
        let fresh: Vec<Value> = bodies[seen..].to_vec();
        seen = bodies.len();
        drop(bodies);
        let mut total = 0usize;
        for (index, body) in fresh.iter().enumerate() {
            let tokens = measure_estimate(&body["messages"].to_string());
            total += tokens;
            let prompt = body["messages"][0]["content"].as_str().unwrap_or("");
            let name = prompt.split("활성 에이전트: ").nth(1)
                .and_then(|rest| rest.split(" (id:").next()).unwrap_or("?");
            println!("round {} call {} agent={} estimate={} items={}", round + 1, index + 1, name, tokens, body["messages"].as_array().map(Vec::len).unwrap_or(0));
        }
        println!("round {} summary calls={} total_estimate={}", round + 1, fresh.len(), total);
    }
    let db = rusqlite::Connection::open_with_flags(
        root.join("runtime/dominion.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).unwrap();
    let mut statement = db.prepare("select data from records where kind='participant_conversation'").unwrap();
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
    for row in rows {
        let data: Value = serde_json::from_str(&row.unwrap()).unwrap();
        let view = &data["workingView"];
        let frames = view["frames"].as_array().cloned().unwrap_or_default();
        let mut contents = std::collections::HashMap::new();
        for frame in &frames {
            let text = frame["message"]["content"].as_str().unwrap_or("").to_string();
            if !text.is_empty() { *contents.entry(text).or_insert(0usize) += 1; }
        }
        let duplicates = contents.values().filter(|count| **count > 1).count();
        println!(
            "view agent={} epoch={} frames={} duplicate_contents={} estimate={}",
            data["agentId"].as_str().unwrap_or("?"),
            view["epoch"], frames.len(), duplicates,
            measure_estimate(&json!(frames).to_string())
        );
    }
}


/// Minimal Codex Responses websocket: one chain per connection, exactly like
/// the subscription backend. Records every `response.create` frame it receives
/// so a test can inspect the incremental payload.
fn scripted_websocket_provider() -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let observed = observed.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                        return;
                    };
                    let mut count = 0usize;
                    while let Some(Ok(message)) = socket.next().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(body) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if body["type"] != "response.create" {
                            continue;
                        }
                        count += 1;
                        observed.lock().unwrap().push(body.clone());
                        let id = format!("resp_ws_fixture_{count}");
                        let answer = format!("WEBSOCKET_ANSWER_{count}");
                        let events = [
                            json!({"type":"response.output_text.delta","delta":answer}),
                            json!({"type":"response.completed","response":{"status":"completed",
                                "id":id,
                                "output":[{"type":"message","role":"assistant",
                                    "content":[{"type":"output_text","text":answer}]}],
                                "usage":{"input_tokens":40,
                                    "input_tokens_details":{"cached_tokens":0},
                                    "output_tokens":5,"total_tokens":45}
                            }}),
                        ];
                        for event in events {
                            if socket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                });
            }
        });
    });
    (port, requests)
}

/// Codex Responses websocket that answers the first request with a structured
/// `workspace_write` function call and the continuation with a final answer.
/// The continuation must carry the function-call output, not a text transcript.
fn scripted_codex_tool_provider() -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let observed = observed.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                        return;
                    };
                    let mut count = 0usize;
                    while let Some(Ok(message)) = socket.next().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(body) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if body["type"] != "response.create" {
                            continue;
                        }
                        count += 1;
                        observed.lock().unwrap().push(body.clone());
                        let id = format!("resp_ws_tool_{count}");
                        let has_tool_output = body["input"]
                            .as_array()
                            .is_some_and(|items| items.iter().any(|item| {
                                item["type"] == "function_call_output"
                            }));
                        let events = if !has_tool_output {
                            let call = json!({
                                "type":"function_call",
                                "id":"fc_ws_tool_1",
                                "call_id":"call_ws_tool_1",
                                "name":"workspace_write",
                                "arguments":"{\"path\":\"ws_tool.txt\",\"content\":\"WS_TOOL_OK\"}"
                            });
                            vec![
                                json!({"type":"response.output_item.done","item":call.clone()}),
                                json!({"type":"response.completed","response":{"status":"completed",
                                    "id":id,
                                    "output":[call],
                                    "usage":{"input_tokens":40,
                                        "input_tokens_details":{"cached_tokens":0},
                                        "output_tokens":5,"total_tokens":45}
                                }}),
                            ]
                        } else {
                            let answer = "웹소켓 도구 실행 완료";
                            vec![
                                json!({"type":"response.output_text.delta","delta":answer}),
                                json!({"type":"response.completed","response":{"status":"completed",
                                    "id":id,
                                    "output":[{"type":"message","role":"assistant",
                                        "content":[{"type":"output_text","text":answer}]}],
                                    "usage":{"input_tokens":45,
                                        "input_tokens_details":{"cached_tokens":40},
                                        "output_tokens":5,"total_tokens":50}
                                }}),
                            ]
                        };
                        for event in events {
                            if socket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                });
            }
        });
    });
    (port, requests)
}

#[test]
fn codex_websocket_executes_a_structured_tool_call_and_continues_the_chain() {
    let _serial = serial();
    let (port, requests) = scripted_codex_tool_provider();
    let app = TestApp::new(port, "compat");
    app.post("/api/policy", &json!({"autoApproveAll": true}));
    let provider = app.post("/api/providers", &json!({
        "name":"codex tool fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture-account","apiKey":"fixture-only",
        "enabled":true,"model":"fixture-model"
    }));
    let agent = app.post("/api/agents", &json!({
        "name":"ws tool participant",
        "systemPrompt":"WEBSOCKET_TOOL_IDENTITY stays verbatim",
        "providerId":provider["id"],"model":"fixture-model"
    }));
    let session = app.post("/api/sessions", &json!({"title":"ws tool","agentId":agent["id"]}));
    let stream = app.chat(&json!({
        "sessionId":session["id"],"agentId":agent["id"],
        "providerId":provider["id"],"model":"fixture-model",
        "input":"ws_tool.txt 파일을 만들어줘",
        "toolsEnabled":true,"autoLearning":false
    }));
    assert!(stream.contains("웹소켓 도구 실행 완료"),
        "{stream}; websocket requests: {:?}",requests.lock().unwrap());
    assert_eq!(
        std::fs::read_to_string(app._root.join("ws_tool.txt")).unwrap_or_default(),
        "WS_TOOL_OK"
    );
    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 2, "one tool round plus one final answer: {seen:?}");
    let tools = seen[0]["tools"].as_array().expect("native tools");
    assert!(
        tools.iter().any(|tool| {
            tool["type"] == "function"
                && tool["name"] == "workspace_write"
                && tool["parameters"]["type"] == "object"
        }),
        "workspace_write schema was not sent: {tools:?}"
    );
    assert!(seen[0].get("previous_response_id").is_none());
    // The acceptance gate adds a stricter developer instruction after the
    // write, so the model contract changes and the chain must be rebased.
    assert!(seen[1].get("previous_response_id").is_none());
    let second = seen[1]["input"].as_array().unwrap();
    assert!(
        second.iter().any(|item| {
            item["type"] == "function_call_output"
                && item["call_id"] == "call_ws_tool_1"
                && item["output"]
                    .as_str()
                    .is_some_and(|output| output.contains("ws_tool.txt"))
        }),
        "the continuation must carry the function-call output: {second:?}"
    );
    assert!(
        !second
            .iter()
            .any(|item| item["role"] == "user" && item["content"].to_string().contains("Tool observation")),
        "tool output stayed structured instead of becoming user text: {second:?}"
    );
    assert!(second.iter().any(|item| {
        item["role"] == "user"
            && item["content"].to_string().contains("ws_tool.txt 파일을 만들어줘")
    }), "a rebased request must carry the complete user turn: {second:?}");
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    let assistant = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .find(|row| row["tool_calls"].as_array().is_some_and(|calls| {
            calls.iter().any(|call| call["id"] == "call_ws_tool_1")
        }))
        .expect("persisted structured tool call");
    assert_eq!(
        assistant["tool_calls"][0]["function"]["name"],
        "workspace_write"
    );
}

#[test]
fn codex_http_fallback_executes_a_structured_tool_call() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|body| {
        let has_output = body["input"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["type"] == "function_call_output")
        });
        if has_output {
            return vec![
                json!({"type":"response.output_text.delta","delta":"HTTP 도구 완료"}),
                json!({"type":"response.completed","response":{"status":"completed",
                    "id":"resp_http_tool_2",
                    "output":[{"type":"message","role":"assistant",
                        "content":[{"type":"output_text","text":"HTTP 도구 완료"}]}],
                    "usage":{"input_tokens":45,"output_tokens":5,"total_tokens":50}
                }}),
            ];
        }
        let call = json!({
            "type":"function_call",
            "id":"fc_http_tool_1",
            "call_id":"call_http_tool_1",
            "name":"workspace_write",
            "arguments":"{\"path\":\"http_tool.txt\",\"content\":\"HTTP_TOOL_OK\"}"
        });
        vec![
            json!({"type":"response.output_item.done","item":call.clone()}),
            json!({"type":"response.completed","response":{"status":"completed",
                "id":"resp_http_tool_1",
                "output":[call],
                "usage":{"input_tokens":40,"output_tokens":5,"total_tokens":45}
            }}),
        ]
    });
    let app = TestApp::new_with_env(port, "compat", &[("DOMINION_CODEX_WS", "off")]);
    app.post("/api/policy", &json!({"autoApproveAll": true}));
    let provider = app.post("/api/providers", &json!({
        "name":"codex http tool fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture-account","apiKey":"fixture-only",
        "enabled":true,"model":"fixture-model"
    }));
    let agent = app.post("/api/agents", &json!({
        "name":"http tool participant",
        "systemPrompt":"HTTP_TOOL_IDENTITY stays verbatim",
        "providerId":provider["id"],"model":"fixture-model"
    }));
    let session = app.post("/api/sessions", &json!({"title":"http tool","agentId":agent["id"]}));
    let stream = app.chat(&json!({
        "sessionId":session["id"],"agentId":agent["id"],
        "providerId":provider["id"],"model":"fixture-model",
        "input":"http_tool.txt 파일을 만들어줘",
        "toolsEnabled":true,"autoLearning":false
    }));
    assert!(stream.contains("HTTP 도구 완료"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("http_tool.txt")).unwrap_or_default(),
        "HTTP_TOOL_OK"
    );
    let seen = requests.lock().unwrap();
    assert!(
        seen.iter().any(|body| {
            body["tools"].as_array().is_some_and(|tools| {
                tools.iter().any(|tool| {
                    tool["type"] == "function" && tool["name"] == "workspace_write"
                })
            })
        }),
        "HTTP fallback must send the tool schema: {seen:?}"
    );
    assert!(
        seen.iter().any(|body| {
            body["input"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["type"] == "function_call_output"
                        && item["call_id"] == "call_http_tool_1"
                        && item["output"]
                            .as_str()
                            .is_some_and(|output| output.contains("http_tool.txt"))
                })
            })
        }),
        "HTTP fallback must send the function-call output: {seen:?}"
    );
}

#[test]
fn codex_websocket_chain_reuses_identity_and_sends_only_the_new_suffix() {
    let _serial = serial();
    let (port, requests) = scripted_websocket_provider();
    let app = TestApp::new(port, "compat");
    let provider = app.post("/api/providers", &json!({
        "name":"codex websocket fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture-account","apiKey":"fixture-only",
        "enabled":true,"model":"fixture-model"
    }));
    let agent = app.post("/api/agents", &json!({
        "name":"ws participant",
        "systemPrompt":"WEBSOCKET_IDENTITY_VERBATIM persona stays whole",
        "providerId":provider["id"],"model":"fixture-model"
    }));
    let session = app.post("/api/sessions", &json!({"title":"ws chain","agentId":agent["id"]}));
    for text in ["첫 번째 질문", "두 번째 질문"] {
        let stream = app.chat(&json!({
            "sessionId":session["id"],"agentId":agent["id"],
            "providerId":provider["id"],"model":"fixture-model",
            "input":text,"toolsEnabled":false,"autoLearning":false
        }));
        assert!(stream.contains("WEBSOCKET_ANSWER"), "{stream}");
    }
    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 2, "one websocket request per turn: {seen:?}");
    assert!(seen[0]["prompt_cache_key"].is_string(),"WebSocket must carry the HTTP cache identity");
    assert_eq!(seen[0]["prompt_cache_key"],seen[1]["prompt_cache_key"]);
    assert!(seen[0].get("previous_response_id").is_none());
    assert_eq!(seen[1]["previous_response_id"], "resp_ws_fixture_1");
    assert_eq!(seen[1]["client_metadata"]["session_id"], seen[0]["client_metadata"]["session_id"]);
    assert!(seen.iter().all(|request|request.get("instructions").is_none()),
        "WebSocket seeds persona as a developer input instead of repeatedly resending instructions");
    let first = seen[0]["input"].as_array().unwrap();
    let second = seen[1]["input"].as_array().unwrap();
    assert!(first.iter().any(|item|
        item.to_string().contains("WEBSOCKET_IDENTITY_VERBATIM")));
    assert!(!second.iter().any(|item|
        item.to_string().contains("WEBSOCKET_IDENTITY_VERBATIM")),
        "previous_response_id must carry the already-seeded identity");
    assert!(
        !second.iter().any(|item| item.to_string().contains("첫 번째 질문")),
        "already-chained items stay on the server: first={first:?} second={second:?}"
    );
    assert!(
        second.iter().any(|item| item.to_string().contains("두 번째 질문")),
        "the new turn is sent: {second:?}"
    );
    let messages = app.get(&format!("/api/sessions/{}/messages", session["id"].as_str().unwrap()));
    let last = messages.as_array().unwrap().iter().rev()
        .find(|row| row["role"] == "assistant").unwrap();
    assert_eq!(
        last["metadata"]["continuity"]["incrementalTransport"]["wire"], "websocket"
    );
    assert_eq!(
        last["metadata"]["continuity"]["incrementalTransport"]["mode"], "delta"
    );
}

#[test]
fn codex_http_rejects_incomplete_and_unterminated_generations() {
    let _serial = serial();
    for terminal in [None, Some(json!({
        "type":"response.incomplete",
        "response":{"incomplete_details":{"reason":"max_output_tokens"}}
    }))] {
        let (port, requests) = scripted_provider(move |_| {
            let mut events = vec![json!({"type":"response.output_text.delta","delta":"PARTIAL_ONLY"})];
            events.extend(terminal.clone());
            events
        });
        let app = TestApp::new(port, "native");
        let provider = app.post("/api/providers", &json!({
            "name":"incomplete fixture","kind":"codex",
            "baseUrl":format!("http://127.0.0.1:{port}/codex"),
            "accountId":"fixture","apiKey":"fixture","enabled":true,"model":"fixture"
        }));
        let session = app.post("/api/sessions", &json!({"title":"incomplete"}));
        let stream = app.chat(&json!({
            "sessionId":session["id"],"providerId":provider["id"],"model":"fixture",
            "input":"hello","toolsEnabled":false,"autoLearning":false
        }));
        assert!(stream.contains("PARTIAL_ONLY"), "{stream}");
        assert!(stream.contains("\"type\":\"error\""), "{stream}");
        assert!(!stream.contains("assistant_saved"), "{stream}");
        assert_eq!(requests.lock().unwrap().len(), 1, "partial output must not be replayed");
        let executions=app.get("/api/executions");
        assert_eq!(executions[0]["status"], "needs_review");
        assert_eq!(executions[0]["kernelState"]["modelInFlight"], true,
            "an incomplete paid generation must retain its replay fence");
    }
}

#[test]
fn codex_receives_uploaded_image_and_file_contents() {
    use base64::Engine as _;
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| vec![
        json!({"type":"response.output_text.delta","delta":"ATTACHMENTS_RECEIVED"}),
        json!({"type":"response.completed","response":{"status":"completed","output":[]}}),
    ]);
    let app = TestApp::new(port, "native");
    let provider = app.post("/api/providers", &json!({
        "name":"attachment fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture","apiKey":"fixture","enabled":true,"model":"fixture",
        "capabilities":{"vision":true}
    }));
    let session = app.post("/api/sessions", &json!({"title":"attachments"}));
    let image_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=";
    let image = app.post("/api/attachments", &json!({
        "sessionId":session["id"],"name":"sample.png","mimeType":"image/png","data":image_data
    }));
    let file = app.post("/api/attachments", &json!({
        "sessionId":session["id"],"name":"sample.txt","mimeType":"text/plain",
        "data":base64::engine::general_purpose::STANDARD.encode("ATTACHMENT_SENTINEL")
    }));
    let stream = app.chat(&json!({
        "sessionId":session["id"],"providerId":provider["id"],"model":"fixture",
        "input":"첨부를 분석해","attachmentIds":[image["id"],file["id"]],
        "toolsEnabled":false,"autoLearning":false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    let usage=app.get(&format!("/api/sessions/{}/usage",session["id"].as_str().unwrap()));
    assert!(usage["totalTokens"].as_u64().unwrap()>0,"missing provider usage must not become a free call: {usage}");
    assert!(usage["estimatedCalls"].as_u64().unwrap()>0,"estimates must remain distinguishable: {usage}");
    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let parts = seen[0]["input"].as_array().unwrap().iter()
        .flat_map(|item| item["content"].as_array().into_iter().flatten()).collect::<Vec<_>>();
    assert!(parts.iter().any(|part| part["type"] == "input_text"
        && part["text"].as_str().is_some_and(|text| text.contains("ATTACHMENT_SENTINEL"))), "{seen:?}");
    assert!(parts.iter().any(|part| part["type"] == "input_image"
        && part["image_url"] == format!("data:image/png;base64,{image_data}")), "{seen:?}");
}

#[test]
fn compound_shell_file_creation_finishes_without_false_repair_or_reviewer() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        let tools = request["tools"].as_array().is_some_and(|tools| !tools.is_empty());
        if !tools {
            return vec![json!({"choices":[{"delta":{"content":"빈 텍스트 파일을 만들었습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"compound-create","type":"function",
            "function":{"name":"shell_exec","arguments":json!({"command":"touch '메모.txt' && ls -l '메모.txt'"}).to_string()}}]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"compound-create"}));
    let stream = app.chat(&json!({"sessionId":session["id"],"input":"메모라는 txt파일 만들어줘","autoLearning":false}));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("acceptance_retry"), "{stream}");
    assert!(!stream.contains("실행 증거가 부족"), "{stream}");
    assert_eq!(std::fs::metadata(app._root.join("메모.txt")).unwrap().len(), 0);
    let execution = app.get("/api/executions")[0].clone();
    assert_eq!(execution["status"], "completed", "{execution}");
    assert_eq!(execution["toolCalls"], 1, "{execution}");
    assert!(execution["modelCalls"].as_u64().unwrap() <= 3, "{execution}");
    let captured = requests.lock().unwrap();
    let chat = captured.iter().find(|request| request["tools"].as_array().is_some_and(|tools| !tools.is_empty())).unwrap();
    assert!(chat["messages"][0]["content"].as_str().unwrap().contains("언어 지침: 추론"));
}

#[test]
fn existing_file_creation_preserves_contents_without_model_calls() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| panic!("existing file must not require a model call"));
    let app = TestApp::new(port, "native");
    std::fs::write(app._root.join("existing.md"), "preserve these bytes").unwrap();
    let stream = app.chat(&json!({"input":"existing.md 파일을 만들어줘","autoLearning":false}));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(stream.contains("파일이 이미 있습니다"), "{stream}");
    assert_eq!(std::fs::read_to_string(app._root.join("existing.md")).unwrap(), "preserve these bytes");
    assert!(requests.lock().unwrap().is_empty());
    let execution = &app.get("/api/executions")[0];
    assert_eq!(execution["modelCalls"], 0, "{execution}");
    assert_eq!(execution["toolCalls"], 0, "{execution}");
    assert_eq!(execution["deterministicShortcut"]["kind"], "file_already_exists", "{execution}");
    let messages = app.get(&format!("/api/sessions/{}/messages", execution["sessionId"].as_str().unwrap()));
    let last = messages.as_array().unwrap().last().unwrap();
    assert_eq!(last["metadata"]["completionSource"], "existing_artifact");
    assert_eq!(last["metadata"]["usage"]["totalTokens"], 0);
}

#[test]
fn empty_file_creation_uses_one_model_call_and_no_paid_final_summary() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        if request["messages"].as_array().unwrap().iter().any(|message|
            message["role"] == "user" && message["content"].as_str().is_some_and(|text|text.contains("방금 만든 파일"))) {
            return vec![json!({"choices":[{"delta":{"content":"empty-once.md"},"finish_reason":"stop"}]})];
        }
        let tools = request["tools"].as_array().expect("creation tools");
        assert!(tools.len() <= 5, "unrelated tool schemas: {tools:?}");
        assert!(!tools.iter().any(|tool| tool["function"]["name"] == "web_search"));
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"create-once","type":"function",
            "function":{"name":"workspace_write","arguments":"{\"path\":\"empty-once.md\",\"content\":\"\"}"}}]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let stream = app.chat(&json!({"input":"empty-once.md 파일을 만들어줘","autoLearning":false}));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("acceptance_retry"), "{stream}");
    assert_eq!(std::fs::metadata(app._root.join("empty-once.md")).unwrap().len(), 0);
    assert_eq!(requests.lock().unwrap().len(), 1);
    let execution = &app.get("/api/executions")[0];
    assert_eq!(execution["modelCalls"], 1, "{execution}");
    assert_eq!(execution["toolCalls"], 1, "{execution}");
    let messages = app.get(&format!("/api/sessions/{}/messages", execution["sessionId"].as_str().unwrap()));
    let last = messages.as_array().unwrap().last().unwrap();
    assert_eq!(last["metadata"]["completionSource"], "verified_artifact");
    assert_eq!(last["metadata"]["usage"]["totalTokens"], 120);
    let followup = app.chat(&json!({"sessionId":execution["sessionId"],
        "input":"방금 만든 파일 이름만 말해줘","toolsEnabled":false,"autoLearning":false}));
    assert!(followup.contains("assistant_saved"), "{followup}");
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 2);
    let history = captured[1]["messages"].as_array().unwrap();
    let call = history.iter().position(|message| message["tool_calls"].as_array().is_some_and(|calls|!calls.is_empty())).unwrap();
    let result = history.iter().position(|message| message["role"] == "tool").unwrap();
    let summary = history.iter().position(|message| message["role"] == "assistant"
        && message["content"].as_str().is_some_and(|text|text.contains("파일을 만들었습니다"))).unwrap();
    assert!(call < result && result < summary, "tool exchange must precede the host summary: {history:?}");
}
