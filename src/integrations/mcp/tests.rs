//! HTTP/stdin MCP contract tests: legacy 2024 and modern 2026 sessions.
use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn pinned_native_contract_changes_on_server_reconfiguration_and_slug_collision(){
        let state=temp_state();
        let schema=json!({"type":"object","required":["value"],
            "properties":{"value":{"type":"string"}}});
        let server=json!({"id":"mcp-a","name":"Echo Lab","transport":"http",
            "url":"http://127.0.0.1:12112/mcp","enabled":true});
        state.store.put("mcp",&server).unwrap();
        let first=native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").unwrap();
        let second=native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").unwrap();
        assert_eq!(first,second);
        let mut changed=server.clone();
        changed["url"]=json!("http://127.0.0.1:12113/mcp");
        state.store.put("mcp",&changed).unwrap();
        let changed=native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").unwrap();
        assert_ne!(first,changed,"reconfigured endpoints cannot inherit approval");
        state.store.put("mcp",&json!({"id":"mcp-b","name":"Echo-Lab",
            "transport":"http","enabled":true,"url":"http://127.0.0.1:9999"})).unwrap();
        assert!(native_contract_fingerprint(&state,"mcp__echo_lab__echo",
            &schema,"Returns echo").is_err(),"duplicate namespaces must fail closed");
    }

    #[test]
    fn qualified_names_are_namespaced() {
        let tool = McpTool {
            server_id: "stable-record-001".into(),
            server_name: "Echo Lab".into(),
            slug: slug("Echo Lab"),
            tool: "echo".into(),
            description: String::new(),
            schema: json!({}),
        };
        assert_eq!(slug("Echo Lab"), "echo_lab");
        assert_eq!(tool.qualified(), "mcp__echo_lab__echo");
        assert_eq!(tool.server_id, "stable-record-001");
    }

    #[tokio::test]
    async fn failed_mcp_discovery_reports_the_exact_installed_record() {
        let state=temp_state();
        state.store.put("mcp",&json!({
            "id":"broken-connector", "name":"Broken connector",
            "transport":"stdio", "command":"__nonexistent_sub3_mcp_server__",
            "enabled":true
        })).unwrap();
        let (tools, errors)=tools_with_errors(&state).await;
        assert!(tools.is_empty());
        assert_eq!(errors.len(),1);
        assert_eq!(errors[0]["mcpId"],"broken-connector");
        assert_eq!(errors[0]["name"],"Broken connector");
        assert!(errors[0]["error"].is_string());
    }

    #[tokio::test]
    async fn http_reuses_mcp_session_and_reconnects_after_credential_rotation() {
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        use tokio::io::{AsyncReadExt,AsyncWriteExt,AsyncBufReadExt};
        let state=temp_state();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let starts=Arc::new(AtomicUsize::new(0));
        let calls=Arc::new(AtomicUsize::new(0));
        let starts_task=starts.clone();
        let calls_task=calls.clone();
        let server_task=tokio::spawn(async move {
            loop {
                let Ok((stream,_))=listener.accept().await else {break;};
                let starts=starts_task.clone();
                let calls=calls_task.clone();
                tokio::spawn(async move {
                    let (reader,mut writer)=stream.into_split();
                    let mut reader=tokio::io::BufReader::new(reader);
                    loop {
                        let mut line=String::new();
                        if reader.read_line(&mut line).await.unwrap_or(0)==0 {break;}
                        if !line.starts_with("POST ") {break;}
                        let mut body_len=0usize;
                        let mut session_header=false;
                        loop {
                            let mut header=String::new();
                            if reader.read_line(&mut header).await.unwrap_or(0)==0 {return;}
                            if header.trim().is_empty(){break;}
                            let low=header.to_ascii_lowercase();
                            if let Some(value)=low.strip_prefix("content-length:"){
                                body_len=value.trim().parse().unwrap_or(0);
                            }
                            if low.starts_with("mcp-session-id: test-session") {
                                session_header=true;
                            }
                        }
                        if body_len==0 || body_len>65536 {return;}
                        let mut body=vec![0;body_len];
                        if reader.read_exact(&mut body).await.is_err(){return;}
                        let request:Value=serde_json::from_slice(&body).unwrap();
                        let method=request["method"].as_str().unwrap_or("");
                        let valid=method=="initialize" || session_header;
                        let response=match method {
                            "initialize"=>{
                                starts.fetch_add(1,Ordering::SeqCst);
                                json!({"jsonrpc":"2.0","id":request["id"],"result":{
                                    "protocolVersion":"2024-11-05",
                                    "capabilities":{"tools":{}},
                                    "serverInfo":{"name":"mock-mcp","version":"1"}
                                }})
                            }
                            "notifications/initialized"=>json!({}),
                            "tools/list"=>json!({"jsonrpc":"2.0","id":request["id"],
                                "result":{"tools":[{"name":"echo","description":"HTTP Echo",
                                    "inputSchema":{"type":"object"}}]}}),
                            "tools/call"=>{
                                calls.fetch_add(1,Ordering::SeqCst);
                                json!({"jsonrpc":"2.0","id":request["id"],
                                    "result":{"content":[{"type":"text",
                                        "text":request["params"]["arguments"]["text"]}]}})
                            }
                            _=>json!({"jsonrpc":"2.0","id":request["id"],
                                "error":{"code":-32601,"message":"unknown"}}),
                        };
                        let payload=if valid {response.to_string()}
                            else {r#"{"error":"missing MCP session id"}"#.to_string()};
                        let status=if valid {"200 OK"}else{"400 Bad Request"};
                        let raw=format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nmcp-session-id: test-session\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n{}",
                            payload.len(),payload);
                        if writer.write_all(raw.as_bytes()).await.is_err(){return;}
                    }
                });
            }
        });
        let server=json!({"id":"mcp-http-pool","name":"HTTP Lab","transport":"http",
            "url":format!("http://127.0.0.1:{port}/mcp"),"enabled":true});
        state.store.put("mcp",&server).unwrap();
        state.save_secret("mcp:mcp-http-pool","first-secret");
        let specs=tools(&state).await;
        assert_eq!(specs.len(),1);
        let qualified=specs[0].qualified();
        for text in ["one","two"] {
            let value=tokio::time::timeout(Duration::from_secs(10),
                call_pinned(&state,&qualified,&json!({"text":text}),
                    &specs[0].schema,&specs[0].description))
                .await.expect("HTTP MCP call stalled").unwrap();
            assert!(value.to_string().contains(text));
        }
        assert_eq!(starts.load(Ordering::SeqCst),1,
            "MCP HTTP initialization should be amortized across discovery and calls");
        assert_eq!(calls.load(Ordering::SeqCst),2);

        // Changing the scoped credential must not reuse a stale bearer session.
        state.save_secret("mcp:mcp-http-pool","second-secret");
        call(&state,&qualified,&json!({"text":"rotated"})).await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst),2);
        assert_eq!(calls.load(Ordering::SeqCst),3);
        invalidate(&state);
        call(&state,&qualified,&json!({"text":"reconnected"})).await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst),3);
        assert_eq!(calls.load(Ordering::SeqCst),4);
        server_task.abort();
    }

    #[tokio::test]
    async fn stdio_reuses_one_live_mcp_process_and_rotates_on_config_change() {
        // The shipped fixture is a real stdio MCP child, not a fake
        // transport. Hosted CI installs Node before the full test matrix.
        if std::process::Command::new("node").arg("--version").output().is_err() {
            eprintln!("node unavailable; stdio fixture skipped");
            return;
        }
        let state=temp_state();
        let script=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/mcp-echo-server.mjs");
        assert!(script.is_file(),"missing real MCP stdio fixture");
        let log=state.config.data_dir.join("mcp-initializations.log");
        let server=json!({
            "id":"mcp-pool-echo","name":"Echo Lab","enabled":true,
            "transport":"stdio","command":"node",
            "args":[script.to_string_lossy()],
            "env":{"DOMINION_MCP_TEST_INIT_LOG":log.to_string_lossy()}
        });
        state.store.put("mcp",&server).unwrap();
        let tools=tools(&state).await;
        assert_eq!(tools.len(),1);
        let qualified=tools[0].qualified();
        for text in ["first","second"] {
            let result=tokio::time::timeout(Duration::from_secs(10),
                call(&state,&qualified,&json!({"text":text})))
                .await.expect("MCP response timed out").unwrap();
            assert!(result.to_string().contains(&format!("MCP_ECHO: {text}")));
        }
        // The first discovery and two tool calls all borrowed the same
        // long-lived child; an idle stdio server must not restart per call.
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),1);
        let pinned=tokio::time::timeout(Duration::from_secs(10),call_pinned(
            &state,&qualified,&json!({"text":"verified"}),
            &tools[0].schema,&tools[0].description,
        )).await.expect("pinned MCP response timed out").unwrap();
        assert!(pinned.to_string().contains("MCP_ECHO: verified"));
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),1);
        let cloned=state.clone();
        call(&cloned,&qualified,&json!({"text":"shared"})).await.unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),1);

        let mut changed=server.clone();
        changed["env"]["DOMINION_MCP_TEST_CONFIG"]=json!("reconfigured");
        state.store.put("mcp",&changed).unwrap();
        invalidate(&state);
        call(&state,&qualified,&json!({"text":"after-config"})).await.unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),2);

        // A rotated secret changes the pool identity even if the public
        // server record was not edited.
        state.save_secret("mcp:mcp-pool-echo","rotated");
        call(&state,&qualified,&json!({"text":"after-rotation"})).await.unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(),3);
        assert!(state.integrations.mcp_sessions.idle.lock().unwrap().len()<=1);
        drop(cloned);
        drop(state);
    }

    #[test]
    fn mcp_approval_follows_policy() {
        let state = temp_state();
        state.store.put(
            "mcp",
            &json!({ "name": "Echo Lab", "transport": "stdio", "command": "node" }),
        );
        assert!(requires_approval(&state, "mcp__echo_lab__echo"));

        state.store.put(
            "policy",
            &json!({ "id": "default", "autoApproveMcp": true }),
        );
        assert!(!requires_approval(&state, "mcp__echo_lab__echo"));
    }
    #[tokio::test]
    async fn modern_mcp_http_uses_discovery_stateless_headers_and_live_pinned_schema(){
        use axum::{extract::State, http::{HeaderMap,StatusCode},
            routing::post, Json,Router};
        use std::sync::Arc;
        type Seen=Arc<Mutex<Vec<Value>>>;
        async fn serve(State(seen):State<Seen>,headers:HeaderMap,
            Json(request):Json<Value>)->(StatusCode,Json<Value>){
            let method=request["method"].as_str().unwrap_or("");
            let version=headers.get("mcp-protocol-version")
                .and_then(|h|h.to_str().ok()).unwrap_or("");
            let routed=headers.get("mcp-method")
                .and_then(|h|h.to_str().ok()).unwrap_or("");
            let name=headers.get("mcp-name").and_then(|h|h.to_str().ok())
                .unwrap_or("");
            let region=headers.get("mcp-param-region")
                .and_then(|h|h.to_str().ok()).unwrap_or("");
            seen.lock().unwrap().push(json!({"method":method,"version":version,
                "routed":routed,"name":name,"region":region,
                "session":headers.get("mcp-session-id").is_some(),
                "meta":request["params"]["_meta"]}));
            if version!=modern::VERSION||routed!=method||
                request["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"]!=version||
                (method=="tools/call"&&(name!="echo"||region!="kr-1")){
                return (StatusCode::BAD_REQUEST,Json(json!({"jsonrpc":"2.0",
                    "id":request["id"],"error":{"code":-32020,
                        "message":"missing or incorrect MCP header"}})));
            }
            let result=match method {
                "server/discover"=>json!({"resultType":"complete",
                    "supportedVersions":[modern::VERSION],"capabilities":{"tools":{}}}),
                "tools/list"=>json!({"resultType":"complete",
                    "tools":[{"name":"echo","description":"HTTP Echo",
                        "inputSchema":{"type":"object","properties":{
                            "region":{"type":"string","x-mcp-header":"Region"},
                            "text":{"type":"string"}}}}]}),
                "tools/call"=>{
                    if request["params"]["arguments"]["text"]=="needs-input"{
                        json!({"resultType":"input_required","inputRequests":[]})
                    }else{
                        json!({"resultType":"complete","content":[{"type":"text",
                            "text":request["params"]["arguments"]["text"]}]})
                    }
                },
                _=>json!({"resultType":"complete"})
            };
            (StatusCode::OK,Json(json!({"jsonrpc":"2.0",
                "id":request["id"],"result":result})))
        }
        let state=temp_state();
        let seen:Seen=Arc::new(Mutex::new(Vec::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let router=Router::new().route("/mcp",post(serve)).with_state(seen.clone());
        let worker=tokio::spawn(async move{axum::serve(listener,router).await.unwrap()});
        let server=json!({"id":"modern-http","name":"Modern Lab",
            "transport":"http","protocolMode":"modern",
            "url":format!("http://127.0.0.1:{port}/mcp"),"enabled":true});
        state.store.put("mcp",&server).unwrap();
        let listed=tools(&state).await;
        assert_eq!(listed.len(),1);
        let tool=&listed[0];
        let response=call_pinned(&state,&tool.qualified(),
            &json!({"region":"kr-1","text":"한글 결과"}),
            &tool.schema,&tool.description).await.unwrap();
        assert!(response.to_string().contains("한글 결과"));
        let before=seen.lock().unwrap().len();
        let result=call_pinned(&state,&tool.qualified(),
            &json!({"region":"kr-1","text":"needs-input"}),
            &tool.schema,&tool.description).await;
        assert!(result.unwrap_err().contains("input_required"));
        let requests=seen.lock().unwrap();
        assert_eq!(requests.len(),before+2,
            "MRTR input_required must never automatically repeat a paid tool call");
        assert_eq!(requests[0]["method"],"server/discover");
        assert!(!requests.iter().any(|r|r["method"]=="initialize"));
        assert!(requests.iter().all(|r|r["session"]==false));
        assert!(requests.iter().all(|r|r["version"]==modern::VERSION));
        assert_eq!(requests.last().unwrap()["region"],"kr-1");
        worker.abort();
    }

    #[tokio::test]
    async fn modern_mcp_auth_failure_does_not_fall_back_to_initialize(){
        use axum::{extract::State,http::StatusCode,routing::post,Json,Router};
        use std::sync::Arc;
        type Seen=Arc<Mutex<Vec<String>>>;
        async fn deny(State(seen):State<Seen>,Json(request):Json<Value>)
            ->(StatusCode,Json<Value>){
            seen.lock().unwrap().push(request["method"].as_str()
                .unwrap_or("").to_string());
            (StatusCode::UNAUTHORIZED,Json(json!({"jsonrpc":"2.0",
                "id":request["id"],"error":{"code":-32000,
                    "message":"authentication required"}})))
        }
        let state=temp_state();
        let seen:Seen=Arc::new(Mutex::new(Vec::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let app=Router::new().route("/mcp",post(deny)).with_state(seen.clone());
        let worker=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let server=json!({"id":"auth-modern","name":"Auth Modern",
            "transport":"http","protocolMode":"auto",
            "url":format!("http://127.0.0.1:{port}/mcp")});
        let error=match HttpSession::open(&server,&state).await {
            Ok(_)=>panic!("modern auth denial must fail closed"),
            Err(error)=>error
        };
        assert!(error.contains("401"),"{error}");
        assert_eq!(*seen.lock().unwrap(),vec!["server/discover"]);
        worker.abort();
    }


    #[tokio::test]
    async fn real_modern_stdio_never_initializes_and_persists_one_live_process(){
        if std::process::Command::new("node").arg("--version").output().is_err(){
            eprintln!("node not available; modern stdio fixture skipped");
            return;
        }
        let state=temp_state();
        let script=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/mcp-echo-modern-server.mjs");
        assert!(script.is_file(),"missing 2026 modern stdio fixture");
        let log=state.config.data_dir.join("mcp-modern-starts.log");
        let server=json!({
            "id":"modern-stdio","name":"Modern Stdio","transport":"stdio",
            "command":"node","protocolMode":"modern",
            "args":[script.to_string_lossy()],
            "env":{"DOMINION_MCP_TEST_MODERN_LOG":log.to_string_lossy()}
        });
        state.store.put("mcp",&server).unwrap();
        let tools=tools(&state).await;
        assert_eq!(tools.len(),1);
        let tool=&tools[0];
        let answer=call_pinned(&state,&tool.qualified(),
            &json!({"text":"first"}),&tool.schema,&tool.description).await.unwrap();
        assert!(answer.to_string().contains("MCP_MODERN: first"));
        let other=call(&state,&tool.qualified(),&json!({"text":"second"}))
            .await.unwrap();
        assert!(other.to_string().contains("MCP_MODERN: second"));
        let events=std::fs::read_to_string(&log).unwrap();
        assert_eq!(events.lines().collect::<Vec<_>>(),vec!["started"],
            "the modern connection must not send initialize or restart each tool");
        assert!(state.integrations.mcp_sessions.idle.lock().unwrap().len()<=1);
    }

