//! Deterministic integration tests for the public Rust kernel (no paid model calls).
use dominion_agent_sdk::{
    Action, BoxFuture, Evidence, Journal, Kernel, Limits, Model, ModelReply,
    Receipt, RunRequest, Snapshot, Status, Strategy, ToolSpec, Tools,
};
use serde_json::{json, Value};
use std::{collections::{HashSet, VecDeque}, sync::{Arc, Mutex}};

struct ScriptedModel { turns: Mutex<VecDeque<String>>, prompts: Mutex<Vec<String>> }
impl ScriptedModel {
    fn new(turns: Vec<Value>) -> Self {
        Self {turns:Mutex::new(turns.into_iter().map(|item|item.to_string()).collect()),
            prompts:Mutex::new(Vec::new())}
    }
}
impl Model for ScriptedModel {
    fn complete<'a>(&'a self, _system:&'a str,prompt:&'a str)
        -> BoxFuture<'a,Result<ModelReply,String>> {
        Box::pin(async move {
            self.prompts.lock().unwrap().push(prompt.to_string());
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
            max_steps:8,max_model_calls:8,max_tool_calls:96,max_estimated_tokens:100_000,
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

/// Reject ambiguous tool catalogues before making a paid model request.
struct MalformedCatalogue { names: Vec<&'static str> }
impl Tools for MalformedCatalogue {
    fn definitions(&self)->Vec<ToolSpec>{
        self.names.iter().map(|name|ToolSpec{
            name:(*name).into(),description:"host tool".into(),
            schema:json!({"type":"object"}),read_only:true,
        }).collect()
    }
    fn execute<'a>(&'a self,_:&'a Action)->BoxFuture<'a,Result<Value,String>>{
        Box::pin(async move{panic!("Untrusted duplicate/reserved tool must never run")})
    }
}

#[tokio::test]
async fn duplicate_or_reserved_host_tool_names_never_reach_model_or_dispatch(){
    for (id,names,expected) in [
        ("duplicate-catalog",vec!["read","read"],"duplicate"),
        ("internal-shadow",vec!["dominion_receipt_read"],"Reserved"),
    ] {
        let model=ScriptedModel::new(vec![]);
        let tools=MalformedCatalogue{names};
        let journal=RecordingJournal::default();
        let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
        let mut req=request(id);
        req.allowed_tools.push("dominion_receipt_read".into());
        let err=kernel.run(req).await.err().expect("bad catalogue must be rejected");
        assert!(err.contains(expected),"{err}");
        assert!(journal.snapshots.lock().unwrap().is_empty(),
            "must reject before any model/tool checkpoint");
        assert!(model.prompts.lock().unwrap().is_empty());
    }
}

/// A provider can report many more tokens than the cheap preflight guess.
/// Such a paid response must not authorize any external side effect.
struct ExpensiveProvider {
    calls: std::sync::atomic::AtomicUsize,
}
impl Model for ExpensiveProvider {
    fn complete<'a>(&'a self,_:&'a str,_:&'a str)
        -> BoxFuture<'a,Result<ModelReply,String>> {
        Box::pin(async move {
            self.calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            Ok(ModelReply {
                content:decide(json!([op("unauthorized-overbudget","write",&[])]),None).to_string(),
                input_tokens:Some(10_000),output_tokens:Some(9_000),
            })
        })
    }
}

#[tokio::test]
async fn provider_reported_token_overrun_cannot_dispatch_any_tool_effect(){
    let model=ExpensiveProvider{calls:std::sync::atomic::AtomicUsize::new(0)};
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("token-overshoot");
    req.limits.max_estimated_tokens=10_000;
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview),"{result:?}");
    assert_eq!(result.charged_tokens,19_000,"actual usage stays auditable");
    assert_eq!(result.model_calls,1);
    assert!(result.receipts.is_empty());
    assert!(result.decision_tape.is_empty(),"an over-budget reply is not accepted as a plan");
    assert_eq!(model.calls.load(std::sync::atomic::Ordering::SeqCst),1);
    assert!(tools.called.lock().unwrap().is_empty());
    let saved=journal.snapshots.lock().unwrap();
    assert!(matches!(saved.last().unwrap().status,Status::NeedsReview));
    assert!(!saved.last().unwrap().model_in_flight);
}

