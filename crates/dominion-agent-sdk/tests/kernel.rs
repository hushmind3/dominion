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
