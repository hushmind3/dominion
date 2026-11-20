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
