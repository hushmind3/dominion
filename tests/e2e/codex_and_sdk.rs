//! codex and sdk E2E contracts backed by the shared real-process fixture.
use super::*;

#[test]
fn codex_websocket_verified_tool_finishes_without_extra_model_call() {
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
    assert!(stream.contains("acceptance_passed"), "{stream}");
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("ws_tool.txt")).unwrap_or_default(),
        "WS_TOOL_OK"
    );
    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 1,
        "verified output should not require another paid model round: {seen:?}");
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
    // The tool receipt and independently verified completion already exist.
    // An extra response.create round only burns tokens and increases latency.
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
    assert!(stream.contains("acceptance_passed"), "{stream}");
    assert!(stream.contains("assistant_saved"), "{stream}");
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
    assert_eq!(seen.len(), 1,
        "verified tool execution must not start a redundant HTTP continuation");
    let messages = app.get(&format!("/api/sessions/{}/messages",session["id"].as_str().unwrap()));
    assert!(messages.as_array().unwrap().iter().any(|row|
        row["tool_calls"].as_array().is_some_and(|calls|
            calls.iter().any(|call|call["id"]=="call_http_tool_1"))),
        "the native tool call must be durably recorded");

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
    // The stable high-priority persona is sent as an input item only on
    // the first socket request. Later turns inherit it by response ID.
    assert!(seen[0]["input"].to_string().contains("WEBSOCKET_IDENTITY_VERBATIM"));
    assert!(!seen[1]["input"].to_string().contains("WEBSOCKET_IDENTITY_VERBATIM"),
        "repeating 4k+ persona on every turn wastes the WebSocket chain");
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



#[test]
fn native_sdk_shares_chat_harness_sqlite_and_single_file_completion(){
    let _serial=serial();
    let (port,requests)=scripted_provider(|_|vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"sdk-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"sdk-one.md\",\"content\":\"\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":40,"completion_tokens":10,"total_tokens":50}})]);
    let app=TestApp::new(port,"native");
    app.post("/api/policy",&json!({"autoApproveAll":true}));
    let run=app.post("/api/sdk/runs",&json!({"instruction":"sdk-one.md 파일을 만들어줘"}));
    let id=run["id"].as_str().expect("SDK run ID");
    let mut final_run=Value::Null;
    for _ in 0..100{
        final_run=app.get(&format!("/api/sdk/runs/{id}"));
        if matches!(final_run["status"].as_str(),Some("completed"|"failed")){break;}
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(final_run["status"],"completed","{final_run}");
    assert_eq!(std::fs::metadata(app._root.join("sdk-one.md")).unwrap().len(),0);
    assert_eq!(requests.lock().unwrap().len(),1);
    let execution=&app.get("/api/executions")[0];
    assert_eq!(execution["runtime"],"dominion-core");
    assert_eq!(execution["kernelState"]["native"]["status"],"completed");
    assert_eq!(execution["kernelState"]["receipts"].as_array().unwrap().len(),1);
    assert_eq!(final_run["checkpoint"]["usage"]["prompt_tokens"],40);
    assert!(app._root.join("runtime").join("dominion.sqlite").is_file());
    assert!(!app._root.join("runtime").join("sdk-runs").exists(),"SDK must not create a second file-backed execution store");
}



#[test]
fn native_sdk_approval_resolves_the_shared_chat_waiter_without_restarting_the_model(){
    let _serial=serial();
    let (port,requests)=scripted_provider(|_|vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"sdk-approved-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"sdk-approved.md\",\"content\":\"\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":40,"completion_tokens":10,"total_tokens":50}})]);
    let app=TestApp::new(port,"native");
    let run=app.post("/api/sdk/runs",&json!({"instruction":"sdk-approved.md 파일을 만들어줘"}));
    let id=run["id"].as_str().unwrap();let mut view=Value::Null;
    for _ in 0..100{view=app.get(&format!("/api/sdk/runs/{id}"));if view["status"]=="awaiting_approval"{break;}std::thread::sleep(Duration::from_millis(20));}
    assert_eq!(view["status"],"awaiting_approval","{view}");
    assert!(!app._root.join("sdk-approved.md").exists());
    let call=view["checkpoint"]["pendingCall"]["id"].clone();
    app.post(&format!("/api/sdk/runs/{id}/approve"),&json!({"callId":call}));
    for _ in 0..100{view=app.get(&format!("/api/sdk/runs/{id}"));if matches!(view["status"].as_str(),Some("completed"|"failed")){break;}std::thread::sleep(Duration::from_millis(20));}
    assert_eq!(view["status"],"completed","{view}");
    assert!(app._root.join("sdk-approved.md").is_file());
    assert_eq!(requests.lock().unwrap().len(),1,"approval must resume the same native execution");
    assert_eq!(app.get("/api/executions").as_array().unwrap().len(),1);
}


