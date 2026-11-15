//! End-to-end Rust kernel test of history projection + model-visible recall.
//! The host has no external tools; both recall operations execute inside the
//! kernel against an isolated durable run snapshot.
use dominion_agent_sdk::{
    Action, BoxFuture, Evidence, Journal, Kernel, Limits, Model, ModelReply,
    Receipt, RunRequest, Snapshot, Status, Strategy, ToolSpec, Tools,
};
use serde_json::{json,Value};
use std::{collections::VecDeque,sync::Mutex};

struct Scripted {
    decisions: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<Value>>,
    systems: Mutex<Vec<String>>,
}
impl Scripted {
    fn new(decisions:Vec<Value>)->Self {
        Self{decisions:Mutex::new(decisions.into_iter().map(|v|v.to_string()).collect()),
            prompts:Mutex::new(vec![]),systems:Mutex::new(vec![])}
    }
}
impl Model for Scripted {
    fn complete<'a>(&'a self,system:&'a str,prompt:&'a str)
        -> BoxFuture<'a,Result<ModelReply,String>> {
        Box::pin(async move {
            self.systems.lock().unwrap().push(system.to_string());
            self.prompts.lock().unwrap().push(serde_json::from_str(prompt)
                .map_err(|e:serde_json::Error|e.to_string())?);
            let content=self.decisions.lock().unwrap().pop_front().ok_or("No more turns")?;
            Ok(ModelReply{content,input_tokens:Some(200),output_tokens:Some(60)})
        })
    }
}
struct ForbiddenHost;
impl Tools for ForbiddenHost {
    fn definitions(&self)->Vec<ToolSpec>{vec![]}
    fn execute<'a>(&'a self,_:&'a Action)->BoxFuture<'a,Result<Value,String>>{
        Box::pin(async {Err("Host must never see kernel-local receipt tool".into())})
    }
}
struct EvidenceNotNeeded;
impl Evidence for EvidenceNotNeeded {
    fn check<'a>(&'a self,_:&'a dominion_agent_sdk::Criterion)
        ->BoxFuture<'a,Result<bool,String>> {
        Box::pin(async {Err("No file checks needed".into())})
    }
}
#[derive(Default)]
struct Durable {history:Mutex<Vec<Snapshot>>}
impl Journal for Durable {
    fn save(&self,run:&Snapshot)->Result<(),String>{
        self.history.lock().unwrap().push(run.clone());Ok(())
    }
}
fn request()->RunRequest {
    RunRequest{
        id:"long-context".into(),instruction:"Recall initial error, no external operations".into(),
        owner_instructions:"Keep all exact evidence available".into(),
        strategy:Strategy::Balanced,limits:Limits{
            max_steps:64,max_model_calls:64,max_estimated_tokens:1_000_000,
            max_parallel_reads:4,max_result_chars:1200,
        },allowed_tools:vec![],checks:vec![],require_evidence:false}
}
fn action(id:&str,tool:&str,arguments:Value)->Value {
    json!({"id":id,"tool":tool,"arguments":arguments,"dependsOn":[]})
}
fn turn(actions:Vec<Value>,answer:Option<&str>)->Value {
    json!({"plan":["Search old evidence and retrieve it"],"actions":actions,"answer":answer})
}

#[tokio::test]
async fn a_long_run_can_search_and_read_evicted_tool_evidence_without_reexecution() {
    let request=request();
    let mut saved=Snapshot::from_request(&request);
    saved.step=30;
    saved.model_calls=30;
    saved.charged_tokens=5000;
    saved.receipts.push(Receipt{id:"failure-from-step-one".into(),
        tool:"shell_exec".into(),arguments:json!({"command":"cargo test"}),
        success:false,result:json!({"stderr":"EARLY_DIAGNOSTIC: trait bound not satisfied",
            "full":"X".repeat(10_000)}),step:1});
    for index in 0..230 {
        saved.receipts.push(Receipt{id:format!("read-{index}"),tool:"workspace_read".into(),
            arguments:json!({"path":format!("src/file{index}.rs")}),
            success:true,result:json!({"content":"source".repeat(900)}),
            step:index/8+2});
    }
    let model=Scripted::new(vec![
        turn(vec![action("find","dominion_receipt_search",json!({
            "query":"EARLY_DIAGNOSTIC","limit":5
        }))],None),
        turn(vec![action("read","dominion_receipt_read",json!({
            "id":"failure-from-step-one","startChar":9500,"maxChars":1000
        }))],None),
        turn(vec![],Some("Recovered the early diagnostic without rerunning tools")),
    ]);
    let host=ForbiddenHost;
    let journal=Durable::default();
    let kernel=Kernel{model:&model,tools:&host,evidence:&EvidenceNotNeeded,journal:&journal};
    let result=kernel.resume(request,saved).await.unwrap();
    assert!(matches!(result.status,Status::Completed),"{result:?}");
    assert!(result.receipts.iter().any(|r|r.id=="find"&&r.success));
    assert!(result.receipts.iter().any(|r|r.id=="read"&&r.success));
    let search=result.receipts.iter().find(|r|r.id=="find").unwrap();
    assert_eq!(search.result["matches"][0]["id"],"failure-from-step-one");
    let read=result.receipts.iter().find(|r|r.id=="read").unwrap();
    assert_eq!(read.result["id"],"failure-from-step-one");
    assert!(read.result["text"].as_str().unwrap().contains("EARLY_DIAGNOSTIC"));
    let prompts=model.prompts.lock().unwrap();
    assert_eq!(prompts.len(),3);
    assert!(prompts[0]["importantPastFailures"].to_string().contains("trait bound"));
    assert!(prompts[0]["executionDigest"]["olderObservationsOmitted"].as_u64().unwrap()>0);
    assert!(prompts[1]["recentObservations"].to_string().contains("failure-from-step-one"));
    assert!(prompts[2]["recentObservations"].to_string().contains("EARLY_DIAGNOSTIC"));
    assert!(model.systems.lock().unwrap()[0].contains("dominion_receipt_search"));
    // Long source outputs must not be resent on every model call.
    for prompt in prompts.iter() {
        assert!(prompt.to_string().chars().count()<30_000);
    }
    assert_eq!(result.receipts.len(),233);
    assert!(journal.history.lock().unwrap().last().unwrap().in_flight.is_empty());
}

#[tokio::test]
async fn invalid_receipt_request_cannot_escape_to_external_host() {
    let model=Scripted::new(vec![
        turn(vec![action("invalid","dominion_receipt_read",json!({
            "id":"not-in-this-run"
        }))],None),
        turn(vec![],Some("Reported missing record")),
    ]);
    let host=ForbiddenHost;
    let journal=Durable::default();
    let kernel=Kernel{model:&model,tools:&host,evidence:&EvidenceNotNeeded,journal:&journal};
    let result=kernel.run(request()).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert!(result.receipts.iter().any(|r|r.id=="invalid"&&!r.success));
}
