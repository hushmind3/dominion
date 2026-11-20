//! The smallest entrypoint: natural-language turn -> native execution -> result.
//! Providers, tools, memory, rooms, jobs and other product subsystems live
//! outside this module. Historical API paths are reexported by engine::mod.
pub mod driver;
pub mod agent_loop;
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Default, Clone)]
pub struct TurnStats {
    pub tool_success: Vec<String>,
    pub tool_errors: Vec<String>,
    pub evidence: Vec<Value>,
}

/// Keep both success receipts and failed effects for the SAME decision
/// kernel. Dropping tool_error meant a prior external effect marked unknown
/// could disappear from autonomous replay admission.
pub fn collect_tool_event(stats: &mut TurnStats, event: &Value) {
    let Some(name) = event.get("name").and_then(Value::as_str)
        .filter(|name| !name.is_empty()) else {return;};
    match event.get("type").and_then(Value::as_str) {
        Some("tool_end") => {
            stats.tool_success.push(name.to_string());
            stats.evidence.push(serde_json::json!({
                "tool": name, "callId": event.get("id"),
                "arguments": event.get("arguments"), "result": event.get("result"),
            }));
        }
        Some("tool_error") => {
            stats.tool_errors.push(name.to_string());
            let result = event.get("result").filter(|result| !result.is_null())
                .cloned().unwrap_or_else(||serde_json::json!({
                    "error":event.get("error").and_then(Value::as_str).unwrap_or("tool failed"),
                }));
            let uncertain = result["outcomeUnknown"]==true || result["effectUnknown"]==true
                || event["outcomeUnknown"]==true || event["effectUnknown"]==true;
            let mut receipt=serde_json::json!({
                "tool":name,"status":"failed","callId":event.get("id"),
                "arguments":event.get("arguments"),"result":result,
            });
            if uncertain {receipt["outcomeUnknown"]=serde_json::json!(true);}
            if event["dispatched"]==false {receipt["dispatched"]=serde_json::json!(false);}
            stats.evidence.push(receipt);
        }
        _ => {}
    }
}

/// Host ports implement all product work; core never imports AppState,
/// Chat, model providers, automation, memory, room logic or MCP.
pub trait ExecutionPort {
    type Request: ?Sized;
    async fn execute(&self, request: &Self::Request,
        events: mpsc::UnboundedSender<Value>) -> Result<Option<Value>, String>;
}

/// The only generic owner-to-result boundary. Evidence collection remains
/// active on both success and failure; no operation is declared successful
/// merely because the model produced text.
pub async fn run_with_events<P: ExecutionPort>(
    port: &P, request: &P::Request,
) -> (Result<Option<Value>, String>, TurnStats) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let collector = tokio::spawn(async move {
        let mut stats = TurnStats::default();
        while let Some(event) = receiver.recv().await {
            collect_tool_event(&mut stats, &event);
        }
        stats
    });
    let result = port.execute(request, sender).await;
    let stats = collector.await.unwrap_or_default();
    (result, stats)
}

#[cfg(test)]
mod port_tests;
