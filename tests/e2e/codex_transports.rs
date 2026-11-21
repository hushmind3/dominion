//! codex transports integration scenarios, using the shared real-binary fixture.
use super::*;

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
    assert!(stream.contains("웹소켓 도구 실행 완료"), "{stream}");
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
    assert_eq!(seen[1]["previous_response_id"], "resp_ws_tool_1");
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
    assert!(
        !second
            .iter()
            .any(|item| item.to_string().contains("ws_tool.txt 파일을 만들어줘")),
        "the chained user turn must not be re-sent: {second:?}"
    );
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
                json!({"type":"response.completed","response":{
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
            json!({"type":"response.completed","response":{
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
    // Codex Responses WebSocket holds the stable developer item in the
    // prior-response chain. Sending it again on every turn wastes tokens;
    // a changed revision must instead force an explicit rebase (unit-tested).
    assert!(seen[0]["input"].as_array().unwrap().iter().any(|item|
        item["role"]=="developer" && item.to_string().contains("WEBSOCKET_IDENTITY_VERBATIM")),
        "first call must transmit the complete persona as developer input");
    assert!(!seen[1]["input"].as_array().unwrap().iter().any(|item|
        item["role"]=="developer" || item.to_string().contains("WEBSOCKET_IDENTITY_VERBATIM")),
        "unchanged persona must be inherited, not resent on the chained turn");
    let first = seen[0]["input"].as_array().unwrap();
    let second = seen[1]["input"].as_array().unwrap();
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
        assert_eq!(app.get("/api/executions")[0]["status"], "failed");
    }
}

#[test]
fn codex_receives_uploaded_image_and_file_contents() {
    use base64::Engine as _;
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| vec![
        json!({"type":"response.output_text.delta","delta":"ATTACHMENTS_RECEIVED"}),
        json!({"type":"response.completed","response":{}}),
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


