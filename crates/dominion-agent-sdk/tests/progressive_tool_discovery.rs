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
     assert!(tools.iter().any(|s|s.name=="dominion_tool_call"));
     assert!(!tools.iter().any(|s|s.name=="mcp__archive_publish"));
     Decision{plan:vec!["Find the archive service first".into()],
       actions:vec![Action{id:"lookup".into(),tool:"dominion_tool_search".into(),
         arguments:json!({"query":"archive publish","limit":5}),depends_on:vec![]}],
       answer:None}
    }
    1=>{
     assert!(!tools.iter().any(|s|s.name=="mcp__archive_publish"),
       "native schemas must stay fixed after discovery for prefix caching");
     assert!(tools.iter().any(|s|s.name=="dominion_tool_call"));
     Decision{plan:vec!["Publish using discovered schema via fixed bridge".into()],
       actions:vec![Action{id:"publish".into(),tool:"dominion_tool_call".into(),
         arguments:json!({"name":"mcp__archive_publish","arguments":{"id":"abc"}}),
         depends_on:vec!["lookup".into()]}],
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
 assert_eq!(seen[0],seen[1],"discovery must not invalidate tool-prefix cache");
 assert_eq!(seen[1],seen[2],"stable catalog even after hidden host effects");
 let full=serde_json::to_string(&host.definitions()).unwrap().len();
 let first=seen[0].iter().filter_map(|name|host.definitions().into_iter().find(|s|s.name==*name))
   .collect::<Vec<_>>();
 let selected=serde_json::to_string(&first).unwrap().len();
 assert!(selected*3<full,"large catalog must be compressed before inference");
 let durable=journal.snapshots.lock().unwrap();
 assert!(durable.iter().any(|s|s.receipts.iter().any(|r|
  r.tool=="dominion_tool_search"&&r.result["nextTurnSchemas"]==false)));
 assert!(state.receipts.iter().any(|r|r.id=="publish"
  && r.tool=="mcp__archive_publish"&&r.success),
  "the bridge must store the real invoked tool, not a synthetic wrapper");
}

// A Korean owner request must expose its service-specific read-only connector
// on the first native model turn, execute through the REAL Rust kernel and
// preserve its receipt, not require an extra paid discovery/planning call.
struct KoreanConnectorHost;
impl Tools for KoreanConnectorHost {
 fn definitions(&self)->Vec<ToolSpec>{
  let mut catalog=(0..75).map(|i|ToolSpec{
   name:format!("mcp__generic_{i:03}"),
   description:"Unrelated operation".into(),
   schema:json!({"type":"object","properties":{"value":{"type":"string"}}}),
   read_only:false
  }).collect::<Vec<_>>();
  catalog.push(ToolSpec{
   name:"github_work_read".into(),
   description:"Read connected GitHub repositories, issues and pull requests".into(),
   schema:json!({"type":"object","properties":{
    "operation":{"type":"string","enum":["issues"]}},
    "required":["operation"],"additionalProperties":false}),
   read_only:true
  });
  catalog.push(ToolSpec{
   name:"google_workspace_read".into(),
   description:"Read Gmail, Calendar and Drive content".into(),
   schema:json!({"type":"object"}),read_only:true
  });
  catalog
 }
 fn execute<'a>(&'a self, action:&'a Action)->BoxFuture<'a,Result<Value,String>>{
  Box::pin(async move {
   if action.tool!="github_work_read"{return Err("Wrong service selected".into())}
   Ok(json!({"issues":[11,22]}))
  })
 }
}
struct KoreanConnectorModel {
 calls:AtomicUsize,
}
impl Model for KoreanConnectorModel {
 fn complete<'a>(&'a self,_:&'a str,_:&'a str)
   ->BoxFuture<'a,Result<ModelReply,String>>{
  Box::pin(async{Err("Unexpected JSON fallback".into())})
 }
 fn supports_native_tools(&self)->bool{true}
 fn complete_with_tools<'a>(&'a self,_:&'a str,_:&'a str,
  specs:&'a [ToolSpec], receipts:&'a [Receipt])
  ->BoxFuture<'a,Result<ModelTurn,String>>{
  Box::pin(async move {
   let turn=self.calls.fetch_add(1,Ordering::SeqCst);
   assert!(specs.len()<=13);
   assert!(specs.iter().any(|s|s.name=="github_work_read"),
      "Korean Github task must expose GitHub immediately");
   assert!(!specs.iter().any(|s|s.name=="google_workspace_read"));
   let decision=match turn {
    0=>Decision{plan:vec!["Read issue list".into()],
      actions:vec![Action{id:"read-issues".into(),tool:"github_work_read".into(),
       arguments:json!({"operation":"issues"}),depends_on:vec![]}],answer:None},
    1=>{
     assert!(receipts.iter().any(|r|r.id=="read-issues"&&r.success
       &&r.result["issues"]==json!([11,22])));
     Decision{plan:vec![],actions:vec![],
       answer:Some("확인된 이슈는 11번과 22번입니다.".into())}
    },
    _=>return Err("Extra model call was unnecessary".into())
   };
   Ok(ModelTurn{reply:ModelReply{
     content:String::new(),input_tokens:Some(40),output_tokens:Some(20)},
     decision:Some(decision)})
  })
 }
}
#[tokio::test]
async fn korean_github_request_finishes_in_two_model_turns_with_real_receipt() {
 let host=KoreanConnectorHost;
 let model=KoreanConnectorModel{calls:AtomicUsize::new(0)};
 let journal=Durable::default();
 let tools=host.definitions().into_iter().map(|s|s.name).collect::<Vec<_>>();
 let kernel=Kernel{model:&model,tools:&host,evidence:&NoFileEvidence,journal:&journal};
 let request=RunRequest{
  id:"korean-github-owner".into(),
  instruction:"깃허브에서 열린 이슈들을 확인하고 번호를 정리해 주세요".into(),
  owner_instructions:String::new(),
  strategy:Strategy::LowCost,
  limits:Limits{max_steps:4,max_model_calls:4,max_tool_calls:8,
    max_estimated_tokens:50000,max_parallel_reads:2,max_result_chars:1200},
  allowed_tools:tools,checks:vec![],require_evidence:true
 };
 let done=kernel.run(request).await.unwrap();
 assert!(matches!(done.status,Status::Completed),"{done:?}");
 assert_eq!(done.answer.as_deref(),Some("확인된 이슈는 11번과 22번입니다."));
 assert_eq!(model.calls.load(Ordering::SeqCst),2);
 assert!(done.receipts.iter().any(|r|r.tool=="github_work_read"&&r.success));
 assert!(!done.receipts.iter().any(|r|r.tool=="dominion_tool_search"),
   "Relevant connector should not cost a separate discovery turn");
}
