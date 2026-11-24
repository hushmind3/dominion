//! Evidence falsification: a successful model/tool claim is not an accepted artifact.
use dominion_agent_sdk::{Action,BoxFuture,Criterion,CriterionKind,Evidence,Journal,
 Kernel,Limits,Model,ModelReply,RunRequest,Snapshot,Status,Strategy,ToolSpec,Tools};
use serde_json::{json,Value};
use std::{collections::VecDeque,sync::Mutex};

struct ScriptedModel{turns:Mutex<VecDeque<String>>}
impl Model for ScriptedModel{
 fn complete<'a>(&'a self,_:&'a str,_:&'a str)->BoxFuture<'a,Result<ModelReply,String>>{
  Box::pin(async move{
   let content=self.turns.lock().unwrap().pop_front().ok_or("No scripted model turn")?;
   Ok(ModelReply{content,input_tokens:Some(30),output_tokens:Some(20)})
  })
 }
}
struct Writes;
impl Tools for Writes{
 fn definitions(&self)->Vec<ToolSpec>{
  vec![ToolSpec{name:"workspace_write".into(),description:"write".into(),
    schema:json!({}),read_only:false}]
 }
 fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
  Box::pin(async move{
   let expected=if action.id=="repair"{"b".repeat(64)}else{"a".repeat(64)};
   Ok(json!({"written":true,"path":"src/output.rs","postSha256":expected}))
  })
 }
}
struct IndependentBytes;
impl Evidence for IndependentBytes{
 fn check<'a>(&'a self,criterion:&'a Criterion)->BoxFuture<'a,Result<bool,String>>{
  Box::pin(async move {
   assert_eq!(criterion.kind,CriterionKind::FileSha256);
   Ok(criterion.path.as_deref()==Some("src/output.rs") &&
      criterion.contains.as_deref()==Some("b".repeat(64).as_str()))
  })
 }
}
#[derive(Default)]
struct JournalLog{saved:Mutex<Vec<Snapshot>>}
impl Journal for JournalLog{
 fn save(&self,snapshot:&Snapshot)->Result<(),String>{
  self.saved.lock().unwrap().push(snapshot.clone());Ok(())
 }
}
fn decision(actions:Value,answer:Value)->String{
 json!({"plan":["edit","verify"],"actions":actions,"answer":answer}).to_string()
}
#[tokio::test]
async fn stale_artifact_bytes_force_repair_before_any_completion(){
 let model=ScriptedModel{turns:Mutex::new(VecDeque::from(vec![
  decision(json!([{"id":"first","tool":"workspace_write",
    "arguments":{"path":"src/output.rs","content":"outdated"},"dependsOn":[]}]),Value::Null),
  decision(json!([]),json!("Done, according to the model")),
  decision(json!([{"id":"repair","tool":"workspace_write",
    "arguments":{"path":"src/output.rs","content":"fixed"},"dependsOn":[]}]),Value::Null),
  decision(json!([]),json!("Fixed and read back")),
 ]))};
 let tools=Writes;
 let evidence=IndependentBytes;
 let journal=JournalLog::default();
 let kernel=Kernel{model:&model,tools:&tools,evidence:&evidence,journal:&journal};
 let request=RunRequest{
  id:"witness".into(),instruction:"Update src/output.rs".into(),
  owner_instructions:String::new(),strategy:Strategy::VerifyFirst,
  limits:Limits{max_steps:8,max_model_calls:8,max_tool_calls:96,
    max_estimated_tokens:100_000,max_parallel_reads:2,max_result_chars:1000},
  allowed_tools:vec!["workspace_write".into()],
  checks:vec![],require_evidence:true
 };
 let result=kernel.run(request).await.unwrap();
 assert!(matches!(result.status,Status::Completed));
 assert_eq!(result.answer.as_deref(),Some("Fixed and read back"));
 assert_eq!(result.receipts.len(),2);
 let snaps=journal.saved.lock().unwrap();
 assert!(snaps.iter().any(|s|s.checks.iter().any(|c|
   c.criterion.kind==CriterionKind::FileSha256&&!c.satisfied)));
 assert!(result.checks.iter().any(|c|
   c.criterion.kind==CriterionKind::FileSha256&&c.satisfied));
}