#[test]
fn owner_replacement_discards_stale_tool_calls_inside_the_same_native_driver(){
    let _serial=serial();
    use std::sync::atomic::{AtomicBool,Ordering};
    let started=Arc::new(AtomicBool::new(false));let release=Arc::new(AtomicBool::new(false));
    let started_model=started.clone();let release_model=release.clone();
    let calls=Arc::new(std::sync::atomic::AtomicUsize::new(0));let count=calls.clone();
    let (port,requests)=scripted_provider(move |_|{
        let n=count.fetch_add(1,Ordering::SeqCst);
        if n==0 {started_model.store(true,Ordering::SeqCst);let deadline=std::time::Instant::now()+Duration::from_secs(10);
            while !release_model.load(Ordering::SeqCst)&&std::time::Instant::now()<deadline {std::thread::sleep(Duration::from_millis(5));}}
        let path=if n==0{"old-target.md"}else{"new-target.md"};
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("owner-call-{n}"),"type":"function",
            "function":{"name":"workspace_write","arguments":json!({"path":path,"content":""}).to_string()}}]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}})]
    });
    let app=TestApp::new(port,"native");app.post("/api/policy",&json!({"autoApproveAll":true}));
    let run=app.post("/api/sdk/runs",&json!({"instruction":"old-target.md 파일을 만들어줘"}));
    let id=run["id"].as_str().unwrap();
    for _ in 0..200 {if started.load(Ordering::SeqCst){break;}std::thread::sleep(Duration::from_millis(10));}
    assert!(started.load(Ordering::SeqCst));
    let changed=app.post(&format!("/api/sdk/runs/{id}/steer"),&json!({"mode":"replace","instruction":"new-target.md 파일을 만들어줘"}));
    assert_eq!(changed["revision"],1);release.store(true,Ordering::SeqCst);
    let mut final_run=Value::Null;
    for _ in 0..250 {final_run=app.get(&format!("/api/sdk/runs/{id}"));if matches!(final_run["status"].as_str(),Some("completed"|"failed")){break;}std::thread::sleep(Duration::from_millis(20));}
    assert_eq!(final_run["status"],"completed","{final_run}");
    assert!(!app._root.join("old-target.md").exists());assert!(app._root.join("new-target.md").is_file());
    let execution=&app.get("/api/executions")[0];
    assert_eq!(execution["kernelState"]["receipts"].as_array().unwrap().len(),1);
    assert_eq!(execution["modelCalls"],2);
    let seen=requests.lock().unwrap();assert_eq!(seen.len(),2);
    assert!(seen[1]["messages"].to_string().contains("현재 유효한 전체 목표"));
}


#[test]
fn explicit_job_quota_blocks_the_first_over_budget_http_request(){
    let _serial=serial();
    let (port,requests)=scripted_provider(|_|vec![json!({"choices":[{"delta":{"content":"unexpected model invocation"},"finish_reason":"stop"}]})]);
    let app=TestApp::new(port,"native");
    let run=app.post("/api/sdk/runs",&json!({"instruction":format!("현재 프로젝트의 실행 구조를 설명해줘. 참고 자료: {}","가".repeat(1500)),"tokenBudget":1024}));
    let id=run["id"].as_str().unwrap();let mut final_run=Value::Null;
    for _ in 0..200 {final_run=app.get(&format!("/api/sdk/runs/{id}"));if matches!(final_run["status"].as_str(),Some("completed"|"failed")){break;}std::thread::sleep(Duration::from_millis(20));}
    assert_eq!(final_run["status"],"failed","{final_run}");
    assert!(final_run["error"].as_str().unwrap_or("").contains("예산"),"{final_run}");
    assert_eq!(requests.lock().unwrap().len(),0,"quota must be checked before the paid model endpoint");
}