#[tokio::test]
async fn failed_receipt_guides_next_turn_without_new_dispatch_authority(){
    struct MissingPathTools;
    impl Tools for MissingPathTools{
        fn definitions(&self)->Vec<ToolSpec>{
            ["workspace_read","workspace_list"].iter().map(|name|ToolSpec{
                name:(*name).into(),description:(*name).into(),
                schema:json!({}),read_only:true,
            }).collect()
        }
        fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
            Box::pin(async move{
                if action.id=="missing"{
                    Err("No such file or directory: missing.rs".into())
                }else{Ok(json!({"path":"src/lib.rs"}))}
            })
        }
    }
    let model=ScriptedModel::new(vec![
        decide(json!([op("missing","workspace_read",&[])]),None),
        decide(json!([op("locate","workspace_list",&[])]),None),
        decide(json!([]),Some("Located src/lib.rs")),
    ]);
    let tools=MissingPathTools;
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,
        evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("failure-probe");
    req.allowed_tools=vec!["workspace_read".into(),"workspace_list".into()];
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(result.model_calls,3);
    assert_eq!(result.receipts.len(),2);
    let prompts=model.prompts.lock().unwrap();
    let after:Value=serde_json::from_str(&prompts[1]).unwrap();
    assert_eq!(after["latestFailureDiagnosis"]["observedFailure"],"missing_path");
    assert_eq!(after["latestFailureDiagnosis"]["evidenceReceiptId"],"missing");
    assert!(after["latestFailureDiagnosis"]["suggestedNextInspection"]
        .as_str().unwrap().contains("workspace"));
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
    assert_eq!(result.answer.as_deref(),Some("Done before last verification"));
    assert_eq!(result.model_calls,4);
    assert_eq!(*tools.called.lock().unwrap(),
        vec!["edit1","test_early","edit2","kernel-verify-4-0"]);
}

#[tokio::test]
async fn failed_autonomous_test_is_passed_to_model_then_rerun_after_repair() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit","write",&[])]),None),
        decide(json!([]),Some("Ready for verification")),
        decide(json!([op("repair","write",&[])]),None),
        decide(json!([]),Some("Verified after repair")),
    ]);
    // Fail the first kernel-controlled run; it must not be blindly rerun
    // until the model has produced an actual repair action.
    let tools=RecordingTools::new(&["kernel-verify-2-0"]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("autonomous-repair");
    req.allowed_tools.push("shell_exec".into());
    req.checks.push(dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into())
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(result.answer.as_deref(),Some("Verified after repair"));
    assert_eq!(result.model_calls,4);
    assert_eq!(*tools.called.lock().unwrap(),
        vec!["edit","kernel-verify-2-0","repair","kernel-verify-4-0"]);
    assert!(result.receipts.iter().any(|r|r.id=="kernel-verify-2-0"&&!r.success));
    assert!(result.receipts.iter().any(|r|r.id=="kernel-verify-4-0"&&r.success));
    let prompts=model.prompts.lock().unwrap();
    assert!(prompts[2].contains("Simulated tool failure"));
    assert!(prompts[2].contains("kernel-verify-2-0"));
    drop(prompts);
    let persisted=journal.snapshots.lock().unwrap();
    assert!(persisted.iter().any(|snap|snap.in_flight.iter().any(|a|
        a.id.starts_with("kernel-verify-"))));
    assert!(persisted.last().unwrap().in_flight.is_empty());
}

