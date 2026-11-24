//! Consumer-side integration test. Only the public SDK is imported.
use dominion_sdk::{run, BudgetLedger, Context, Host, Journal, Snapshot, Flow, ModelDecision, BoxFuture, Status};
use serde_json::Value;
use std::{sync::{Arc,Mutex},time::Duration};

struct Memory(Mutex<Snapshot>);
impl Journal for Memory {
    fn save(&self,snapshot:&Snapshot)->Result<(),String>{
        *self.0.lock().unwrap()=snapshot.clone();Ok(())
    }
    fn cancelled(&self,_:&str)->bool{false}
}
struct Consumer { journal:Arc<Memory>, calls:usize, verifies:usize }
impl Host for Consumer {
    type Reply=();
    type ToolResult=();
    fn reload(&self)->Result<Snapshot,String>{Ok(self.journal.0.lock().unwrap().clone())}
    fn initialize<'a>(&'a mut self,_:&'a mut Context<'_>)->BoxFuture<'a,Result<Flow,String>>{Box::pin(async{Ok(Flow::Continue)})}
    fn wants_plan(&self)->bool{false}
    fn plan<'a>(&'a mut self,_:&'a mut Context<'_>)->BoxFuture<'a,Result<Option<String>,String>>{Box::pin(async{Ok(None)})}
    fn prepare_model<'a>(&'a mut self,_:&'a mut Context<'_>)->BoxFuture<'a,Result<Vec<Value>,String>>{Box::pin(async{Ok(vec![])})}
    fn model<'a>(&'a mut self,_:&'a mut Context<'_>)->BoxFuture<'a,Result<(),String>>{
        Box::pin(async move{self.calls+=1;Ok(())})
    }
    fn observe_model<'a>(&'a mut self,_:&'a mut Context<'_>,_:())->BoxFuture<'a,Result<ModelDecision,String>>{Box::pin(async{Ok(ModelDecision::Verify)})}
    fn verify<'a>(&'a mut self,_:&'a mut Context<'_>)->BoxFuture<'a,Result<Flow,String>>{
        Box::pin(async move{self.verifies+=1;Ok(if self.verifies==2{Flow::Complete}else{Flow::Continue})})
    }
    fn begin_tools<'a>(&'a mut self,_:&'a mut Context<'_>)->BoxFuture<'a,Result<usize,String>>{Box::pin(async{Err("unexpected tool".into())})}
    fn prepare_tool<'a>(&'a mut self,_:&'a mut Context<'_>,_:usize)->BoxFuture<'a,Result<bool,String>>{Box::pin(async{Err("unexpected tool".into())})}
    fn dispatch_tool<'a>(&'a mut self,_:&'a mut Context<'_>,_:usize)->BoxFuture<'a,Result<(),String>>{Box::pin(async{Err("unexpected tool".into())})}
    fn observe_tool<'a>(&'a mut self,_:&'a mut Context<'_>,_:usize,_:())->BoxFuture<'a,Result<(),String>>{Box::pin(async{Err("unexpected tool".into())})}
    fn verify_tools<'a>(&'a mut self,_:&'a mut Context<'_>)->BoxFuture<'a,Result<Flow,String>>{Box::pin(async{Err("unexpected tool".into())})}
    fn exhausted(&mut self)->Result<(),String>{Err("budget exhausted".into())}
    fn completion(&self)->Result<(String,usize,u64),String>{Ok(("accepted".into(),self.calls,0))}
}
#[tokio::test]
async fn external_host_uses_native_verification_retry_and_journal(){
    let snapshot=Snapshot::new("external","verify twice",3).unwrap();
    let journal=Arc::new(Memory(Mutex::new(snapshot.clone())));
    let mut host=Consumer{journal:journal.clone(),calls:0,verifies:0};
    run(&mut host,Context{snapshot,budget:BudgetLedger::new(3,1,Duration::from_secs(5)),round:0,journal:journal.as_ref()}).await.unwrap();
    assert_eq!(host.calls,2);
    let saved=journal.0.lock().unwrap();
    assert_eq!(saved.native.status,Status::Completed);
    assert_eq!(saved.model_calls,2);
    assert!(!saved.model_in_flight);
}
#[tokio::test]
async fn unresolved_intent_blocks_external_host_before_model_call(){
    let mut snapshot=Snapshot::new("uncertain","do not replay",3).unwrap();
    snapshot.model_in_flight=true;
    let journal=Arc::new(Memory(Mutex::new(snapshot.clone())));
    let mut host=Consumer{journal:journal.clone(),calls:0,verifies:0};
    assert!(run(&mut host,Context{snapshot,budget:BudgetLedger::new(3,1,Duration::from_secs(5)),round:0,journal:journal.as_ref()}).await.is_err());
    assert_eq!(host.calls,0);
}
