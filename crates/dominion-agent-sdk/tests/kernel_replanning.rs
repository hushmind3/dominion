//! Deterministic integration tests for the public Rust kernel (no paid model calls).
use dominion_agent_sdk::{
    Action, BoxFuture, Evidence, Journal, Kernel, Limits, Model, ModelReply,
    Receipt, RunRequest, Snapshot, Status, Strategy, ToolSpec, Tools,
};
use serde_json::{json, Value};
use std::{collections::{HashSet, VecDeque}, sync::{Arc, Mutex}};

struct ScriptedModel { turns: Mutex<VecDeque<String>> }
impl ScriptedModel {
    fn new(turns: Vec<Value>) -> Self {
        Self {turns:Mutex::new(turns.into_iter().map(|item|item.to_string()).collect())}
    }
}
impl Model for ScriptedModel {
    fn complete<'a>(&'a self, _system:&'a str,_prompt:&'a str)
        -> BoxFuture<'a,Result<ModelReply,String>> {
        Box::pin(async move {
            let content=self.turns.lock().unwrap().pop_front()
                .ok_or("Scripted model received an unexpected extra call")?;
            Ok(ModelReply {content,input_tokens:Some(100),output_tokens:Some(50)})
        })
    }
}
struct RecordingTools {
    called: Arc<Mutex<Vec<String>>>,
    fail_ids: HashSet<String>,
}
impl RecordingTools {
    fn new(fail_ids:&[&str]) -> Self {
        Self {called:Arc::new(Mutex::new(Vec::new())),
            fail_ids:fail_ids.iter().map(|x|x.to_string()).collect()}
    }
}
impl Tools for RecordingTools {
    fn definitions(&self)->Vec<ToolSpec>{
        ["read","write","shell_exec"].into_iter().map(|name|ToolSpec {
            name:name.into(),description:name.into(),schema:json!({}),
            read_only:name=="read"
        }).collect()
    }
    fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
        Box::pin(async move {
            self.called.lock().unwrap().push(action.id.clone());
            if self.fail_ids.contains(&action.id) { Err("Simulated tool failure".into()) }
            else { Ok(json!({"ok":true,"id":action.id})) }
        })
    }
}
struct AlwaysTrueEvidence;
impl Evidence for AlwaysTrueEvidence {
    fn check<'a>(&'a self,_:&'a dominion_agent_sdk::Criterion)
        ->BoxFuture<'a,Result<bool,String>>{
        Box::pin(async move{Ok(true)})
    }
}
#[derive(Default)]
struct RecordingJournal{snapshots:Mutex<Vec<Snapshot>>}
impl Journal for RecordingJournal {
    fn save(&self,snapshot:&Snapshot)->Result<(),String>{
        self.snapshots.lock().unwrap().push(snapshot.clone());
        Ok(())
    }
}
fn request(id:&str)->RunRequest{
    RunRequest{id:id.into(),instruction:"Analyze the repository".into(),
        owner_instructions:"Follow the user's instruction".into(),
        strategy:Strategy::Balanced,limits:Limits {
            max_steps:8,max_model_calls:8,max_estimated_tokens:100_000,
            max_parallel_reads:4,max_result_chars:1000,
        },allowed_tools:vec!["read".into(),"write".into()],
        checks:vec![],require_evidence:true}
}
fn decide(actions:Value, answer:Option<&str>)->Value {
    json!({"plan":["inspect","repair","verify"],"actions":actions,"answer":answer})
}
fn op(id:&str,tool:&str,deps:&[&str])->Value {
    json!({"id":id,"tool":tool,"arguments":{},"dependsOn":deps})
}

