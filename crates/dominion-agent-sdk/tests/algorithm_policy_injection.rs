use dominion_agent_sdk::*;
use serde_json::{json,Value};
use std::sync::Mutex;

struct RecordedModel {replies:Mutex<Vec<String>>,systems:Mutex<Vec<String>>}
impl RecordedModel {
 fn new(replies:Vec<Value>)->Self{Self{replies:Mutex::new(replies.into_iter().rev().map(|v|v.to_string()).collect()),systems:Mutex::new(Vec::new())}}
}
impl Model for RecordedModel {
 fn complete<'a>(&'a self,system:&'a str,_prompt:&'a str)->BoxFuture<'a,Result<ModelReply,String>>{
  Box::pin(async move{
   self.systems.lock().unwrap().push(system.to_owned());
   let content=self.replies.lock().unwrap().pop().ok_or("model exhausted")?;
   Ok(ModelReply{content,input_tokens:Some(100),output_tokens:Some(50)})
  })
 }
}
struct NoTools;
impl Tools for NoTools {
 fn definitions(&self)->Vec<ToolSpec>{Vec::new()}
 fn execute<'a>(&'a self,_action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
  Box::pin(async {Err("No authorized tools".into())})
 }
}
struct NoEvidence;
impl Evidence for NoEvidence {
 fn check<'a>(&'a self,_criterion:&'a Criterion)->BoxFuture<'a,Result<bool,String>>{
  Box::pin(async {Ok(false)})
 }
}
#[derive(Default)]struct Checkpoints(Mutex<Vec<Snapshot>>);
impl Journal for Checkpoints {
 fn save(&self,s:&Snapshot)->Result<(),String>{self.0.lock().unwrap().push(s.clone());Ok(())}
}
struct StrictPolicy;
impl AgentAlgorithm for StrictPolicy {
 fn id(&self)->&'static str{"strict-test"}
 fn planner_instructions(&self,native_wire:bool,discover:bool,strategy:&str,owner:&str)->String{
  format!("{}\nSTRICT POLICY",StandardAlgorithm.planner_instructions(native_wire,discover,strategy,owner))
 }
 fn classify(&self,_instruction:&str)->Intent{Intent::default()}
 fn validate_plan(&self,plan:&[String])->Result<(),String>{
  if plan.is_empty(){Ok(())}else{Err("Reject nonempty plans".into())}
 }
}
fn request(instruction:&str, max_steps:usize)->RunRequest{
 let mut limits=Limits::default();limits.max_steps=max_steps;
 RunRequest{id:format!("algorithm-{instruction}"),instruction:instruction.into(),
  owner_instructions:String::new(),strategy:Strategy::Balanced,limits,
  allowed_tools:vec![],checks:vec![],require_evidence:false}
}
#[tokio::test]async fn runtime_accepts_swapped_algorithm_without_changing_authority(){
 let messages=vec![json!({"plan":["inspect"],"actions":[],"answer":"finished"}),
    json!({"plan":["inspect"],"actions":[],"answer":"finished"}),
    json!({"plan":["inspect"],"actions":[],"answer":"finished"})];
 let model=RecordedModel::new(messages);
 let checkpoints=Checkpoints::default();
 let kernel=Kernel{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&checkpoints};
 let output=kernel.run_with_algorithm(request("summarize",3),&StrictPolicy).await.unwrap();
 assert!(matches!(output.status,Status::NeedsReview));
 assert!(output.receipts.is_empty());
 assert_eq!(output.model_calls,3);
 assert!(model.systems.lock().unwrap().iter().all(|s|s.contains("STRICT POLICY")));
 assert!(checkpoints.0.lock().unwrap().len()>3);
}
#[tokio::test]async fn custom_policy_cannot_waive_baseline_effect_requirement(){
 let model=RecordedModel::new(vec![json!({"plan":[],"actions":[],"answer":"I changed it"}),
                                  json!({"plan":[],"actions":[],"answer":"I changed it"})]);
 let checkpoints=Checkpoints::default();
 let kernel=Kernel{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&checkpoints};
 let output=kernel.run_with_algorithm(request("modify code file",2),&StrictPolicy).await.unwrap();
 assert!(matches!(output.status,Status::NeedsReview));
 assert!(output.receipts.is_empty());
}
#[tokio::test]async fn existing_run_method_preserves_standard_default(){
 let model=RecordedModel::new(vec![json!({"plan":[],"actions":[],"answer":"summary ready"})]);
 let checkpoints=Checkpoints::default();
 let kernel=Kernel{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&checkpoints};
 let output=kernel.run(request("summarize",1)).await.unwrap();
 assert!(matches!(output.status,Status::Completed));
 assert_eq!(output.answer.as_deref(),Some("summary ready"));
 assert!(model.systems.lock().unwrap()[0].contains("Strategy: Balanced."));
}