#[tokio::test]
async fn no_repair_means_no_blind_test_retry_and_needs_review() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit","write",&[])]),None),
        decide(json!([]),Some("Claim complete")),
        decide(json!([]),Some("Claim complete again")),
    ]);
    let tools=RecordingTools::new(&["kernel-verify-2-0"]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("no-repair");
    req.allowed_tools.push("shell_exec".into());
    req.checks.push(dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into())
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert_eq!(*tools.called.lock().unwrap(),vec!["edit","kernel-verify-2-0"]);
    assert!(result.error.unwrap_or_default().contains("no intervening repair"));
}

#[tokio::test]
async fn two_required_commands_run_without_extra_planning_turns() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit","write",&[])]),None),
        decide(json!([]),Some("All checks passed"))
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("two-checks");
    req.allowed_tools.push("shell_exec".into());
    for command in ["cargo test","cargo clippy"] {
        req.checks.push(dominion_agent_sdk::Criterion{
            kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
            path:None,contains:Some(command.into()),tool:Some("shell_exec".into())
        });
    }
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(result.model_calls,2);
    assert!(result.checks.iter().all(|check|check.satisfied));
    assert_eq!(*tools.called.lock().unwrap(),
        vec!["edit","kernel-verify-2-0","kernel-verify-2-1"]);
}

#[tokio::test]
async fn missing_verification_permission_never_runs_a_command() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit","write",&[])]),None),
        decide(json!([]),Some("Ready")),
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("no-command-permission");
    req.checks.push(dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into())
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert_eq!(*tools.called.lock().unwrap(),vec!["edit"]);
}

#[tokio::test]
async fn passing_test_is_not_proof_that_a_requested_edit_occurred() {
    let model=ScriptedModel::new(vec![
        decide(json!([]),Some("I implemented the feature")),
        decide(json!([]),Some("I implemented the feature")),
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("test-not-edit-proof");
    req.instruction="구현을 완료해라".into();
    req.allowed_tools.push("shell_exec".into());
    req.limits.max_steps=2;
    req.checks.push(dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into())
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert!(tools.called.lock().unwrap().is_empty(),
        "Never run expensive user checks when no file was actually modified");
}

struct FailingIntentJournal {
    snapshots: Mutex<Vec<Snapshot>>,
}
impl Journal for FailingIntentJournal {
    fn save(&self,snapshot:&Snapshot)->Result<(),String>{
        if snapshot.in_flight.iter().any(|a|a.id.starts_with("kernel-verify-")) {
            return Err("Persistence failed before command dispatch".into());
        }
        self.snapshots.lock().unwrap().push(snapshot.clone());
        Ok(())
    }
}
#[tokio::test]
async fn write_ahead_failure_blocks_automatic_command_dispatch() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit","write",&[])]),None),
        decide(json!([]),Some("Ready"))
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=FailingIntentJournal{snapshots:Mutex::new(Vec::new())};
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("write-ahead-guard");
    req.allowed_tools.push("shell_exec".into());
    req.checks.push(dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into())
    });
    assert!(kernel.run(req).await.is_err());
    assert_eq!(*tools.called.lock().unwrap(),vec!["edit"]);
    assert!(journal.snapshots.lock().unwrap().last().unwrap().in_flight.is_empty());
}

#[tokio::test]
async fn clean_resume_reuses_passed_acceptance_without_repeating_shell_effect() {
    let mut req=request("resume-verified");
    req.allowed_tools.push("shell_exec".into());
    req.instruction="구현하세요".into();
    req.checks.push(dominion_agent_sdk::Criterion {
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into()),
    });
    let mut saved=Snapshot::from_request(&req);
    saved.step=1;
    saved.model_calls=1;
    saved.receipts.push(Receipt {
        id:"edit".into(),tool:"write".into(),arguments:json!({}),
        success:true,result:json!({"written":true}),step:1,
    });
    saved.receipts.push(Receipt {
        id:"kernel-verify-1-0".into(),tool:"shell_exec".into(),
        arguments:json!({"command":"cargo test"}),
        success:true,result:json!({"exitCode":0}),step:1,
    });
    let model=ScriptedModel::new(vec![decide(json!([]),Some("Recovered verified result"))]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let result=kernel.resume(req,saved).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert!(tools.called.lock().unwrap().is_empty(),"Prior test must not run twice");
    assert_eq!(result.model_calls,2);
}

