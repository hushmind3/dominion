//! Provider protocol and context-budget contract cases.
use super::*;

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn codex_ws_registers_full_persona_once_and_reuses_its_response_chain() {
        let long= format!("ROOM_OWNER_POLICY_{}\nPERSONA_SHALL_SURVIVE", "x".repeat(16_000));
        let (instructions, base)=codex_input(&[
            json!({"role":"system","content":long}),
            json!({"role":"user","content":"first owner request"}),
        ]);
        let persisted=codex_ws_persistent_input(&instructions,&base);
        assert_eq!(persisted.len(),2);
        assert_eq!(persisted[0]["role"],"developer");
        assert_eq!(persisted[0]["content"][0]["text"],instructions);
        assert_eq!(persisted[1]["role"],"user");
        let old=vec![
            super::super::codex_ws::item_signature(&persisted[0]),
            super::super::codex_ws::item_signature(&persisted[1]),
            super::super::codex_ws::item_signature(&json!({
                "role":"assistant","content":[{"type":"output_text","text":"reply"}]
            })),
        ];
        let mut next=persisted.clone();
        next.push(json!({"role":"assistant","content":[{"type":"output_text","text":"reply"}]}));
        next.push(json!({"role":"user","content":[{"type":"input_text","text":"second owner request"}]}));
        let incoming=next.iter().map(super::super::codex_ws::item_signature).collect::<Vec<_>>();
        assert_eq!(super::super::codex_ws::plan_chain(&old,&incoming),
            super::super::codex_ws::Plan::Delta(old.len()));
        let delta=next[old.len()..].to_vec();
        assert!(!json!(delta).to_string().contains("PERSONA_SHALL_SURVIVE"),
            "the second WebSocket wire request must not resend the 4k static persona");
        let mut altered=next.clone();
        altered[0]["content"][0]["text"]=json!("owner edited persona");
        let edited=altered.iter().map(super::super::codex_ws::item_signature).collect::<Vec<_>>();
        assert_eq!(super::super::codex_ws::plan_chain(&old,&edited),
            super::super::codex_ws::Plan::Full,
            "changing owner instructions must force a complete fresh chain");
        // HTTP must still receive the original full instructions; this new
        // construction is only used for the stateful Codex WebSocket path.
        assert!(instructions.contains("PERSONA_SHALL_SURVIVE"));
        assert_eq!(base.len(),1);
    }

    #[test]
    fn codex_input_preserves_attachment_text_and_images() {
        let image = "data:image/png;base64,aW1hZ2U=";
        let (instructions, input) = codex_input(&[
            json!({"role":"system","content":"identity"}),
            json!({"role":"user","content":[
                {"type":"text","text":"첨부를 분석해"},
                {"type":"text","text":"quoted file contents"},
                {"type":"image_url","image_url":{"url":image,"detail":"high"}}
            ]}),
            json!({"role":"user","content":[{"type":"image_url","image_url":{"url":image}}]}),
        ]);
        assert_eq!(instructions, "identity");
        assert_eq!(input.len(), 2, "attachment messages must not disappear");
        assert_eq!(input[0]["content"], json!([
            {"type":"input_text","text":"첨부를 분석해"},
            {"type":"input_text","text":"quoted file contents"},
            {"type":"input_image","image_url":image,"detail":"high"}
        ]));
        assert_eq!(input[1]["content"][0]["image_url"], image);
    }

    #[test]
    fn codex_input_keeps_tool_calls_and_results_structured() {
        let messages = vec![
            json!({"role":"system","content":"identity"}),
            json!({"role":"user","content":"write a file"}),
            json!({
                "role":"assistant","content":"",
                "tool_calls":[{
                    "id":"call-1","type":"function",
                    "function":{"name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"}
                }]
            }),
            json!({"role":"tool","tool_call_id":"call-1","content":"ok"}),
        ];
        let (instructions, input) = codex_input(&messages);
        assert_eq!(instructions, "identity");
        assert_eq!(input.len(), 3, "{input:?}");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call-1");
        assert_eq!(input[1]["name"], "workspace_write");
        assert_eq!(input[1]["arguments"], "{\"path\":\"a.txt\"}");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call-1");
        assert_eq!(input[2]["output"], "ok");
    }

    #[test]
    fn codex_function_calls_become_harness_tool_calls_without_duplicates() {
        let output = vec![
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"preamble"}]}),
            json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"}),
            json!({"type":"function_call","id":"fc_1d","call_id":"call_1","name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"}),
        ];
        let calls = codex_tool_calls(&output);
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "workspace_write");
        assert_eq!(calls[0].arguments, "{\"path\":\"a.txt\"}");
    }

    #[test]
    fn codex_tool_schema_moves_function_fields_to_the_top_level() {
        let tools = vec![json!({
            "type":"function",
            "function":{
                "name":"workspace_write",
                "description":"write",
                "parameters":{"type":"object"}
            }
        })];
        let codex = codex_tools(&tools);
        assert_eq!(codex.len(), 1, "{codex:?}");
        assert_eq!(codex[0]["type"], "function");
        assert_eq!(codex[0]["name"], "workspace_write");
        assert_eq!(codex[0]["description"], "write");
        assert_eq!(codex[0]["parameters"]["type"], "object");
        assert!(codex[0].get("function").is_none(), "{codex:?}");
    }

    #[test]
    fn reflection_quotes_missing_reasoning_without_dropping_old_context() {
        let messages = vec![
            json!({"role":"system","content":"s"}),
            json!({"role":"user","content":"original owner request"}),
            json!({"role":"assistant","content":"old assistant answer","reasoning_content":""}),
            json!({"role":"user","content":"followup"}),
            json!({"role":"assistant","content":"native beam","reasoning_content":"original thinking"}),
            json!({"role":"user","content":"current"}),
        ];
        let (preserved, quoted) = preserve_reflection_history(messages);
        assert!(quoted);
        assert_eq!(preserved.len(), 6);
        assert!(preserved[2]["content"].as_str().unwrap().contains("old assistant answer"));
        assert_eq!(preserved[4]["role"], "assistant");
        assert_eq!(preserved[4]["reasoning_content"], "original thinking");
        assert_eq!(preserved[5]["content"], "current");
    }

    #[test]
    fn runtime_hints_never_split_an_assistant_tool_call_from_its_result() {
        let messages = vec![
            json!({"role":"system","content":"s"}),
            json!({"role":"user","content":"write a file"}),
            json!({"role":"assistant","content":"","tool_calls":[{"id":"call-1","type":"function","function":{"name":"workspace_write","arguments":"{}"}}]}),
            json!({"role":"system","content":"read-before-write hint"}),
            json!({"role":"tool","tool_call_id":"call-1","content":"ok"}),
            json!({"role":"user","content":"continue"}),
        ];
        let ordered = order_tool_exchanges(messages);
        let roles = ordered.iter().map(|message| message["role"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(roles, vec!["system","user","assistant","tool","system","user"], "{ordered:?}");
        assert_eq!(ordered[3]["tool_call_id"], "call-1");
        assert_eq!(ordered[4]["content"], "read-before-write hint");
    }

    #[test]
    fn cache_usage_distinguishes_hit_from_missing_telemetry() {
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens_details":{"cached_tokens":4032}})),Some(4032));
        assert_eq!(cached_prompt_tokens(&json!({"prompt_cache_hit_tokens":1280})),Some(1280));
        assert_eq!(cached_prompt_tokens(&json!({"input_tokens_details":{"cached_tokens":512}})),Some(512));
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens":4049})),None);
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens_details":{"cached_tokens":0}})),Some(0));
    }

    #[test]
    fn reflection_requires_reasoning_content_on_assistant_history() {
        let mut messages = vec![
            json!({"role":"system","content":"s"}),
            json!({"role":"assistant","content":"a"}),
            json!({"role":"assistant","content":"b","reasoning_content":"kept"}),
            json!({"role":"user","content":"u"}),
        ];
        normalize_assistant_reasoning(&mut messages, true);
        assert_eq!(messages[1]["reasoning_content"], "", "{messages:?}");
        assert_eq!(messages[2]["reasoning_content"], "kept", "{messages:?}");
        assert!(messages[0].get("reasoning_content").is_none());
        normalize_assistant_reasoning(&mut messages, false);
        assert!(messages[1].get("reasoning_content").is_none(), "{messages:?}");
    }

    #[test]
    fn local_chat_template_never_receives_openai_reasoning_effort() {
        let mut body = json!({"model":"llama3.2:3b"});
        apply_reasoning_options(
            &mut body,
            "chat_template",
            Some(false),
            Some("low"),
        );
        assert!(
            body.get("reasoning_effort").is_none(),
            "Ollama rejects reasoning_effort on non-thinking models: {body}"
        );
        assert_eq!(
            body["chat_template_kwargs"]["enable_thinking"],
            false,
            "{body}"
        );

        let mut qwen = json!({"model":"qwen3:4b"});
        apply_reasoning_options(&mut qwen, "qwen", Some(true), Some("high"));
        assert!(qwen.get("reasoning_effort").is_none(), "{qwen}");
        assert_eq!(qwen["enable_thinking"], true, "{qwen}");

        let mut api = json!({"model":"reasoning-model"});
        apply_reasoning_options(&mut api, "openai", Some(true), Some("high"));
        assert_eq!(api["reasoning_effort"], "high", "{api}");
    }

    #[test]
    fn each_provider_receives_its_own_tool_window_from_full_evidence() {
        let base = ProviderRef {
            id:"local".into(), name:"local".into(), base_url:String::new(),
            api_key:String::new(), model:"small".into(), tool_mode:"compat".into(),
            reasoning_protocol:String::new(), local:true,
            context_tokens:Some(4096), max_output_tokens:Some(512),
            requests_per_minute: None, kind:"openai".into(), account_id:None,
        };
        let original = vec![
            json!({"role":"system","content":"working context"}),
            json!({"role":"tool","tool_call_id":"old","content":"x".repeat(40_000)}),
            json!({"role":"tool","tool_call_id":"latest","content":"y".repeat(9000)}),
        ];
        let mut local_messages = original.clone();
        fit_tool_context(&base, &mut local_messages, 500);
        let local_latest = local_messages[2]["content"].as_str().unwrap();
        assert_eq!(local_latest.chars().count(), 9000, "most recent evidence wins");
        assert!(local_messages[1]["content"].as_str().unwrap().chars().count() < 40_000);
        assert_eq!(local_messages[1]["tool_call_id"], "old");

        let mut api = base.clone();
        api.context_tokens = Some(128_000);
        api.local = false;
        let mut api_messages = original.clone();
        fit_tool_context(&api, &mut api_messages, 500);
        assert_eq!(api_messages, original, "large API receives full evidence");
        assert_eq!(original[1]["content"].as_str().unwrap().chars().count(), 40_000);
    }

    #[test]
    fn cache_aware_context_editing_saves_old_reads_but_retains_effects_and_recent_results() {
        let mut original = vec![json!({"role":"system","content":"standing owner task"})];
        let entries = [
            ("a", "workspace_read", "A".repeat(22_000)),
            ("b", "file_read", "B".repeat(22_000)),
            ("write", "workspace_write", "WRITE_PROOF".repeat(600)),
            ("error", "workspace_read", json!({"error":"tool failed","details":"e".repeat(6_000)}).to_string()),
            ("new1", "workspace_read", "X".repeat(5_000)),
            ("new2", "workspace_read", "Y".repeat(5_000)),
            ("new3", "workspace_read", "Z".repeat(5_000)),
        ];
        for (id, name, content) in entries {
            original.push(json!({"role":"assistant","content":"","tool_calls":[{
                "id":id,"type":"function",
                "function":{"name":name,"arguments":"{}"}
            }]}));
            original.push(json!({"role":"tool","tool_call_id":id,"content":content}));
        }
        let before_chars=serde_json::to_string(&original).unwrap().chars().count();
        let mut edited=original.clone();
        let saved=edit_stale_read_context(&mut edited, before_chars + 2_000, 0);
        assert!(saved > 30_000, "expected material reduction, got {saved}");
        assert!(serde_json::to_string(&edited).unwrap().chars().count() < before_chars - 30_000);
        assert!(edited[2]["content"].as_str().unwrap().contains("compacted"));
        assert!(edited[4]["content"].as_str().unwrap().contains("compacted"));
        for at in [6,8,10,12,14] {
            assert_eq!(edited[at],original[at], "write, error and newest reads stay exact");
        }
        for at in (1..original.len()).step_by(2) {
            assert_eq!(edited[at],original[at], "tool calls and IDs stay paired");
        }
        assert_eq!(original[2]["content"].as_str().unwrap().len(),22_000,
            "durable context must never be mutated");
        let mut short=original.clone();
        assert_eq!(edit_stale_read_context(&mut short,before_chars * 2,0),0);
        assert_eq!(short,original, "avoid breaking prompt cache far below threshold");
    }

    #[test]
    fn reused_tool_ids_never_reclassify_an_earlier_write_as_a_read() {
        let mut rows=vec![json!({"role":"system","content":"task"})];
        for (id,name,text) in [
            ("reused","workspace_write","WRITE_PROOF".repeat(4_000)),
            ("reused","workspace_read","READ_RESULT".repeat(4_000)),
            ("r1","workspace_read","new".repeat(2_000)),
            ("r2","workspace_read","new".repeat(2_000)),
            ("r3","workspace_read","new".repeat(2_000)),
        ] {
            rows.push(json!({"role":"assistant","content":"","tool_calls":[{
                "id":id,"function":{"name":name,"arguments":"{}"},"type":"function"
            }]}));
            rows.push(json!({"role":"tool","tool_call_id":id,"content":text}));
        }
        let original=rows.clone();
        let saved=edit_stale_read_context(&mut rows,40_000,0);
        assert!(saved>8_000, "older read should be compacted");
        assert_eq!(rows[2],original[2], "earlier write output cannot be cleared");
        assert!(rows[4]["content"].as_str().unwrap().contains("compacted"),
            "the newer read with the same ID is a separate operation");
    }

    #[test]
    fn old_tool_error_and_write_receipt_are_never_context_edited() {
        let mut messages=vec![json!({"role":"system","content":"task"})];
        for (id,name,content) in [
            ("w","workspace_write","updated".repeat(6_000)),
            ("e","workspace_read",json!({"error":"bad","details":"z".repeat(10_000)}).to_string()),
            ("a","workspace_read","new result".repeat(200)),
            ("b","workspace_read","new result".repeat(200)),
            ("c","workspace_read","new result".repeat(200)),
        ] {
            messages.push(json!({"role":"assistant","content":"","tool_calls":[{
                "id":id,"type":"function","function":{"name":name,"arguments":"{}"}
            }]}));
            messages.push(json!({"role":"tool","tool_call_id":id,"content":content}));
        }
        let original=messages.clone();
        let _=edit_stale_read_context(&mut messages,30_000,0);
        assert_eq!(messages[2],original[2]);
        assert_eq!(messages[4],original[4]);
    }

    #[test]
    fn json_escaped_tool_output_respects_provider_budget() {
        let original = "\"\n\\세션".repeat(120);
        let clipped = clip_tool_for_provider(&original, 160);
        assert!(escaped_content_chars(&clipped) <= 160);
        assert!(clipped.contains("stored execution_operation"));
    }

    #[test]
    fn native_tool_history_can_be_replayed_to_a_compat_local_model() {
        let before = vec![
            json!({"role":"system","content":"rules"}),
            json!({"role":"assistant","content":"","tool_calls":[{
                "id":"call_1","type":"function","function":{"name":"file_read","arguments":"{}"}
            }]}),
            json!({"role":"tool","tool_call_id":"call_1","content":"파일 내용"}),
        ];
        let compatible = compat_messages(&before);
        assert_eq!(compatible[2]["role"], "user");
        assert!(compatible[2]["content"].as_str().unwrap().contains("파일 내용"));
        assert!(compatible[1]["content"].as_str().unwrap().contains("file_read"));
        assert!(compatible[1].get("tool_calls").is_none());
    }

    #[tokio::test]
    async fn failed_native_cloud_route_switches_to_compat_local_with_tool_manual() {
        use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        async fn local_model(
            State(observed): State<std::sync::Arc<std::sync::Mutex<Vec<Value>>>>,
            Json(body): Json<Value>,
        ) -> axum::response::Response {
            observed.lock().unwrap().push(body);
            let payload = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"로컬 대체 완료"}}]})
            );
            ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], payload)
                .into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/chat/completions", post(local_model))
            .with_state(observed.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = std::env::temp_dir().join(format!("dominion-mixed-failover-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port:0, data_dir:root.clone(), web_dir:root.join("web"), workspace:root.clone(),
        };
        let state = AppState::new(config, crate::db::Store::open(&root.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&root.join("keys.json")));
        state.store.put("session", &json!({"id":"mixed"}));
        state.store.put("provider", &json!({
            "id":"cloud","baseUrl":"http://127.0.0.1:1","model":"cloud-native",
            "toolMode":"native","enabled":true,
        }));
        state.store.put("provider", &json!({
            "id":"local","baseUrl":format!("http://{address}"),"model":"local-compat",
            "toolMode":"compat","enabled":true,"local":true,"contextTokens":8192,
        }));
        let request = TurnRequest::from_value(&json!({
            "input":"도구로 파일 읽기","sessionId":"mixed","providerId":"cloud",
            "autoLearning":false,
        })).unwrap();
        let primary = select_provider(&state,&request,&state.store.get("session","mixed").unwrap()).unwrap();
        let tools = vec![json!({
            "type":"function","function":{"name":"workspace_read","description":"Read a file",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}}}}
        })];
        let messages = vec![
            json!({"role":"system","content":"You are a helper"}),
            json!({"role":"user","content":"도구로 파일 읽기"}),
            json!({"role":"assistant","content":"","tool_calls":[{
                "id":"tc1","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"test.txt\"}"}
            }]}),
            json!({"role":"tool","tool_call_id":"tc1","content":"test.txt 내용"}),
        ];
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let (answer, selected) = tokio::time::timeout(std::time::Duration::from_secs(5),
            stream_completion(&state,&reqwest::Client::new(),&primary,
                &messages,&tools,&request,"mixed",&tx,true)
        ).await.unwrap().unwrap();
        assert_eq!(selected.id,"local");
        assert_eq!(answer.content,"로컬 대체 완료");
        let calls = observed.lock().unwrap();
        assert_eq!(calls.len(),1);
        let body = &calls[0];
        assert!(body.get("tools").is_none());
        let prompt = body["messages"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("Available tools:"));
        assert!(prompt.contains("workspace_read"));
        assert_eq!(body["messages"][3]["role"],"user");
        assert!(body["messages"][3]["content"].as_str().unwrap().contains("test.txt 내용"));
        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_transient_failures_are_replayed() {
        assert!(transient_provider_error("remote 연결 실패: refused"));
        assert!(transient_provider_error("remote 호출 실패 (HTTP 503 Service Unavailable):"));
        assert!(transient_provider_error("모델 스트림이 완료 신호 없이 종료되었습니다."));
        assert!(!transient_provider_error("모델 응답이 콘텐츠 필터로 중단되었습니다."));
        assert!(!transient_provider_error("실행이 중단되었습니다."));
        assert!(!transient_provider_error("remote 호출 실패 (HTTP 400 Bad Request):"));
    }

    #[tokio::test]
    async fn transient_failure_uses_alternative_without_repeating_visible_output() {
        use axum::{routing::post, response::IntoResponse, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/chat/completions", post(|| async {
            let events = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"대체 모델 정상 응답"}}]})
            );
            ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], events).into_response()
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = std::env::temp_dir().join(format!("dominion-failover-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port:0, data_dir:root.clone(), web_dir:root.join("web"), workspace:root.clone(),
        };
        let state = AppState::new(config, crate::db::Store::open(&root.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&root.join("keys.json")));
        state.store.put("session", &json!({"id":"session-failover"}));
        state.store.put("provider", &json!({
            "id":"primary","baseUrl":"http://127.0.0.1:1","model":"broken",
            "toolMode":"native","enabled":true,
        }));
        state.store.put("provider", &json!({
            "id":"alternative","baseUrl":format!("http://{address}"),"model":"working",
            "toolMode":"native","enabled":true,
        }));
        let request = TurnRequest::from_value(&json!({
            "input":"응답해","sessionId":"session-failover","providerId":"primary","autoLearning":false,
        })).unwrap();
        let primary = select_provider(&state, &request, &state.store.get("session","session-failover").unwrap()).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (answer, selected) = tokio::time::timeout(std::time::Duration::from_secs(5),
            stream_completion(
                &state, &reqwest::Client::new(), &primary,
                &[json!({"role":"user","content":"응답해"})], &[],
                &request, "session-failover", &tx, true,
            )
        ).await.unwrap().unwrap();
        assert_eq!(selected.id, "alternative");
        assert_eq!(answer.content, "대체 모델 정상 응답");
        let mut output = String::new();
        while let Ok(event) = rx.try_recv() {
            if event["type"] == "text" {
                output.push_str(event["text"].as_str().unwrap_or(""));
            }
        }
        assert_eq!(output, "대체 모델 정상 응답");
        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hides_text_tool_calls_across_split_deltas() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pending = String::new();
        let mut in_tool = false;
        for piece in [
            "파일을 ",
            "<tool",
            "_call>{\"name\":\"workspace_write\",",
            "\"arguments\":{}}</tool",
            "_call>",
            "생성했습니다.",
        ] {
            stream_visible_text(&tx, piece, &mut pending, &mut in_tool);
        }
        if !in_tool && !pending.is_empty() {
            tx.send(json!({"type":"text","text":pending})).unwrap();
        }
        drop(tx);
        let mut text = String::new();
        while let Ok(event) = rx.try_recv() {
            text.push_str(event["text"].as_str().unwrap_or(""));
        }
        assert_eq!(text, "파일을 생성했습니다.");
        assert!(!text.contains("tool_call"));
    }

    #[tokio::test]
    async fn anthropic_native_stream_translates_messages_tools_and_usage() {
        use axum::{routing::post, Router, Json};
        use std::sync::{Arc, Mutex};
        let observed = Arc::new(Mutex::new(Vec::<Value>::new()));
        let captured = observed.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/messages",
            post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                let captured = captured.clone();
                async move {
                    captured.lock().unwrap().push(json!({
                        "headerKey": headers.get("x-api-key").and_then(|v| v.to_str().ok()),
                        "headerVersion": headers.get("anthropic-version").and_then(|v| v.to_str().ok()),
                        "body": body,
                    }));
                    let events = [
                        json!({"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":0}}}),
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"안녕"}}),
                        json!({"type":"content_block_stop","index":0}),
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool_1","name":"workspace_read","input":{}}}),
                        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"x\"}"}}),
                        json!({"type":"content_block_stop","index":1}),
                        json!({"type":"message_delta","usage":{"output_tokens":5}}),
                        json!({"type":"message_stop"}),
                    ];
                    let payload = events
                        .iter()
                        .map(|event| format!("event: x\ndata: {event}\n\n"))
                        .collect::<String>();
                    ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], payload)
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = std::env::temp_dir().join(format!("dominion-anthropic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port: 0, data_dir: root.clone(), web_dir: root.join("web"), workspace: root.clone(),
        };
        let state = AppState::new(
            config,
            crate::db::Store::open(&root.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&root.join("keys.json")),
        );
        let provider = ProviderRef {
            id: "anthropic".into(), name: "Anthropic".into(),
            base_url: format!("http://{address}"), api_key: "secret".into(),
            model: "claude-test".into(), tool_mode: "native".into(),
            reasoning_protocol: "anthropic".into(), local: false,
            context_tokens: None, max_output_tokens: Some(4096),
            requests_per_minute: None, kind: "anthropic".into(), account_id: None,
        };
        let request = TurnRequest::from_value(&json!({
            "input":"안녕","sessionId":"s","autoLearning":false
        })).unwrap();
        let messages = vec![
            json!({"role":"system","content":"system rule"}),
            json!({"role":"user","content":"안녕"}),
            json!({"role":"assistant","content":"","tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"x\"}"}}
            ]}),
            json!({"role":"tool","tool_call_id":"call_1","content":"ok"}),
        ];
        let tools = vec![json!({"type":"function","function":{
            "name":"workspace_read","description":"read",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}}}
        }})];
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let completion = stream_completion_once(
            &state, &reqwest::Client::new(), &provider, &messages, &tools,
            &request, "s", &tx, false,
        ).await.unwrap();
        assert_eq!(completion.content, "안녕");
        assert_eq!(completion.usage, 15);
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].name, "workspace_read");
        assert_eq!(
            serde_json::from_str::<Value>(&completion.tool_calls[0].arguments).unwrap(),
            json!({"path":"x"})
        );
        let calls = observed.lock().unwrap();
        let body = &calls[0]["body"];
        assert_eq!(calls[0]["headerKey"], "secret");
        assert_eq!(calls[0]["headerVersion"], "2023-06-01");
        assert_eq!(anthropic_message_text(&body["system"]), "system rule");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(completion.continuity.is_some());
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        drop(calls);
        // Confirm that editing reaches the actual Anthropic HTTP payload;
        // the durable input and assistant/tool IDs are not modified.
        let mut pressure = vec![
            json!({"role":"system","content":"Keep user's objective unchanged"}),
            json!({"role":"user","content":"Continue the same task"}),
        ];
        for (id, name, content) in [
            ("old1","workspace_read","A".repeat(28_000)),
            ("old2","file_read","B".repeat(28_000)),
            ("write","workspace_write","write receipt".repeat(450)),
            ("error","workspace_read",json!({"error":"permission denied","text":"z".repeat(5_000)}).to_string()),
            ("new1","workspace_read","recent-one".repeat(500)),
            ("new2","workspace_read","recent-two".repeat(500)),
            ("new3","workspace_read","recent-three".repeat(500)),
        ] {
            pressure.push(json!({"role":"assistant","content":"","tool_calls":[{
                "id":id,"type":"function",
                "function":{"name":name,"arguments":"{}"}
            }]}));
            pressure.push(json!({"role":"tool","tool_call_id":id,"content":content}));
        }
        let original=pressure.clone();
        let mut limited=provider.clone();
        limited.context_tokens=Some(28_000);
        let (tx2,_rx2)=tokio::sync::mpsc::unbounded_channel();
        let _completion=stream_completion_once(
            &state, &reqwest::Client::new(), &limited, &pressure, &[],
            &request, "s", &tx2, false,
        ).await.unwrap();
        let bodies=observed.lock().unwrap();
        assert_eq!(bodies.len(),2);
        let sent=bodies[1]["body"]["messages"].to_string();
        assert!(sent.contains("compacted"), "large old reads must be reduced");
        assert!(state.store.recent_audit(200).iter().any(|row|
            row["event"]=="chat.context_edit_prepared"
            && row["detail"]["wire"]=="anthropic"
            && row["detail"]["removedJsonCharsEstimate"].as_u64().unwrap_or(0)>=8_000
            && row["detail"]["billedTokenSavings"].is_null()),
            "the reduction must be observable without inventing actual billed tokens");
        assert!(sent.contains("recent-three"), "latest tool observations survive");
        assert!(sent.contains("write receipt"), "write result must survive");
        assert!(sent.contains("permission denied"), "tool errors must survive");
        assert_eq!(pressure,original, "provider must only edit its transmitted copy");
        drop(bodies);
        // The provider reasoning adapter must not enlarge the output after
        // budget admission, including when the remaining allowance is small.
        let mut thinking_request=request.clone();thinking_request.thinking=Some(true);
        limited.max_output_tokens=Some(1500);
        let (tx3,_rx3)=tokio::sync::mpsc::unbounded_channel();
        stream_completion_once(&state,&reqwest::Client::new(),&limited,&messages,&[],
            &thinking_request,"s",&tx3,false).await.unwrap();
        let bodies=observed.lock().unwrap();
        assert_eq!(bodies[2]["body"]["max_tokens"],1500);
        assert_eq!(bodies[2]["body"]["thinking"]["budget_tokens"],1499);
        drop(bodies);
        limited.max_output_tokens=Some(1024);
        let (tx4,_rx4)=tokio::sync::mpsc::unbounded_channel();
        let error=stream_completion_once(&state,&reqwest::Client::new(),&limited,&messages,&[],
            &thinking_request,"s",&tx4,false).await.err().unwrap();
        assert!(error.contains("[model_not_started]"));
        assert_eq!(observed.lock().unwrap().len(),3,"unaffordable reasoning must not reach HTTP");
        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }
}
