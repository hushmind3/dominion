use dominion_agent_sdk::{select_named,ModelPort,ModelResponse,RunOptions,ToolRequest};
use serde_json::{json,Value};
use std::collections::VecDeque;
struct Model {
    replies: VecDeque<ModelResponse>,
    catalog_sizes: Vec<usize>,
}
impl ModelPort for Model {
    async fn complete(&mut self, _: &[Value], catalog: &[Value]) -> Result<ModelResponse, String> {
        self.catalog_sizes.push(catalog.len());
        self.replies.pop_front().ok_or_else(|| "unexpected model call".into())
    }
}

fn model(name: &str) -> Model {
    Model { replies: VecDeque::from([
        ModelResponse {content: String::new(), calls: vec![
            ToolRequest {id: "call-1".into(), name: name.into(), arguments: json!({})}
        ]},
        ModelResponse {content: "finished".into(), calls: vec![]}
    ]), catalog_sizes: vec![] }
}
fn enabled() -> RunOptions {
    RunOptions {tools_enabled: true, tool_budget: 2, ..RunOptions::default()}
}


#[tokio::test]
async fn standalone_sdk_connects_real_stdio_process_and_dispatches_through_harness(){
    use dominion_agent_sdk::extensions::ExtensionRegistry;
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture=root.join("scripts/mcp-echo-server.mjs").canonicalize().unwrap();
    let config=json!({"command":"node","args":[fixture]});
    let mut registry=ExtensionRegistry::new();
    let grants=dominion_mcp_adapter::register_mcp_stdio(&mut registry,"echo",&config,&root).await.unwrap();
    assert_eq!(grants,vec!["ext__echo__echo"]);
    let mut provider=model("ext__echo__echo");
    let mut history=vec![json!({"role":"user","content":"echo hello"})];
    let mut algorithm=select_named(Some("react"),false);
    let result=registry.execute(&mut algorithm,&mut provider,&mut history,&grants,enabled()).await.unwrap();
    assert_eq!(result.response,"finished");
    assert!(json!(history).to_string().contains("MCP_ECHO:"));
    assert!(registry.unregister("echo"));
    assert!(registry.names().is_empty());
}

#[tokio::test]
async fn standalone_sdk_negotiates_modern_stdio_without_legacy_initialize(){
    use dominion_agent_sdk::extensions::ExtensionRegistry;
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture=root.join("scripts/mcp-echo-server.mjs").canonicalize().unwrap();
    let config=json!({"command":"node","args":[fixture.to_string_lossy().as_ref(),"--modern"]});
    let mut registry=ExtensionRegistry::new();
    let grants=dominion_mcp_adapter::register_mcp_stdio(&mut registry,"modern",&config,&root).await.unwrap();
    let mut provider=model("ext__modern__echo");
    let mut history=vec![json!({"role":"user","content":"echo hello"})];
    let mut algorithm=select_named(Some("react"),false);
    let result=registry.execute(&mut algorithm,&mut provider,&mut history,&grants,enabled()).await.unwrap();
    assert_eq!(result.response,"finished");
    assert!(json!(history).to_string().contains("MCP_MODERN_ECHO:"));
    assert!(registry.unregister("modern"));
}

#[tokio::test]
async fn legacy_empty_discovery_reply_falls_back_only_when_auto_selected(){
    use dominion_mcp_adapter::StdioSession;
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture=root.join("scripts/mcp-echo-server.mjs").canonicalize().unwrap();
    let mut config=json!({"command":"node","args":[fixture.to_string_lossy().as_ref(),"--empty-discovery"]});
    let mut session=StdioSession::open(&config,&root).await.unwrap();
    let list=session.list_tools().await.unwrap();
    assert_eq!(list["tools"][0]["name"],"echo");
    let result=session.request("tools/call",json!({"name":"echo","arguments":{"text":"legacy"}})).await.unwrap();
    assert_eq!(result["content"][0]["text"],"MCP_ECHO: legacy");
    drop(session);
    config["protocolVersion"]=json!("2026-07-28");
    assert!(StdioSession::open(&config,&root).await.is_err());
}
