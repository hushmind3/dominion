use dominion_agent_sdk::{
    execute, select_named, AgentLoop, ModelPort, ModelResponse, RoundObservation,
    RunOptions, Step, ToolPort, ToolRequest,
};
use serde_json::{json, Value};
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

#[derive(Default)]
struct Host { calls: Vec<String>, uncertain: bool }
impl ToolPort for Host {
    async fn dispatch(&mut self, call: &ToolRequest) -> Result<Value, String> {
        self.calls.push(call.name.clone());
        if self.uncertain { return Err("connection lost".into()); }
        Ok(json!({"text": "external host receipt"}))
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
fn catalog() -> Vec<Value> {
    vec![json!({"type":"function","function":{"name":"read"}})]
}
fn enabled() -> RunOptions {
    RunOptions {tools_enabled: true, tool_budget: 2, ..RunOptions::default()}
}

#[tokio::test]
async fn independent_host_runs_both_builtin_workflows() {
    for name in ["react", "plan-execute"] {
        let mut algorithm = select_named(Some(name), false);
        let mut provider = model("read");
        let mut host = Host::default();
        let mut history = vec![json!({"role":"user","content":"read"})];
        let result = execute(&mut algorithm, &mut provider, &mut host,
            &mut history, &catalog(), enabled()).await.unwrap();
        assert_eq!(result.workflow, name);
        assert_eq!(result.response, "finished");
        assert_eq!(host.calls, vec!["read"]);
        assert_eq!(result.model_rounds, 2);
        assert_eq!(history[2]["tool_call_id"], "call-1");
    }
}

struct ExternalStrategy;
impl AgentLoop for ExternalStrategy {
    fn name(&self) -> &'static str { "external" }
    fn after_model(&mut self, observation: RoundObservation) -> Step {
        if observation.proposed_calls == 0 { Step::Answer }
        else { Step::Execute {calls: observation.proposed_calls} }
    }
}

#[tokio::test]
async fn external_strategy_cannot_dispatch_hidden_tools() {
    let mut host = Host::default();
    let error = execute(&mut ExternalStrategy, &mut model("hidden"), &mut host,
        &mut vec![], &catalog(), enabled()).await.unwrap_err();
    assert!(error.contains("not advertised"));
    assert!(host.calls.is_empty());
}
#[tokio::test]
async fn tools_are_disabled_and_hidden_by_default() {
    let mut provider = model("read");
    let mut host = Host::default();
    assert!(execute(&mut ExternalStrategy, &mut provider, &mut host,
        &mut vec![], &catalog(), RunOptions::default()).await.is_err());
    assert_eq!(provider.catalog_sizes, vec![0]);
    assert!(host.calls.is_empty());
}
#[tokio::test]
async fn uncertain_effect_is_preserved_without_replay() {
    let mut host = Host {uncertain: true, ..Host::default()};
    let mut history = vec![];
    let error = execute(&mut ExternalStrategy, &mut model("read"), &mut host,
        &mut history, &catalog(), enabled()).await.unwrap_err();
    assert_eq!(error, "connection lost");
    assert_eq!(host.calls.len(), 1);
    let receipt: Value = serde_json::from_str(history[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(receipt["outcomeUnknown"], true);
}

#[tokio::test]
async fn compacted_sdk_requests_do_not_mutate_original_tool_receipts() {
    use dominion_agent_sdk::execute_compacted;
    struct InspectModel { wire: Vec<Value> }
    impl ModelPort for InspectModel {
        async fn complete(&mut self, history:&[Value],_:&[Value])->Result<ModelResponse,String>{
            self.wire=history.to_vec();
            Ok(ModelResponse{content:"done".into(),calls:vec![]})
        }
    }
    let long="LARGE_OLD_RECEIPT".repeat(4000);
    let mut history=vec![
        json!({"role":"user","content":"original goal"}),
        json!({"role":"assistant","tool_calls":[{"id":"old","type":"function","function":{"name":"read","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":"old","content":long}),
        json!({"role":"user","content":"continue"}),
    ];
    let original=history.clone();
    let mut provider=InspectModel{wire:vec![]};
    let result=execute_compacted(&mut ExternalStrategy,&mut provider,&mut Host::default(),
        &mut history,&catalog(),RunOptions::default(),2048).await.unwrap();
    assert_eq!(result.response,"done");
    assert_eq!(&history[..original.len()],original.as_slice());
    assert!(provider.wire.iter().any(|row|row["_dominion_trace_compacted"]==true));
    assert!(json!(provider.wire).to_string().len()<3000);
    assert_eq!(provider.wire[0]["content"],"original goal");
}

#[tokio::test]
async fn extension_registration_executes_only_granted_tools_and_supports_removal(){
    use dominion_agent_sdk::extensions::{ExtensionRegistry,ExtensionTool,ToolRegistration,ToolFuture};
    use std::{cell::Cell,rc::Rc};
    struct Count(Rc<Cell<usize>>);
    impl ExtensionTool for Count{
        fn call<'a>(&'a mut self,_:&'a Value)->ToolFuture<'a>{
            Box::pin(async move{
                self.0.set(self.0.get()+1);
                Ok(json!({"ok":true,"count":self.0.get()}))
            })
        }
    }
    let count=Rc::new(Cell::new(0));
    let tool=|name:&str|ToolRegistration{name:name.into(),description:"count".into(),
        parameters:json!({"type":"object","properties":{}}),
        handler:Box::new(Count(count.clone()))};
    let mut registry=ExtensionRegistry::new();
    assert!(registry.register("test",vec![tool("read"),tool("read")]).is_err());
    assert!(registry.names().is_empty(),"failed registration must be atomic");
    registry.register("test",vec![tool("read"),tool("write")]).unwrap();
    assert!(registry.register("test",vec![tool("other")]).is_err());
    let read=String::from("ext__test__read");
    let grants=vec![read.clone()];
    let mut algorithm=select_named(Some("react"),false);
    let mut provider=model("ext__test__write");
    assert!(registry.execute(&mut algorithm,&mut provider,&mut vec![],&grants,enabled()).await.is_err());
    assert_eq!(count.get(),0);
    let mut provider=model(&read);
    let mut history=vec![];
    let outcome=registry.execute_compacted(&mut algorithm,&mut provider,&mut history,&grants,enabled(),2048).await.unwrap();
    assert_eq!(outcome.response,"finished");
    assert_eq!(count.get(),1);
    assert_eq!(provider.catalog_sizes,vec![1,1]);
    assert!(registry.unregister("test"));
    assert!(registry.names().is_empty());
    let mut provider=model(&read);
    assert!(registry.execute(&mut algorithm,&mut provider,&mut vec![],&grants,enabled()).await.is_err());
    assert!(provider.catalog_sizes.is_empty(),"stale grant rejected before model request");
    assert_eq!(count.get(),1);
}

#[tokio::test]
async fn standalone_sdk_connects_real_stdio_process_and_dispatches_through_harness(){
    use dominion_agent_sdk::extensions::ExtensionRegistry;
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture=root.join("scripts/mcp-echo-server.mjs").canonicalize().unwrap();
    let config=json!({"command":"node","args":[fixture]});
    let mut registry=ExtensionRegistry::new();
    let grants=registry.register_mcp_stdio("echo",&config,&root).await.unwrap();
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
    let grants=registry.register_mcp_stdio("modern",&config,&root).await.unwrap();
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
    use dominion_agent_sdk::mcp_stdio::StdioSession;
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
