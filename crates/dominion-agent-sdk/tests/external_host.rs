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