#[test]
fn owner_replacement_during_approval_cannot_write_the_old_goal_or_escape_workspace(){
    let _serial=serial();
    let count=Arc::new(std::sync::atomic::AtomicUsize::new(0));let calls=count.clone();
    let (port,requests)=scripted_provider(move |_|{
        let n=calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        let path=if n==0{"stale-approved.md"}else{"current-approved.md"};
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("steered-approved-{n}"),"type":"function","function":{"name":"workspace_write","arguments":json!({"path":path,"content":""}).to_string()}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}})]
    });
    let app=TestApp::new(port,"native");
    let project=app._root.join("selected-project");std::fs::create_dir_all(&project).unwrap();
    let run=app.post("/api/sdk/runs",&json!({"instruction":"stale-approved.md 파일을 만들어줘","workspaceDir":project}));
    let id=run["id"].as_str().unwrap();let mut view=Value::Null;
    for _ in 0..150{view=app.get(&format!("/api/sdk/runs/{id}"));if view["status"]=="awaiting_approval"{break;}std::thread::sleep(Duration::from_millis(20));}
    assert_eq!(view["status"],"awaiting_approval","{view}");
    let old_call=view["checkpoint"]["pendingCall"]["id"].clone();
    let changed=app.post(&format!("/api/sdk/runs/{id}/steer"),&json!({"mode":"replace","instruction":"current-approved.md 파일을 만들어줘"}));
    assert_eq!(changed["revision"],1);
    app.post(&format!("/api/sdk/runs/{id}/approve"),&json!({"callId":old_call}));
    for _ in 0..150{view=app.get(&format!("/api/sdk/runs/{id}"));if view["status"]=="awaiting_approval"&&view["checkpoint"]["pendingCall"]["id"]!=old_call{break;}std::thread::sleep(Duration::from_millis(20));}
    let new_call=view["checkpoint"]["pendingCall"]["id"].clone();assert_ne!(new_call,old_call,"{view}");
    assert!(!project.join("stale-approved.md").exists());
    app.post(&format!("/api/sdk/runs/{id}/approve"),&json!({"callId":new_call}));
    for _ in 0..150{view=app.get(&format!("/api/sdk/runs/{id}"));if matches!(view["status"].as_str(),Some("completed"|"failed")){break;}std::thread::sleep(Duration::from_millis(20));}
    assert_eq!(view["status"],"completed","{view}");
    assert!(project.join("current-approved.md").is_file());
    assert!(!project.join("stale-approved.md").exists());assert!(!app._root.join("current-approved.md").exists());
    assert_eq!(requests.lock().unwrap().len(),2);
}


#[test]
fn retired_media_and_rsi_remain_read_only_and_cannot_queue_new_work(){
    let _serial=serial();
    let app=TestApp::new(1,"native");
    let root=app._root.clone();
    let id="cfa99b14-5aaa-44d3-a5a6-118236eb810d";
    let output=root.join("runtime/video").join(id);std::fs::create_dir_all(&output).unwrap();
    let original=b"archived original bytes";std::fs::write(output.join("output.mp4"),original).unwrap();
    {
        let db=Connection::open(root.join("runtime/dominion.sqlite")).unwrap();
        for (kind,value) in [("job",json!({"id":id,"kind":"video","status":"completed","input":{"title":"archived"}})),
            ("rsi_campaign",json!({"id":"archived-rsi","status":"completed","history":[{"accepted":true}]}))]{
            db.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3)",rusqlite::params![kind,value["id"].as_str().unwrap(),value.to_string()]).unwrap();
        }
    }
    drop(app);
    let app=TestApp::from_root(1,"native",root,&[]);
    assert_eq!(app.get(&format!("/api/video/jobs/{id}"))["title"],"archived");
    assert_eq!(app.get("/api/rsi/campaigns/archived-rsi")["history"][0]["accepted"],true);
    let bytes=app.client.get(format!("{}/api/video/jobs/{id}/content",app.base)).send().unwrap().error_for_status().unwrap().bytes().unwrap();
    assert_eq!(&bytes[..],original);
    let before=app.get("/api/jobs").as_array().unwrap().len();
    for endpoint in ["/api/video/jobs","/api/video/generate","/api/video/config","/api/youtube/upload","/api/rsi/campaigns","/api/rsi/inspect","/api/rsi/issues"] {
        let response=app.client.post(format!("{}{endpoint}",app.base)).json(&json!({"brief":"must not start","instruction":"must not start"})).send().unwrap();
        assert!(matches!(response.status().as_u16(),404|405|410),"{endpoint}: {}",response.status());
    }
    for kind in ["video","youtube_upload","rsi_evolve","rsi_compare","rsi_generation"]{
        let response=app.client.post(format!("{}/api/jobs",app.base)).json(&json!({"kind":kind,"input":{"prompt":"must not start"}})).send().unwrap();
        assert_eq!(response.status().as_u16(),400,"{kind}");
    }
    assert_eq!(app.get("/api/jobs").as_array().unwrap().len(),before);
    assert_eq!(std::fs::read(output.join("output.mp4")).unwrap(),original);
    assert!(!app.get("/api/oauth/providers").as_array().unwrap().iter().any(|p|p["id"]=="youtube"));
}
