use dominion_agent_sdk::*;
use serde_json::{json, Value};
use std::{collections::VecDeque, path::PathBuf, sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

struct Responses(Mutex<VecDeque<String>>);
impl Model for Responses {
    fn complete<'a>(&'a self, _system: &'a str, _prompt: &'a str) -> BoxFuture<'a, Result<ModelReply, String>> {
        Box::pin(async move {
            self.0.lock().unwrap().pop_front()
                .map(|content| ModelReply { content, input_tokens: Some(100), output_tokens: Some(20) })
                .ok_or_else(|| "model out of responses".into())
        })
    }
}
struct Files {
    root: PathBuf,
    writes: AtomicUsize,
}
impl Tools for Files {
    fn definitions(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {name:"write".into(), description:"Write an artifact".into(), schema:json!({}),read_only:false},
            ToolSpec {name:"read".into(),description:"Read a file".into(),schema:json!({}),read_only:true},
        ]
    }
    fn execute<'a>(&'a self, action:&'a Action)->BoxFuture<'a,Result<Value,String>> {
        Box::pin(async move {
            let name=action.arguments["path"].as_str().unwrap_or("artifact.txt");
            let path=self.root.join(name);
            match action.tool.as_str() {
                "write" => {
                    let content=action.arguments["content"].as_str().unwrap_or("");
                    std::fs::write(path,content).map_err(|e|e.to_string())?;
                    self.writes.fetch_add(1,Ordering::SeqCst);
                    Ok(json!({"written":true}))
                }
                "read" => Ok(json!({"content":std::fs::read_to_string(path).map_err(|e|e.to_string())?})),
                _ => Err("unknown".into())
            }
        })
    }
}
struct FileEvidence(PathBuf);
impl Evidence for FileEvidence {
    fn check<'a>(&'a self, check:&'a Criterion)->BoxFuture<'a, Result<bool,String>> {
        Box::pin(async move {
            let target=self.0.join(check.path.as_deref().unwrap_or(""));
            match check.kind {
                CriterionKind::FileExists => Ok(target.is_file()),
                CriterionKind::FileAbsent => Ok(!target.exists()),
                CriterionKind::FileContains =>
                    Ok(std::fs::read_to_string(target).map_err(|e|e.to_string())?.contains(check.contains.as_deref().unwrap_or(""))),
                CriterionKind::ToolSucceeded => Err("kernel verifies receipts itself".into()),
            }
        })
    }
}
#[derive(Default)]
struct Recorded(Mutex<Vec<Snapshot>>);
impl Journal for Recorded {
    fn save(&self, run:&Snapshot)->Result<(),String> {
        self.0.lock().unwrap().push(run.clone());
        Ok(())
    }
}
fn test_env(responses:Vec<Value>)->(Responses,Files,FileEvidence,Recorded,RunRequest,PathBuf) {
    let root=std::env::temp_dir().join(format!("dominion-kernel-test-{}-{:?}",std::process::id(),std::thread::current().id()));
    let _=std::fs::create_dir_all(&root);
    let model=Responses(Mutex::new(responses.into_iter().map(|v|v.to_string()).collect()));
    let tools=Files {root:root.clone(),writes:AtomicUsize::new(0)};
    let evidence=FileEvidence(root.clone());
    let journal=Recorded::default();
    let request=RunRequest { id:"run-example".into(),instruction:"write artifact.txt with hello".into(),
        owner_instructions:"Preserve the user's exact requested content".into(),
        strategy:Strategy::VerifyFirst,limits:Limits::default(),
        allowed_tools:vec!["read".into(),"write".into()],
        checks:vec![Criterion{kind:CriterionKind::FileContains,
            path:Some("artifact.txt".into()),contains:Some("hello".into()),tool:None}],
        require_evidence:true};
    (model,tools,evidence,journal,request,root)
}
#[tokio::test]
async fn follows_real_file_writes_then_independent_check() {
    let (model,tools,evidence,journal,request,root)=test_env(vec![
        json!({"plan":["write exact content"],"actions":[{"id":"a","tool":"write",
            "arguments":{"path":"artifact.txt","content":"hello"},"dependsOn":[]}]}),
        json!({"plan":["verify done"],"actions":[],"answer":"artifact ready"})
    ]);
    let kernel=Kernel{model:&model,tools:&tools,evidence:&evidence,journal:&journal};
    let output=kernel.run(request).await.unwrap();
    assert!(matches!(output.status,Status::Completed));
    assert_eq!(tools.writes.load(Ordering::SeqCst),1);
    assert_eq!(std::fs::read_to_string(root.join("artifact.txt")).unwrap(),"hello");
    assert_eq!(output.receipts.len(),1);
    assert!(output.checks.iter().all(|check|check.satisfied));
    assert_eq!(output.charged_tokens,240);
    assert!(journal.0.lock().unwrap().iter().any(|snapshot|snapshot.in_flight.len()==1));
    assert!(journal.0.lock().unwrap().iter().any(|snapshot|snapshot.model_in_flight));
    assert!(journal.0.lock().unwrap().last().unwrap().in_flight.is_empty());
    let _=std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn unsupported_success_claim_does_not_complete() {
    let (model,tools,evidence,journal,mut request,root)=test_env(vec![
        json!({"plan":[],"actions":[],"answer":"I edited the file"}),
        json!({"plan":[],"actions":[],"answer":"I edited the file"}),
    ]);
    request.limits.max_steps=2;
    let output=Kernel{model:&model,tools:&tools,evidence:&evidence,journal:&journal}.run(request).await.unwrap();
    assert!(matches!(output.status,Status::NeedsReview));
    assert_eq!(tools.writes.load(Ordering::SeqCst),0);
    assert_eq!(output.model_calls,2);
    let _=std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn repairs_a_failed_acceptance_check_and_verifies_again() {
    let (model,tools,evidence,journal,request,root)=test_env(vec![
        json!({"plan":["first attempt"],"actions":[{"id":"one","tool":"write",
            "arguments":{"path":"artifact.txt","content":"wrong"},"dependsOn":[]}]}),
        json!({"plan":[],"actions":[],"answer":"done"}),
        json!({"plan":["repair"],"actions":[{"id":"two","tool":"write",
            "arguments":{"path":"artifact.txt","content":"hello"},"dependsOn":["one"]}]}),
        json!({"plan":[],"actions":[],"answer":"repaired"})
    ]);
    let result=Kernel{model:&model,tools:&tools,evidence:&evidence,journal:&journal}.run(request).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert_eq!(result.answer.as_deref(),Some("repaired"));
    assert_eq!(tools.writes.load(Ordering::SeqCst),2);
    let _=std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn rejects_unapproved_tool_without_dispatch() {
    let (model,tools,evidence,journal,mut request,root)=test_env(vec![
        json!({"plan":[],"actions":[{"id":"a","tool":"write",
            "arguments":{"path":"artifact.txt","content":"hello"}}]}),
    ]);
    request.allowed_tools=vec!["read".into()];
    let result=Kernel{model:&model,tools:&tools,evidence:&evidence,journal:&journal}.run(request).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert_eq!(tools.writes.load(Ordering::SeqCst),0);
    let _=std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn rejects_duplicate_operation_id_to_prevent_replay() {
    let (model,tools,evidence,journal,request,root)=test_env(vec![
        json!({"plan":[],"actions":[{"id":"same","tool":"write",
            "arguments":{"path":"artifact.txt","content":"hello"}}]}),
        json!({"plan":[],"actions":[{"id":"same","tool":"write",
            "arguments":{"path":"artifact.txt","content":"hello"}}]})
    ]);
    let result=Kernel{model:&model,tools:&tools,evidence:&evidence,journal:&journal}.run(request).await.unwrap();
    assert!(matches!(result.status,Status::NeedsReview));
    assert_eq!(tools.writes.load(Ordering::SeqCst),1);
    let _=std::fs::remove_dir_all(root);
}

struct Directed {
    reads: AtomicUsize,
    snapshots: Mutex<Vec<Snapshot>>,
}
impl Journal for Directed {
    fn save(&self, snapshot:&Snapshot)->Result<(),String>{
        self.snapshots.lock().unwrap().push(snapshot.clone());
        Ok(())
    }
    fn directive(&self, _id:&str)->Result<Option<String>,String>{
        if self.reads.fetch_add(1,Ordering::SeqCst)==0 {Ok(None)}
        else {Ok(Some("Owner corrected the target".into()))}
    }
}
#[tokio::test]
async fn owner_intervention_invalidates_stale_tool_action_before_dispatch() {
    let (model,tools,evidence,_journal,request,root)=test_env(vec![
        json!({"plan":["stale operation"],"actions":[{"id":"wrong","tool":"write",
            "arguments":{"path":"old.txt","content":"stale"}}]}),
        json!({"plan":["owner correction"],"actions":[{"id":"right","tool":"write",
            "arguments":{"path":"artifact.txt","content":"hello"}}]}),
        json!({"plan":[],"actions":[],"answer":"owner's corrected target saved"})
    ]);
    let journal=Directed{reads:AtomicUsize::new(0),snapshots:Mutex::new(vec![])};
    let result=Kernel{model:&model,tools:&tools,evidence:&evidence,journal:&journal}.run(request).await.unwrap();
    assert!(matches!(result.status,Status::Completed));
    assert!(!root.join("old.txt").exists());
    assert_eq!(tools.writes.load(Ordering::SeqCst),1);
    assert!(journal.snapshots.lock().unwrap().iter().any(|s|
        s.checks.iter().any(|c|c.explanation.contains("Owner changed"))));
    let _=std::fs::remove_dir_all(root);
}

struct FailingJournal;
impl Journal for FailingJournal {
    fn save(&self, _: &Snapshot) -> Result<(), String> { Err("disk unavailable".into()) }
}
struct CountDispatch(AtomicUsize);
impl Dispatcher for CountDispatch {
    type Output = ();
    fn execute<'a>(&'a self, _: &'a Action) -> BoxFuture<'a, Result<DispatchResult<()>, String>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(DispatchResult { output: (), observation: json!({}), success: true })
        })
    }
}
#[tokio::test]
async fn native_protocol_cannot_dispatch_without_a_durable_intent() {
    let (_, _, _, _, request, root) = test_env(vec![]);
    let mut run = Snapshot::from_request(&request);
    let dispatch = CountDispatch(AtomicUsize::new(0));
    let action = Action { id:"effect".into(), tool:"write".into(), arguments:json!({}), depends_on:vec![] };
    let result = dispatch_batch(&mut run, &[action], &dispatch, &FailingJournal).await;
    assert!(result.unwrap_err().contains("disk unavailable"));
    assert_eq!(dispatch.0.load(Ordering::SeqCst), 0);
    assert_eq!(run.in_flight.len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn same_decision_write_then_dependent_read_runs_in_dependency_order() {
    let (model, tools, evidence, journal, request, root) = test_env(vec![
        json!({"actions":[
            {"id":"write-first","tool":"write","arguments":{"path":"artifact.txt","content":"hello"}},
            {"id":"read-after","tool":"read","arguments":{"path":"artifact.txt"},
             "dependsOn":["write-first"]}
        ]}),
        json!({"actions":[],"answer":"artifact written and inspected"}),
    ]);
    let output = Kernel {model:&model,tools:&tools,evidence:&evidence,journal:&journal}
        .run(request).await.unwrap();
    assert!(matches!(output.status, Status::Completed), "{:?}",output.error);
    assert_eq!(output.receipts.len(), 2);
    assert_eq!(output.receipts[0].tool, "write");
    assert_eq!(output.receipts[1].tool, "read");
    assert_eq!(output.receipts[1].result["content"], "hello");
    assert_eq!(tools.writes.load(Ordering::SeqCst), 1);
    // A dependent read must never share the write's in-flight dispatch batch.
    let snapshots = journal.0.lock().unwrap();
    assert!(snapshots.iter().any(|snapshot| snapshot.in_flight.len() == 1
        && snapshot.in_flight[0].id == "write-first"));
    assert!(snapshots.iter().any(|snapshot| snapshot.in_flight.len() == 1
        && snapshot.in_flight[0].id == "read-after"));
    drop(snapshots);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn forward_reference_is_topologically_ordered_without_extra_model_call() {
    let (model, tools, evidence, journal, request, root) = test_env(vec![
        json!({"actions":[
            {"id":"inspect","tool":"read","arguments":{"path":"artifact.txt"},
             "dependsOn":["create"]},
            {"id":"create","tool":"write","arguments":{"path":"artifact.txt","content":"hello"}}
        ]}),
        json!({"actions":[],"answer":"completed"}),
    ]);
    let output = Kernel {model:&model,tools:&tools,evidence:&evidence,journal:&journal}
        .run(request).await.unwrap();
    assert!(matches!(output.status, Status::Completed), "{:?}",output.error);
    assert_eq!(output.model_calls, 2);
    assert_eq!(output.receipts.iter().map(|r|r.id.as_str()).collect::<Vec<_>>(),
        vec!["create","inspect"]);
    assert_eq!(output.receipts[1].result["content"],"hello");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn failed_predecessor_skips_dependent_without_executing_it() {
    let (model, tools, evidence, journal, mut request, root) = test_env(vec![
        json!({"actions":[
            {"id":"failed-write","tool":"write",
             "arguments":{"path":"missing-directory/artifact.txt","content":"hello"}},
            {"id":"dependent-read","tool":"read","arguments":{"path":"artifact.txt"},
             "dependsOn":["failed-write"]}
        ]}),
        json!({"actions":[],"answer":"done"}),
    ]);
    request.limits.max_steps = 2;
    let output = Kernel {model:&model,tools:&tools,evidence:&evidence,journal:&journal}
        .run(request).await.unwrap();
    assert!(matches!(output.status, Status::NeedsReview));
    assert_eq!(output.receipts.len(), 2);
    assert!(!output.receipts[0].success);
    assert!(!output.receipts[1].success);
    assert_eq!(output.receipts[1].result["reason"], "failed_dependency");
    assert_eq!(output.receipts[1].result["skipped"], true);
    assert_eq!(tools.writes.load(Ordering::SeqCst), 0);
    assert!(!journal.0.lock().unwrap().iter().any(|snapshot|
        snapshot.in_flight.iter().any(|action|action.id=="dependent-read")));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn invalid_dependency_graph_is_rejected_before_any_tool_dispatch() {
    for actions in [
        json!([
            {"id":"a","tool":"write","arguments":{"content":"hello"},"dependsOn":["b"]},
            {"id":"b","tool":"read","dependsOn":["a"]}
        ]),
        json!([
            {"id":"a","tool":"write","arguments":{"content":"hello"},"dependsOn":["missing"]}
        ]),
    ] {
        let (model, tools, evidence, journal, request, root) =
            test_env(vec![json!({"actions":actions})]);
        let output=Kernel {model:&model,tools:&tools,evidence:&evidence,journal:&journal}
            .run(request).await.unwrap();
        assert!(matches!(output.status, Status::NeedsReview));
        assert!(output.receipts.is_empty());
        assert_eq!(tools.writes.load(Ordering::SeqCst), 0);
        let _=std::fs::remove_dir_all(root);
    }
}

struct StructuredFailure(AtomicUsize);
impl Tools for StructuredFailure {
    fn definitions(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {name:"structured_failure".into(), description:"simulate a structured tool error".into(),
                schema:json!({}), read_only:false},
            ToolSpec {name:"dependent".into(), description:"a dependent side effect".into(),
                schema:json!({}), read_only:false},
        ]
    }
    fn execute<'a>(&'a self, action: &'a Action) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            if action.tool == "structured_failure" {
                Ok(json!({"isError":true,"error":"operation failed","exitCode":1}))
            } else {
                Ok(json!({"ok":true}))
            }
        })
    }
}

#[tokio::test]
async fn structured_tool_error_blocks_descendants_and_never_satisfies_tool_success() {
    let (model, _unused_tools, evidence, journal, mut request, root) = test_env(vec![
        json!({"actions":[
            {"id":"first","tool":"structured_failure"},
            {"id":"second","tool":"dependent","dependsOn":["first"]}
        ]}),
        json!({"actions":[],"answer":"done"}),
    ]);
    request.allowed_tools=vec!["structured_failure".into(),"dependent".into()];
    request.checks=vec![Criterion {
        kind:CriterionKind::ToolSucceeded, path:None, contains:None,
        tool:Some("structured_failure".into()),
    }];
    request.limits.max_steps=2;
    let tools=StructuredFailure(AtomicUsize::new(0));
    let output=Kernel {model:&model,tools:&tools,evidence:&evidence,journal:&journal}
        .run(request).await.unwrap();
    assert!(matches!(output.status, Status::NeedsReview));
    assert_eq!(tools.0.load(Ordering::SeqCst), 1, "dependent tool must not run");
    assert_eq!(output.receipts.len(), 2);
    assert!(!output.receipts[0].success);
    assert!(!output.receipts[1].success);
    assert_eq!(output.receipts[1].result["skipped"], true);
    assert!(output.checks.iter().any(|check| !check.satisfied));
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn repeated_toolless_success_claims_stop_after_three_model_calls() {
    let (model, tools, evidence, journal, request, root) = test_env(vec![
        json!({"actions":[],"answer":"done"}),
        json!({"actions":[],"answer":"done"}),
        json!({"actions":[],"answer":"done"}),
    ]);
    let output=Kernel {model:&model,tools:&tools,evidence:&evidence,journal:&journal}
        .run(request).await.unwrap();
    assert!(matches!(output.status,Status::NeedsReview));
    assert_eq!(output.model_calls,3, "never consume all 24 default rounds on false claims");
    assert!(output.error.as_deref().unwrap_or("").contains("no actionable progress"));
    assert_eq!(tools.writes.load(Ordering::SeqCst),0);
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn genuine_tool_progress_resets_nonprogress_guard() {
    let (model, tools, evidence, journal, request, root) = test_env(vec![
        json!({"actions":[],"answer":"premature claim"}),
        json!({"actions":[{"id":"real","tool":"write",
            "arguments":{"path":"artifact.txt","content":"hello"}}]}),
        json!({"actions":[],"answer":"real work finished"}),
    ]);
    let output=Kernel {model:&model,tools:&tools,evidence:&evidence,journal:&journal}
        .run(request).await.unwrap();
    assert!(matches!(output.status,Status::Completed));
    assert_eq!(output.model_calls,3);
    assert_eq!(tools.writes.load(Ordering::SeqCst),1);
    let _=std::fs::remove_dir_all(root);
}
