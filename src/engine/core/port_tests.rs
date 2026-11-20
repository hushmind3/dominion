use super::{run_with_events,ExecutionPort};
use serde_json::{json,Value};
use tokio::sync::mpsc;

struct FakePort { fail: bool }
impl ExecutionPort for FakePort {
    type Request = str;

    async fn execute(&self, input: &str,
        events: mpsc::UnboundedSender<Value>) -> Result<Option<Value>, String> {
        assert_eq!(input,"make a document");
        let _=events.send(json!({"type":"tool_end","id":"x","name":"document_write",
            "arguments":{"content":"done"},"result":{"ok":true}}));
        if self.fail {
            let _=events.send(json!({"type":"tool_error","id":"y","name":"external",
                "result":{"effectUnknown":true,"error":"pending"}}));
            Err("provider failed".to_owned())
        } else {Ok(Some(json!({"role":"assistant","content":"done"})))}
    }
}

#[tokio::test]
async fn standalone_host_port_executes_without_any_app_state_or_provider() {
    let (reply,stats)=run_with_events(&FakePort{fail:false},"make a document").await;
    assert_eq!(reply.unwrap().unwrap()["content"],"done");
    assert_eq!(stats.tool_success,vec!["document_write"]);
    assert!(stats.tool_errors.is_empty());
    assert_eq!(stats.evidence.len(),1);
}

#[tokio::test]
async fn failed_provider_preserves_success_and_uncertain_effect_receipts() {
    let (error,stats)=run_with_events(&FakePort{fail:true},"make a document").await;
    assert!(error.unwrap_err().contains("provider failed"));
    assert_eq!(stats.tool_success,vec!["document_write"]);
    assert_eq!(stats.tool_errors,vec!["external"]);
    assert_eq!(stats.evidence[1]["outcomeUnknown"],true);
}