#[tokio::test]
async fn unmet_file_criterion_blocks_unnecessary_shell_verification() {
    struct NeverEvidence;
    impl Evidence for NeverEvidence {
        fn check<'a>(&'a self,_:&'a dominion_agent_sdk::Criterion)
            ->BoxFuture<'a,Result<bool,String>>{
            Box::pin(async move{Ok(false)})
        }
    }
    let model=ScriptedModel::new(vec![
        decide(json!([op("edit","write",&[])]),None),
        decide(json!([]),Some("All files ready")),
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&NeverEvidence,journal:&journal};
    let mut req=request("cheap-checks-first");
    req.limits.max_steps=2;
    req.allowed_tools.push("shell_exec".into());
    req.checks.push(dominion_agent_sdk::Criterion {
        kind:dominion_agent_sdk::CriterionKind::FileContains,
        path:Some("src/lib.rs".into()),contains:Some("new_fn".into()),tool:None,
    });
    req.checks.push(dominion_agent_sdk::Criterion {
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into()),
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert_eq!(*tools.called.lock().unwrap(),vec!["edit"]);
}

#[tokio::test]
async fn tool_budget_rejects_entire_batch_before_side_effects(){
    let model=ScriptedModel::new(vec![decide(json!([
        op("first","write",&[]),op("second","write",&[])
    ]),None)]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("bounded-batch");
    req.limits.max_tool_calls=1;
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert!(result.receipts.is_empty(),"over-budget batch must have no partial effects");
    assert!(tools.called.lock().unwrap().is_empty());
}
#[tokio::test]
async fn owner_verification_uses_same_tool_budget_as_model_actions(){
    let model=ScriptedModel::new(vec![
        decide(json!([op("read_1","read",&[])]),None),
        decide(json!([]),Some("Observed")),
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RecordingJournal::default();
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("bounded-verification");
    req.limits.max_tool_calls=1;
    req.allowed_tools.push("shell_exec".into());
    req.checks=vec![dominion_agent_sdk::Criterion{
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),
        tool:Some("shell_exec".into())}];
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert_eq!(*tools.called.lock().unwrap(),vec!["read_1"]);
    assert!(!result.receipts.iter().any(|receipt|receipt.tool=="shell_exec"));
}

#[derive(Clone, Copy)]
enum InterruptFence { Model, Tool, Verification }

struct InterruptAtPersistedFence {
    fence: InterruptFence,
    cancelled: std::sync::atomic::AtomicBool,
    snapshots: Mutex<Vec<Snapshot>>,
}
impl InterruptAtPersistedFence {
    fn new(fence: InterruptFence) -> Self {
        Self {fence,cancelled:std::sync::atomic::AtomicBool::new(false),
            snapshots:Mutex::new(Vec::new())}
    }
}
impl Journal for InterruptAtPersistedFence {
    fn save(&self,snapshot:&Snapshot)->Result<(),String> {
        self.snapshots.lock().unwrap().push(snapshot.clone());
        let reached=match self.fence {
            InterruptFence::Model => snapshot.model_in_flight,
            InterruptFence::Tool => snapshot.in_flight.iter().any(|a|a.tool=="read"),
            InterruptFence::Verification => snapshot.in_flight.iter()
                .any(|a|a.id.starts_with("kernel-verify-")),
        };
        if reached {
            self.cancelled.store(true,std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
    fn cancelled(&self,_id:&str)->bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[tokio::test]
async fn cancellation_at_model_write_ahead_fence_never_calls_provider() {
    let model=ScriptedModel::new(vec![]);
    let tools=RecordingTools::new(&[]);
    let journal=InterruptAtPersistedFence::new(InterruptFence::Model);
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let result=kernel.run(request("model-fence-cancel")).await.unwrap();
    assert!(matches!(result.status,Status::Cancelled));
    assert_eq!(result.model_calls,0);
    assert!(!result.model_in_flight);
    assert!(model.prompts.lock().unwrap().is_empty(),
        "No paid provider generation after owner cancellation");
    assert!(tools.called.lock().unwrap().is_empty());
    assert!(!journal.snapshots.lock().unwrap().last().unwrap().model_in_flight);
}

#[tokio::test]
async fn cancellation_at_tool_write_ahead_fence_never_executes_action() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("read_once","read",&[])]),None)
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=InterruptAtPersistedFence::new(InterruptFence::Tool);
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let result=kernel.run(request("tool-fence-cancel")).await.unwrap();
    assert!(matches!(result.status,Status::Cancelled));
    assert_eq!(result.model_calls,1);
    assert!(result.receipts.is_empty());
    assert!(result.in_flight.is_empty());
    assert!(tools.called.lock().unwrap().is_empty(),
        "Persisting an intent must not authorize a stale tool call");
    assert!(journal.snapshots.lock().unwrap().last().unwrap().in_flight.is_empty());
}

#[tokio::test]
async fn cancellation_at_owner_check_write_ahead_fence_skips_command() {
    let model=ScriptedModel::new(vec![decide(json!([]),Some("Inspected"))]);
    let tools=RecordingTools::new(&[]);
    let journal=InterruptAtPersistedFence::new(InterruptFence::Verification);
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("verification-fence-cancel");
    req.allowed_tools.push("shell_exec".into());
    req.checks.push(dominion_agent_sdk::Criterion {
        kind:dominion_agent_sdk::CriterionKind::ToolSucceeded,
        path:None,contains:Some("cargo test".into()),tool:Some("shell_exec".into()),
    });
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Cancelled));
    assert!(result.receipts.is_empty());
    assert!(result.in_flight.is_empty());
    assert!(tools.called.lock().unwrap().is_empty(),
        "Owner checks cannot run after cancellation at durable checkpoint");
    assert!(journal.snapshots.lock().unwrap().last().unwrap().in_flight.is_empty());
}

struct RedirectAtToolFence {
    redirected: std::sync::atomic::AtomicBool,
    snapshots: Mutex<Vec<Snapshot>>,
}
impl Journal for RedirectAtToolFence {
    fn save(&self,snapshot:&Snapshot)->Result<(),String> {
        self.snapshots.lock().unwrap().push(snapshot.clone());
        if snapshot.in_flight.iter().any(|a|a.tool=="read") {
            self.redirected.store(true,std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
    fn directive(&self,_id:&str)->Result<Option<String>,String> {
        Ok(self.redirected.load(std::sync::atomic::Ordering::SeqCst)
            .then(||"Inspect only, do not run the prior tool".into()))
    }
}
#[tokio::test]
async fn owner_correction_at_tool_fence_replans_without_stale_execution() {
    let model=ScriptedModel::new(vec![
        decide(json!([op("stale-read","read",&[])]),None),
        decide(json!([]),Some("Updated owner instruction followed")),
    ]);
    let tools=RecordingTools::new(&[]);
    let journal=RedirectAtToolFence {
        redirected:std::sync::atomic::AtomicBool::new(false),
        snapshots:Mutex::new(Vec::new()),
    };
    let kernel=Kernel{model:&model,tools:&tools,evidence:&AlwaysTrueEvidence,journal:&journal};
    let mut req=request("tool-fence-redirect");
    req.require_evidence=false;
    let result=kernel.run(req).await.unwrap();
    assert!(matches!(result.status,Status::Completed),"{result:?}");
    assert_eq!(result.model_calls,2);
    assert!(tools.called.lock().unwrap().is_empty(),
        "Changing the owner directive during checkpoint save cancels the stale batch");
    assert!(result.in_flight.is_empty());
    let prompts=model.prompts.lock().unwrap();
    assert_eq!(prompts.len(),2);
    assert!(prompts[1].contains("Inspect only, do not run the prior tool"));
    assert!(journal.snapshots.lock().unwrap().last().unwrap().in_flight.is_empty());
}