#[derive(Clone)]
struct MutatingEnvironment {
 bytes:std::sync::Arc<Mutex<String>>,
 checks:std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl Tools for MutatingEnvironment {
 fn definitions(&self)->Vec<ToolSpec>{
  ["workspace_write","shell_exec"].into_iter().map(|name|ToolSpec{
   name:name.into(),description:name.into(),schema:json!({}),read_only:false
  }).collect()
 }
 fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
  Box::pin(async move{
   if action.tool=="shell_exec" {
    let attempt=self.checks.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
    if attempt==0 {
     *self.bytes.lock().unwrap()="corrupted-by-test".into();
    }
    return Ok(json!({"exitCode":0}));
   }
   if action.tool=="workspace_write" {
    let is_repair=action.id=="repair";
    *self.bytes.lock().unwrap()=if is_repair{"repaired"}else{"first"}.into();
    return Ok(json!({"written":true,"postSha256":
        if is_repair{"b".repeat(64)}else{"a".repeat(64)}}));
   }
   Err("Unsupported action".into())
  })
 }
}
impl Evidence for MutatingEnvironment {
 fn check<'a>(&'a self,criterion:&'a Criterion)->BoxFuture<'a,Result<bool,String>>{
  Box::pin(async move {
   if criterion.kind!=CriterionKind::FileSha256{return Err("Unexpected criterion".into())}
   let actual=self.bytes.lock().unwrap().clone();
   Ok(criterion.path.as_deref()==Some("src/output.rs")
    && (criterion.contains.as_deref()==Some("a".repeat(64).as_str()) && actual=="first"
     || criterion.contains.as_deref()==Some("b".repeat(64).as_str()) && actual=="repaired"))
  })
 }
}
#[tokio::test]
async fn acceptance_command_mutation_invalidates_old_witness_and_forces_repair(){
 let env=MutatingEnvironment{
  bytes:std::sync::Arc::new(Mutex::new(String::new())),
  checks:std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
 };
 let model=ScriptedModel{turns:Mutex::new(VecDeque::from(vec![
  decision(json!([{"id":"first","tool":"workspace_write",
    "arguments":{"path":"src/output.rs","content":"first"},"dependsOn":[]}]),Value::Null),
  decision(json!([]),json!("Claimed complete before test changes the file")),
  decision(json!([{"id":"repair","tool":"workspace_write",
    "arguments":{"path":"src/output.rs","content":"repaired"},"dependsOn":[]}]),Value::Null),
  decision(json!([]),json!("Now repaired and reverified")),
 ]))};
 let journal=JournalLog::default();
 let kernel=Kernel{model:&model,tools:&env,evidence:&env,journal:&journal};
 let request=RunRequest{
  id:"post-check-witness".into(),
  instruction:"Update src/output.rs".into(),
  owner_instructions:String::new(),strategy:Strategy::VerifyFirst,
  limits:Limits{max_steps:8,max_model_calls:8,max_tool_calls:8,
    max_estimated_tokens:100_000,max_parallel_reads:2,max_result_chars:1000},
  allowed_tools:vec!["workspace_write".into(),"shell_exec".into()],
  checks:vec![Criterion{kind:CriterionKind::ToolSucceeded,
   tool:Some("shell_exec".into()),contains:Some("cargo test".into()),path:None}],
  require_evidence:true,
 };
 let result=kernel.run(request).await.unwrap();
 assert!(matches!(result.status,Status::Completed));
 assert_eq!(result.answer.as_deref(),Some("Now repaired and reverified"));
 assert_eq!(env.bytes.lock().unwrap().as_str(),"repaired");
 assert_eq!(env.checks.load(std::sync::atomic::Ordering::SeqCst),2);
 let history=journal.saved.lock().unwrap();
 assert!(history.iter().any(|s|s.checks.iter().any(|c|
   c.criterion.kind==CriterionKind::FileSha256&&!c.satisfied)),
   "test command changed bytes after first successful write; completion must fail");
 assert_eq!(result.receipts.iter().filter(|r|r.tool=="shell_exec").count(),2);
}
