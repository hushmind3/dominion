//! MCP tests transport and contract.
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
    fn sse_parser_ignores_heartbeats_foreign_replies_and_crlf_boundaries() {
        let heartbeat = b": keepalive\r\n";
        assert!(sse_response_event(heartbeat, Some(&json!(7))).is_none());
        let notification = br#"data: {"jsonrpc":"2.0","method":"notifications/progress"}"#;
        assert!(sse_response_event(notification, Some(&json!(7))).is_none());
        let wrong = br#"data: {"jsonrpc":"2.0","id":8,"result":{"wrong":true}}"#;
        assert!(sse_response_event(wrong, Some(&json!(7))).is_none());
        let matching = br#"data: {"jsonrpc":"2.0","id":7,"result":{"ready":true}}"#;
        assert_eq!(sse_response_event(matching, Some(&json!(7))).unwrap()["result"]["ready"], true);
        assert_eq!(sse_event_boundary(b"data: x\r\n\r\n"), Some((8, 3)));
        assert_eq!(sse_event_boundary(b"data: x\n\n"), Some((7, 2)));
        let transport_error = br#"data: {"jsonrpc":"2.0","error":{"code":-32600,"message":"invalid"}}"#;
        assert!(sse_response_event(transport_error, Some(&json!(7))).is_some());
    }

    #[tokio::test]
    async fn sse_request_returns_without_waiting_for_server_to_close() {
        use axum::{response::sse::{Event, Sse}, routing::post, Json, Router};
        use std::convert::Infallible;
        let app = Router::new().route("/mcp", post(|Json(_request): Json<Value>| async move {
            let events = async_stream::stream! {
                yield Ok::<Event, Infallible>(Event::default().comment("keepalive"));
                yield Ok::<Event, Infallible>(Event::default().data(
                    json!({"jsonrpc":"2.0","id":999,"result":{"wrong":true}}).to_string()
                ));
                yield Ok::<Event, Infallible>(Event::default().data(
                    json!({"jsonrpc":"2.0","id":7,"result":{"ready":true}}).to_string()
                ));
                std::future::pending::<()>().await;
            };
            Sse::new(events)
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut client = HttpSession {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(3)).build().unwrap(),
            url: format!("http://{address}/mcp"),
            session: None,
            token: None,
            next_id: 1,
            era: HttpEra::Legacy,
        };
        let reply = tokio::time::timeout(
            Duration::from_secs(2),
            client.post(&json!({"jsonrpc":"2.0","id":7,"method":"tools/list","params":{}}))
        ).await.expect("MCP SSE response must not wait for stream closure").unwrap();
        assert_eq!(reply["result"]["ready"], true);
        server.abort();
    }


    #[tokio::test]
    async fn independent_mcp_servers_list_concurrently_but_keep_stable_order() {
        use axum::{routing::post, Json, Router};
        use std::sync::Arc;
        let state = temp_state();
        let gate = Arc::new(tokio::sync::Barrier::new(3));
        let mut servers = Vec::new();
        for index in 0..3 {
            let gate = gate.clone();
            let name = format!("MCP concurrency {index}");
            let tool_name = format!("tool_{index}");
            let app = Router::new().route("/mcp", post(move |Json(payload): Json<Value>| {
                let gate = gate.clone();
                let tool_name = tool_name.clone();
                async move {
                    let method = payload["method"].as_str().unwrap_or("");
                    if method == "tools/list" {
                        gate.wait().await;
                    }
                    Json(json!({
                        "jsonrpc":"2.0", "id": payload["id"],
                        "result": if method == "tools/list" {
                            json!({"tools":[{"name": tool_name,
                                "inputSchema":{"type":"object","properties":{}}}]})
                        } else { json!({}) }
                    }))
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            servers.push(tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            }));
            state.store.put("mcp", &json!({
                "id": format!("concurrent-{index}"),
                "name": name,
                "transport": "http",
                "url": format!("http://{addr}/mcp"),
            }));
        }
        let expected = enabled_servers(&state).iter().map(|server| {
            let suffix = server["id"].as_str().unwrap().trim_start_matches("concurrent-");
            format!("tool_{suffix}")
        }).collect::<Vec<_>>();
        let listed = tokio::time::timeout(Duration::from_secs(6), tools(&state))
            .await.expect("independent MCP discovery must not be serialized");
        assert_eq!(listed.len(), 3);
        let names = listed.into_iter().map(|tool| tool.tool).collect::<Vec<_>>();
        assert_eq!(names, expected, "completion order cannot reorder tool schemas");
        for server in servers { server.abort(); }
    }

    #[test]
    fn qualified_names_are_namespaced() {
        let tool = McpTool {
            server_name: "Echo Lab".into(),
            slug: slug("Echo Lab"),
            tool: "echo".into(),
            description: String::new(),
            schema: json!({}),
        };
        assert_eq!(slug("Echo Lab"), "echo_lab");
        assert_eq!(tool.qualified(), "mcp__echo_lab__echo");
    }

    #[test]
    fn collision_renames_only_ambiguous_server_namespaces() {
        let first=json!({"id":"one","name":"Search Lab","approval":false});
        let second=json!({"id":"two","name":"Search-Lab","approval":true});
        let lone=vec![first.clone()];
        assert_eq!(server_namespace(&first,&lone),"search_lab");
        let both=vec![first.clone(),second.clone()];
        let a=server_namespace(&first,&both);
        let b=server_namespace(&second,&both);
        assert_ne!(a,b);
        assert!(a.starts_with("search_lab_"));
        assert!(b.starts_with("search_lab_"));
        assert_eq!(a,server_namespace(&first,&both));
    }

    #[test]
    fn colliding_mcp_servers_retain_independent_approval_policies() {
        let state=temp_state();
        let first=state.store.put("mcp",&json!({"id":"mcp-one",
            "name":"Search Lab","approval":false})).unwrap();
        let second=state.store.put("mcp",&json!({"id":"mcp-two",
            "name":"Search-Lab","approval":true})).unwrap();
        let servers=enabled_servers(&state);
        let a=format!("mcp__{}__lookup",server_namespace(&first,&servers));
        let b=format!("mcp__{}__lookup",server_namespace(&second,&servers));
        assert_ne!(a,b);
        assert!(!requires_approval(&state,&a));
        assert!(requires_approval(&state,&b));
        // An ambiguous legacy alias must never silently target the first.
        assert!(requires_approval(&state,"mcp__search_lab__lookup"));
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
async fn modern_http_discovers_and_uses_routed_schema_headers_without_initialize() {
    use axum::{routing::post, Json, Router, http::HeaderMap};
    use std::sync::Arc;
    let state=temp_state();
    let seen=Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let events=seen.clone();
    let app=Router::new().route("/mcp",post(move |headers:HeaderMap, Json(payload):Json<Value>|{
        let events=events.clone();
        async move {
            let method=payload["method"].as_str().unwrap_or("");
            let header=|name:&str|headers.get(name).and_then(|v|v.to_str().ok()).unwrap_or("").to_string();
            events.lock().unwrap().push(json!({
                "method":method,"version":header("MCP-Protocol-Version"),
                "route":header("Mcp-Method"),"tool":header("Mcp-Name"),
                "region":header("Mcp-Param-Region"),
                "metaVersion":payload["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"]
            }));
            let result=match method {
                "server/discover"=>json!({"resultType":"complete",
                    "supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}),
                "tools/list"=>json!({"resultType":"complete","tools":[]}),
                "tools/call"=>json!({"resultType":"complete","content":[{"type":"text","text":"ok"}]}),
                _=>json!({}),
            };
            Json(json!({"jsonrpc":"2.0","id":payload["id"],"result":result}))
        }
    }));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr=listener.local_addr().unwrap();
    let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
    let record=json!({"id":"modern-test","name":"modern-test","transport":"http","url":format!("http://{addr}/mcp")});
    let mut session=HttpSession::open(&record,&state).await.unwrap();
    assert!(session.modern());
    session.request("tools/list",json!({})).await.unwrap();
    session.set_tool_schema(json!({"type":"object","properties":{
        "region":{"type":"string","x-mcp-header":"Region"}}}));
    let response=session.request("tools/call",json!({
        "name":"execute_sql","arguments":{"region":"서울"}})).await.unwrap();
    assert_eq!(response["content"][0]["text"],"ok");
    let history=seen.lock().unwrap();
    assert_eq!(history.len(),3,"modern transport must not initialize");
    assert_eq!(history[0]["method"],"server/discover");
    for entry in history.iter(){
        assert_eq!(entry["version"],"2026-07-28");
        assert_eq!(entry["route"],entry["method"]);
        assert_eq!(entry["metaVersion"],"2026-07-28");
    }
    assert_eq!(history[2]["tool"],"execute_sql");
    assert_eq!(history[2]["region"],"=?base64?7ISc7Jq4?=");
    server.abort();
}
