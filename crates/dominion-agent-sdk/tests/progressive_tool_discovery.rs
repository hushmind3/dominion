//! Full Rust planning path: owner-authorized sparse catalog -> local search
//! -> next-turn schema promotion -> real host effect. No provider/network calls.
use dominion_agent_sdk::{
 Action, BoxFuture, Criterion, Decision, Evidence, Journal, Kernel, Limits, Model,
 ModelReply, ModelTurn, Receipt, RunRequest, Snapshot, Status, Strategy, ToolSpec, Tools,
};
use serde_json::{json,Value};
use std::sync::{Arc,Mutex,atomic::{AtomicUsize,Ordering}};

struct Host{calls:Arc<Mutex<Vec<String>>>}
impl Tools for Host{
 fn definitions(&self)->Vec<ToolSpec>{
  let mut specs=(0..90).map(|i|ToolSpec{
   name:format!("mcp__unrelated_{i:03}"),
   description:"An unrelated service with expensive schema metadata".repeat(5),
   schema:json!({"type":"object","properties":{"value":{"type":"string","description":"unrelated".repeat(50)}}}),
   read_only:false,
  }).collect::<Vec<_>>();
  specs.push(ToolSpec{
   name:"mcp__archive_publish".into(),
   description:"Publish generated archive to remote artifact registry".into(),
   schema:json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}),
   read_only:false,
  });
  specs.push(ToolSpec{name:"mcp__excluded_secret".into(),
   description:"Never authorized secret operation".into(),
   schema:json!({"type":"object"}),read_only:false});
  specs
 }
 fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
  Box::pin(async move{
   self.calls.lock().unwrap().push(action.tool.clone());
   if action.tool!="mcp__archive_publish"{return Err("Non-approved operation!".into())}
   Ok(json!({"published":true,"archive":"abc"}))
  })
 }
}
struct SequencedNative{
 steps:AtomicUsize,
 seen:Mutex<Vec<Vec<String>>>,
}
impl Model for SequencedNative{
 fn complete<'a>(&'a self,_:&'a str,_:&'a str)
  ->BoxFuture<'a,Result<ModelReply,String>>{
  Box::pin(async{Err("Native model must never call text-only fallback".into())})
 }
 fn supports_native_tools(&self)->bool{true}
 fn complete_with_tools<'a>(&'a self,_system:&'a str,_prompt:&'a str,
   tools:&'a [ToolSpec],_receipts:&'a [Receipt])
   ->BoxFuture<'a,Result<ModelTurn,String>>{
  Box::pin(async move{
   self.seen.lock().unwrap().push(tools.iter().map(|s|s.name.clone()).collect());
   let decision=match self.steps.fetch_add(1,Ordering::SeqCst){
    0=>{
     assert!(tools.iter().any(|s|s.name=="dominion_tool_search"));
     assert!(!tools.iter().any(|s|s.name=="mcp__archive_publish"));
     Decision{plan:vec!["Find the archive service first".into()],
       actions:vec![Action{id:"lookup".into(),tool:"dominion_tool_search".into(),
         arguments:json!({"query":"archive publish","limit":5}),depends_on:vec![]}],
       answer:None}
    }
    1=>{
     assert!(tools.iter().any(|s|s.name=="mcp__archive_publish"));
     Decision{plan:vec!["Publish using discovered schema".into()],
       actions:vec![Action{id:"publish".into(),tool:"mcp__archive_publish".into(),
         arguments:json!({"id":"abc"}),depends_on:vec!["lookup".into()]}],
       answer:None}
    }
    2=>Decision{plan:vec![],actions:vec![],answer:Some("Archive published".into())},
    _=>return Err("Unexpected extra paid model call".into()),
   };
   Ok(ModelTurn{reply:ModelReply{content:String::new(),
      input_tokens:Some(40),output_tokens:Some(20)},decision:Some(decision)})
  })
 }
}
struct NoFileEvidence;
impl Evidence for NoFileEvidence{
 fn check<'a>(&'a self,_:&'a Criterion)->BoxFuture<'a,Result<bool,String>>{
  Box::pin(async{Ok(true)})
 }
}
#[derive(Default)]
struct Durable{snapshots:Mutex<Vec<Snapshot>>}
impl Journal for Durable{
 fn save(&self,snapshot:&Snapshot)->Result<(),String>{
  self.snapshots.lock().unwrap().push(snapshot.clone());Ok(())
 }
}
#[tokio::test]
async fn discovery_is_local_durable_and_promotes_only_owner_allowed_schemas(){
 let host=Host{calls:Arc::new(Mutex::new(vec![]))};
 let model=SequencedNative{steps:AtomicUsize::new(0),seen:Mutex::new(vec![])};
 let journal=Durable::default();
 let permitted=host.definitions().iter().filter(|s|s.name!="mcp__excluded_secret")
   .map(|s|s.name.clone()).collect::<Vec<_>>();
 let kernel=Kernel{model:&model,tools:&host,evidence:&NoFileEvidence,journal:&journal};
 let request=RunRequest{
  id:"progressive-native".into(),
  instruction:"Complete delivery using the right service".into(),
  owner_instructions:String::new(),
  strategy:Strategy::LowCost,
  limits:Limits{max_steps:6,max_model_calls:6,max_tool_calls:12,
    max_estimated_tokens:100_000,max_parallel_reads:2,max_result_chars:1200},
  allowed_tools:permitted,checks:vec![],require_evidence:true,
 };
 let state=kernel.run(request).await.unwrap();
 assert!(matches!(state.status,Status::Completed),"{state:?}");
 assert_eq!(state.answer.as_deref(),Some("Archive published"));
 assert_eq!(*host.calls.lock().unwrap(),vec!["mcp__archive_publish"]);
 assert!(state.receipts.iter().any(|r|r.tool=="dominion_tool_search"&&r.success));
 assert_eq!(model.steps.load(Ordering::SeqCst),3);
 let seen=model.seen.lock().unwrap();
 assert_eq!(seen.len(),3);
 assert!(seen.iter().all(|c|c.len()<=13));
 assert!(seen.iter().all(|c|!c.contains(&"mcp__excluded_secret".into())));
 assert!(seen[1].contains(&"mcp__archive_publish".into()));
 let full=serde_json::to_string(&host.definitions()).unwrap().len();
 let first=seen[0].iter().filter_map(|name|host.definitions().into_iter().find(|s|s.name==*name))
   .collect::<Vec<_>>();
 let selected=serde_json::to_string(&first).unwrap().len();
 assert!(selected*3<full,"large catalog must be compressed before inference");
 let durable=journal.snapshots.lock().unwrap();
 assert!(durable.iter().any(|s|s.receipts.iter().any(|r|
  r.tool=="dominion_tool_search"&&r.result["nextTurnSchemas"]==true)));
}