#[tokio::test]
async fn failed_read_replans_and_never_dispatches_dependent_write(){
    let model=ScriptedModel::new(vec![
        decide(json!([op("inspect","read",&[]),op("modify","write",&["inspect"])]),None),
        decide(json!([op("inspect_again","read",&[])]),None),
        decide(json!([]),Some("Analyzed and verified")),
    ]);
    let tools=RecordingTools::new(&["inspect"]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let result=kernel.run(request("failure-repair")).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(*tools.called.lock().unwrap(),vec!["inspect","inspect_again"]);
    assert!(result.receipts.iter().any(|receipt|receipt.id=="inspect"&&!receipt.success));
    assert!(result.receipts.iter().all(|receipt|receipt.id!="modify"));
    let saved=journal.snapshots.lock().unwrap();
    assert!(saved.iter().any(|snapshot|!snapshot.in_flight.is_empty()));
    assert!(saved.iter().any(|snapshot|snapshot.receipts.iter().any(|r|r.id=="inspect"&&!r.success)));
}
#[tokio::test]
async fn failed_write_cannot_leak_later_write_from_same_plan(){
    let model=ScriptedModel::new(vec![
        decide(json!([op("first_write","write",&[]),op("dangerous_followup","write",&[])]),None),
        decide(json!([op("observe","read",&[])]),None),
        decide(json!([]),Some("Failure inspected")),
    ]);
    let tools=RecordingTools::new(&["first_write"]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("write-barrier");
    req.require_evidence=false;
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(*tools.called.lock().unwrap(),vec!["first_write","observe"]);
}
#[tokio::test]
async fn malformed_dependencies_never_hit_host_tools(){
    let model=ScriptedModel::new(vec![
        decide(json!([op("a","read",&["future"]),op("future","write",&[])]),None),
        decide(json!([op("corrected","read",&[])]),None),
        decide(json!([]),Some("Inspected")),
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let result=kernel.run(request("rejected-graph")).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(*tools.called.lock().unwrap(),vec!["corrected"]);
}

#[tokio::test]
async fn safe_checkpoint_resumes_without_replaying_prior_tool_effects(){
    let req=request("resume-clean");
    let mut saved=Snapshot::from_request(&req);
    saved.step=2;
    saved.model_calls=2;
    saved.charged_tokens=300;
    saved.receipts.push(Receipt {
        id:"already_done".into(),tool:"read".into(),arguments:json!({}),
        success:true,result:json!({"inspected":true}),step:1,
    });
    let model=ScriptedModel::new(vec![decide(json!([]),Some("Continued from receipt"))]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let result=kernel.resume(req,saved).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(result.step,3);
    assert_eq!(result.model_calls,3);
    assert_eq!(result.receipts.len(),1);
    assert!(tools.called.lock().unwrap().is_empty());
}
#[tokio::test]
async fn checkpoint_identity_and_unknown_effects_fail_closed(){
    let req=request("resume-validation");
    let model=ScriptedModel::new(vec![]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut wrong=Snapshot::from_request(&req);
    wrong.id="different-job".into();
    assert!(kernel.resume(req.clone(),wrong).await.is_err());
    let mut in_flight=Snapshot::from_request(&req);
    in_flight.in_flight.push(Action{id:"not-yet-confirmed".into(),tool:"write".into(),
        arguments:json!({}),depends_on:vec![]});
    assert!(kernel.resume(req.clone(),in_flight).await.is_err());
    let mut model_pending=Snapshot::from_request(&req);
    model_pending.model_in_flight=true;
    assert!(kernel.resume(req,model_pending).await.is_err());
    assert!(tools.called.lock().unwrap().is_empty());
    assert!(journal.snapshots.lock().unwrap().is_empty());
}

#[test]
fn request_length_matches_the_public_job_api_limit(){
    let mut req=request("payload-size-parity");
    req.instruction="x".repeat(64_000);
    assert!(req.validate().is_ok());
    req.instruction.push('x');
    assert!(req.validate().is_err());
}

#[tokio::test]
async fn command_success_is_exact_and_must_follow_last_edit() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit1","write",&[])]),None),
        decide(json!([json!({"id":"test_early","tool":"shell_exec",
            "arguments":{"command":"cargo test"},"dependsOn":[]})]),None),
        decide(json!([op("edit2","write",&[])]),None),
        decide(json!([]),Some("Done before last verification")),
        decide(json!([json!({"id":"test_late","tool":"shell_exec",
            "arguments":{"command":"cargo test"},"dependsOn":[]})]),None),
        decide(json!([]),Some("Verified after final edit")),
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("command-postcondition");
    req.allowed_tools.push("shell_exec".into());
    req.checks.push(dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into())
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(result.answer.as_deref(),Some("Verified after final edit"));
    assert_eq!(*tools.called.lock().unwrap(),
        vec!["edit1","test_early","edit2","test_late"]);
}

#[tokio::test]
async fn failed_or_different_verification_command_requires_repair() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit","write",&[])]),None),
        decide(json!([json!({"id":"wrong","tool":"shell_exec",
            "arguments":{"command":"echo all good"},"dependsOn":[]})]),None),
        decide(json!([]),Some("Not yet verified")),
        decide(json!([json!({"id":"failing","tool":"shell_exec",
            "arguments":{"command":"cargo test"},"dependsOn":[]})]),None),
        decide(json!([json!({"id":"passing","tool":"shell_exec",
            "arguments":{"command":"cargo test"},"dependsOn":[]})]),None),
        decide(json!([]),Some("Verified")),
    ]);
    let tools=RecordingTools::new(&["failing"]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("command-repair");
    req.allowed_tools.push("shell_exec".into());
    req.checks.push(dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into())
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(result.answer.as_deref(),Some("Verified"));
    assert!(result.receipts.iter().any(|r|r.id=="failing"&&!r.success));
    assert_eq!(tools.called.lock().unwrap().len(),4);
}