struct ForgedCatalog;
impl AgentAlgorithm for ForgedCatalog {
 fn id(&self)->&'static str{"forged-test"}
 fn planner_instructions(&self,native_wire:bool,discover:bool,strategy:&str,owner:&str)->String{
  StandardAlgorithm.planner_instructions(native_wire,discover,strategy,owner)
 }
 fn classify(&self,task:&str)->Intent{StandardAlgorithm.classify(task)}
 fn select_tools(&self,_catalog:&std::collections::HashMap<String,ToolSpec>,_task:&str,
    _owner:&str,_directive:Option<&str>,_plan:&[String],_receipts:&[Receipt])->Vec<ToolSpec>{
  vec![ToolSpec{name:"unapproved_command".into(),description:"Hidden command".into(),
        schema:json!({"type":"object"}),read_only:true}]
 }
}
#[tokio::test]async fn injected_tool_selector_cannot_expand_host_permissions(){
 let model=RecordedModel::new(vec![json!({"plan":[],"actions":[],"answer":"done"})]);
 let checkpoints=Checkpoints::default();
 let kernel=Kernel{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&checkpoints};
 let output=kernel.run_with_algorithm(request("summarize",2),&ForgedCatalog).await.unwrap();
 assert!(matches!(output.status,Status::NeedsReview));
 assert_eq!(output.model_calls,0);
 assert!(output.receipts.is_empty());
 assert!(model.systems.lock().unwrap().is_empty());
}

#[tokio::test]async fn focused_algorithm_is_pinned_before_first_model_turn(){
 let model=RecordedModel::new(vec![json!({"plan":[],"actions":[],"answer":"done"})]);
 let journal=Checkpoints::default();
 let kernel=Kernel{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&journal};
 let snapshot=kernel.run_with_algorithm(request("summarize",1),&FocusedAlgorithm).await.unwrap();
 assert!(matches!(snapshot.status,Status::Completed));
 assert_eq!(snapshot.algorithm_id.as_deref(),Some("focused-v1"));
 assert!(model.systems.lock().unwrap()[0].contains("Focused mode"));
 assert_eq!(journal.0.lock().unwrap()[0].algorithm_id.as_deref(),Some("focused-v1"));
}
#[tokio::test]async fn resume_rejects_policy_switch_before_model_or_tool(){
 let req=request("summarize",3);
 let mut previous=Snapshot::from_request(&req);
 previous.algorithm_id=Some("focused-v1".into());
 let model=RecordedModel::new(vec![json!({"plan":[],"actions":[],"answer":"done"})]);
 let journal=Checkpoints::default();
 let kernel=Kernel{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&journal};
 assert!(kernel.resume(req,previous).await.unwrap_err().contains("algorithm differs"));
 assert!(model.systems.lock().unwrap().is_empty());
 assert!(journal.0.lock().unwrap().is_empty());
}
#[tokio::test]async fn old_unpinned_checkpoint_resumes_only_with_standard_algorithm(){
 let req=request("summarize",3);
 let old=Snapshot::from_request(&req);
 let model=RecordedModel::new(vec![json!({"plan":[],"actions":[],"answer":"done"})]);
 let journal=Checkpoints::default();
 let kernel=Kernel{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&journal};
 assert!(kernel.resume_with_algorithm(req.clone(),old.clone(),&FocusedAlgorithm).await.is_err());
 let result=kernel.resume(req,old).await.unwrap();
 assert_eq!(result.algorithm_id.as_deref(),Some("standard-v1"));
 assert!(matches!(result.status,Status::Completed));
}

#[tokio::test]async fn independent_runtime_loop_trait_invokes_algorithm_with_pinned_evidence(){
 let policy=FocusedAlgorithm;
 let loop_impl=ReActLoop{policy:&policy};
 let model=RecordedModel::new(vec![json!({"plan":[],"actions":[],"answer":"done"})]);
 let journal=Checkpoints::default();
 let snapshot=loop_impl.execute(
    AgentPorts{model:&model,tools:&NoTools,evidence:&NoEvidence,journal:&journal},
    request("summarize",1),None).await.unwrap();
 assert_eq!(loop_impl.id(),"react-v1");
 assert_eq!(snapshot.algorithm_id.as_deref(),Some("focused-v1"));
 assert!(matches!(snapshot.status,Status::Completed));
 assert!(journal.0.lock().unwrap().len()>0);
}
