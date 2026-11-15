//! Product-path tests for lazy MCP schema discovery using real local HTTP
//! model and MCP servers. No credentials, remote network, or model charges.
use super::*;
use axum::{routing::post,Json,Router};
use std::sync::{Arc,Mutex,atomic::{AtomicUsize,Ordering}};

async fn fixture(model_url:String,mcp_url:String)->(AppState,PathBuf){
    let root=std::env::temp_dir().join(format!(
        "dominion-tool-routing-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store=crate::store::Store::open(&root.join("state.sqlite")).unwrap();
    let (bus,_)=tokio::sync::broadcast::channel(128);
    let mcp=crate::mcp::Mcp::new(vec![crate::mcp::Server{
        name:"ledger".into(),url:mcp_url,bearer_token:String::new(),
    }]).unwrap();
    let state=AppState{
        store,bus,mcp:Arc::new(mcp),
        groups:Arc::new(crate::group::Groups::open(root.join("groups")).await.unwrap()),
        videos:Arc::new(crate::video::Videos::open(root.join("videos")).await.unwrap()),
        improvements:Arc::new(crate::improvement::Improvements::open(
            root.join("improvements"),root.clone()).await.unwrap()),
        provider:crate::provider::ModelClient::new(
            crate::provider::ModelConfig::basic(model_url,"mock".into(),None),None
        ).unwrap(),
        workspace:root.clone(),
        workspace_write_lock:Arc::new(tokio::sync::Mutex::new(())),
        model_name:"mock".into(),
    };
    (state,root)
}
fn mock_reply_tool(id:&str,name:&str,args:Value)->Value{
    json!({"choices":[{"message":{"role":"assistant","content":null,
        "tool_calls":[{"id":id,"type":"function","function":{
            "name":name,"arguments":args.to_string()
        }}]}}],"usage":{"total_tokens":37}})
}
fn mock_reply_final(answer:&str)->Value{
    json!({"choices":[{"message":{"role":"assistant","content":answer}}],
        "usage":{"total_tokens":25}})
}

#[tokio::test]
async fn model_discovers_mcp_then_receives_only_the_promoted_full_schema_and_executes_it(){
    let seen=Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let model_seen=seen.clone();
    let model_app=Router::new().route("/v1/chat/completions",post(
        move |Json(request):Json<Value>| {
            let history=model_seen.clone();
            async move {
                let declared=request["tools"].as_array().cloned().unwrap_or_default()
                    .iter().filter_map(|tool|schema_router::name(tool).map(str::to_owned))
                    .collect::<Vec<_>>();
                history.lock().unwrap().push(declared.clone());
                let messages=request["messages"].as_array().cloned().unwrap_or_default();
                let has_discovery=messages.iter().any(|message|
                    message["role"]=="tool" &&
                    message["content"].as_str().is_some_and(|s|s.contains("eligibleMcpTools")));
                let has_receipt=messages.iter().any(|message|
                    message["role"]=="tool" &&
                    message["content"].as_str().is_some_and(|s|s.contains("verified-charge-42")));
                let response=if !has_discovery {
                    // The initial request must NOT eager-load the large catalog.
                    assert!(declared.iter().any(|n|n=="tool_discover"));
                    assert!(!declared.iter().any(|n|n.starts_with("mcp__")));
                    mock_reply_tool("discover-1","tool_discover",
                        json!({"query":"invoice payment receipt","limit":3}))
                } else if !has_receipt {
                    // This full schema was not available until discovery.
                    assert!(declared.iter().any(|n|n=="mcp__ledger__lookup_invoice"));
                    mock_reply_tool("invoke-1","mcp__ledger__lookup_invoice",
                        json!({"query":"last invoice"}))
                }else{
                    mock_reply_final("Retrieved a verified billing receipt.")
                };
                Json(response)
            }
        }
    ));
    let model_listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_url=format!("http://{}/v1/chat/completions",
        model_listener.local_addr().unwrap());
    let model_server=tokio::spawn(async move{
        axum::serve(model_listener,model_app).await.unwrap()
    });

    let executed=Arc::new(AtomicUsize::new(0));
    let executed_service=executed.clone();
    let mcp_app=Router::new().route("/mcp",post(
        move |Json(request):Json<Value>|{
            let executed=executed_service.clone();
            async move {
                let id=request["id"].clone();
                let output=if request["method"]=="tools/list"{
                    let mut tools=(0..45).map(|i|json!({
                        "name":format!("unrelated_{i}"),
                        "description":format!("Telemetry ingestion and metric series {i}"),
                        "inputSchema":{"type":"object","properties":{
                            "payload":{"type":"string","description":"marker".repeat(160)}
                        }}
                    })).collect::<Vec<_>>();
                    tools.push(json!({"name":"lookup_invoice",
                        "description":"Find invoices and payment receipts",
                        "inputSchema":{"type":"object","properties":{
                            "query":{"type":"string","description":"Invoice keyword"}
                        },"required":["query"]}}));
                    json!({"tools":tools})
                } else {
                    assert_eq!(request["method"],"tools/call");
                    assert_eq!(request["params"]["name"],"lookup_invoice");
                    executed.fetch_add(1,Ordering::SeqCst);
                    json!({"content":[{"type":"text",
                        "text":"verified-charge-42"}]})
                };
                Json(json!({"jsonrpc":"2.0","id":id,"result":output}))
            }
        }
    ));
    let mcp_listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mcp_url=format!("http://{}/mcp",mcp_listener.local_addr().unwrap());
    let mcp_server=tokio::spawn(async move{
        axum::serve(mcp_listener,mcp_app).await.unwrap()
    });
    let (state,root)=fixture(model_url,mcp_url).await;
    let (task,created)=state.store.submit("lazy-tools-2026",
        &json!({"prompt":"Use an external connector to verify a recent charge",
            "allowMcp":true,"planningMode":"never",
            "maxSteps":4,"tokenBudget":10_000})).unwrap();
    assert!(created);
    let id=task["id"].as_str().unwrap().to_owned();
    run(state.clone(),id.clone()).await;
    let record=state.store.get(&id).unwrap();
    assert_eq!(record["status"],"completed","{record:?}");
    assert_eq!(executed.load(Ordering::SeqCst),1);
    let per_turn=seen.lock().unwrap().clone();
    assert_eq!(per_turn.len(),3);
    assert!(!per_turn[0].iter().any(|name|name.starts_with("mcp__")));
    assert!(per_turn[1].iter().any(|name|name=="mcp__ledger__lookup_invoice"));
    let events=state.store.events_after(&id,0,200);
    let routing=events.iter().filter(|event|event["type"]=="tool_schema_routed")
        .collect::<Vec<_>>();
    assert_eq!(routing.len(),3);
    assert!(routing[0]["detail"]["catalogBytes"].as_u64().unwrap_or(0)>
        routing[0]["detail"]["advertisedBytes"].as_u64().unwrap_or(0));
    assert!(routing[0]["detail"]["omittedCount"].as_u64().unwrap_or(0)>=45);
    assert!(events.iter().any(|event|event["type"]=="tool_schema_promoted"));
    model_server.abort();
    mcp_server.abort();
    drop(state);
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn model_cannot_invoke_unadvertised_mcp_or_write_tool(){
    // A model may hallucinate a function name from a previous context. The
    // native dispatcher must fail closed even when it owns dispatch logic.
    let model_app=Router::new().route("/v1/chat/completions",post(
        |Json(request):Json<Value>|async move{
            let history=request["messages"].as_array().cloned().unwrap_or_default();
            let had_error=history.iter().any(|message|
                message["role"]=="tool" &&
                message["content"].as_str().is_some_and(|s|
                    s.contains("was not advertised")));
            Json(if had_error{
                mock_reply_final("I cannot perform an ungranted edit.")
            }else{
                mock_reply_tool("guess-write","workspace_write",
                    json!({"path":"forbidden.txt","content":"must not exist"}))
            })
        }
    ));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url=format!("http://{}/v1/chat/completions",listener.local_addr().unwrap());
    let server=tokio::spawn(async move{
        axum::serve(listener,model_app).await.unwrap()
    });
    // Unused valid loopback MCP address; MCP permissions are disabled.
    let (state,root)=fixture(url,"http://127.0.0.1:1/mcp".into()).await;
    let (task,_)=state.store.submit("no-write-2026",
        &json!({"prompt":"Answer without writing",
            "allowWrite":false,"allowMcp":false,
            "planningMode":"never","maxSteps":3,"tokenBudget":6000})).unwrap();
    let id=task["id"].as_str().unwrap().to_owned();
    run(state.clone(),id.clone()).await;
    assert_eq!(state.store.get(&id).unwrap()["status"],"completed");
    assert!(!root.join("forbidden.txt").exists());
    let observations=state.store.events_after(&id,0,100);
    assert!(observations.iter().any(|event|
        event["type"]=="tool_result"
        && event["detail"].to_string().contains("was not advertised")));
    assert!(!observations.iter().any(|event|event["type"]=="tool_start"),
        "a rejected model-tool alias must never become a pending external effect");
    server.abort();
    drop(state);
    let _=std::fs::remove_dir_all(root);
}
