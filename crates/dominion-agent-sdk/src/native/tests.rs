use super::*;
use std::sync::atomic::{AtomicUsize,Ordering};
struct HostStub {
    checkpoints: Mutex<std::collections::HashMap<String,Value>>,
    calls: AtomicUsize,
    tools: AtomicUsize,
    reject: bool,
    unknown_usage: bool,
}
impl HostStub {
    fn new(reject:bool,unknown_usage:bool)->Self{
        Self{checkpoints:Mutex::new(Default::default()),calls:AtomicUsize::new(0),tools:AtomicUsize::new(0),reject,unknown_usage}
    }
}
impl NativeHost for HostStub {
    fn complete(&self,request:ModelRequest)->NativeFuture<'_,ModelResponse>{
        Box::pin(async move {
            let n=self.calls.fetch_add(1,Ordering::SeqCst);
            let tools=if n==0 && !request.tools.is_empty() {
                assert!(request.tools.iter().any(|t|t.pointer("/function/name").and_then(Value::as_str)==Some("test_read")));
                vec![ToolCall{id:"native-call-1".into(),name:"test_read".into(),arguments:json!({})}]
            }else{vec![]};
            let content=if tools.is_empty(){"observed answer".into()}else{String::new()};
            Ok(ModelResponse{content,calls:tools,input_tokens:if self.unknown_usage{None}else{Some(12)},output_tokens:Some(4)})
        })
    }
    fn dispatch(&self,_:&str,call:ToolCall)->NativeFuture<'_,ToolOutcome>{
        Box::pin(async move {
            assert_eq!(call.id,"native-call-1");
            self.tools.fetch_add(1,Ordering::SeqCst);
            Ok(ToolOutcome{value:json!({"success":true,"result":"observed"}),success:true})
        })
    }
    fn verify(&self,_:&str,answer:String)->NativeFuture<'_,Verification>{
        Box::pin(async move {
            assert_eq!(answer,"observed answer");
            Ok(if self.reject{Verification::Reject("missing evidence".into())}else{Verification::Accept})
        })
    }
    fn load_checkpoint(&self,id:&str)->Result<Option<Value>,String>{
        Ok(self.checkpoints.lock().unwrap().get(id).cloned())
    }
    fn save_checkpoint(&self,id:&str,state:&Value)->Result<(),String>{
        self.checkpoints.lock().unwrap().insert(id.into(),state.clone()); Ok(())
    }
}
fn request(id:&str)->RunRequest{
    let mut r=RunRequest::new(id,"inspect example");
    r.tools=vec![ToolSpec{name:"test_read".into(),description:"Read example".into(),parameters:json!({"type":"object","properties":{}}),read_only:true}];
    r
}
#[tokio::test]
async fn common_contract_runs_native_tool_and_verifies_with_exact_usage(){
    let host=HostStub::new(false,false);
    let result=run(&host,request("roundtrip")).await.unwrap();
    assert_eq!(result.answer,"observed answer");
    assert_eq!(result.model_calls,2);
    assert_eq!(result.tool_calls,1);
    assert_eq!(result.total_tokens(),Some(32));
    assert_eq!(host.tools.load(Ordering::SeqCst),1);
    assert_eq!(host.load_checkpoint("roundtrip").unwrap().unwrap()["verified"],true);
    assert!(run(&host,request("roundtrip")).await.is_err());
    assert_eq!(host.tools.load(Ordering::SeqCst),1);
}
#[tokio::test]
async fn rejection_is_never_a_verified_completion(){
    let host=HostStub::new(true,false);
    assert!(run(&host,request("rejected")).await.is_err());
    assert_eq!(host.load_checkpoint("rejected").unwrap().unwrap()["verified"],false);
}
#[tokio::test]
async fn missing_usage_is_not_falsely_reported_as_zero(){
    let host=HostStub::new(false,true);
    let result=run(&host,request("unknown")).await.unwrap();
    assert_eq!(result.total_tokens(),None);
}
#[tokio::test]
async fn token_cap_rejects_unknown_usage_before_effect(){
    let host=HostStub::new(false,true);
    let mut r=request("capped");r.limits.max_tokens=Some(10000);
    assert!(run(&host,r).await.is_err());
    assert_eq!(host.tools.load(Ordering::SeqCst),0);
}
#[tokio::test]
async fn batch_rejects_duplicate_ids_before_any_work(){
    let host=HostStub::new(false,false);
    assert!(run_batch(&host,vec![request("same"),request("same")]).await.is_err());
    assert_eq!(host.calls.load(Ordering::SeqCst),0);
}
