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
    fn supports_streaming(&self)->bool{true}
    fn complete_streamed<'a>(&'a self,request:ModelRequest,emit:&'a(dyn Fn(ModelDelta)+Send+Sync))->NativeFuture<'a,ModelResponse>{
        Box::pin(async move{emit(ModelDelta::Text("fixture delta".into()));self.complete(request).await})
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

struct SuspendAfterTool(RunControl);
impl NativeHook for SuspendAfterTool{fn event(&self,_:&str,event:&Event){if matches!(event,Event::ToolFinished{..}){self.0.suspend();}}}
#[tokio::test]
async fn suspend_resume_does_not_repeat_paid_calls_or_effects(){
    let host=HostStub::new(false,false);let control=RunControl::new();let hook=SuspendAfterTool(control.clone());
    let mut options=RunOptions::default();options.control=Some(control);options.hooks.push(&hook);
    assert!(run_with_options(&host,request("suspend"),options).await.unwrap_err().contains("suspended"));
    assert_eq!(host.load_checkpoint("suspend").unwrap().unwrap()["suspended"],true);
    assert_eq!(host.calls.load(Ordering::SeqCst),1);assert_eq!(host.tools.load(Ordering::SeqCst),1);
    let result=resume(&host,request("suspend")).await.unwrap();
    assert_eq!(result.model_calls,2);assert_eq!(result.tool_calls,1);assert_eq!(result.total_tokens(),Some(32));
    assert_eq!(resume(&host,request("suspend")).await.unwrap().answer,result.answer);
    assert_eq!(host.calls.load(Ordering::SeqCst),2);assert_eq!(host.tools.load(Ordering::SeqCst),1);
}
#[tokio::test]
async fn rejects_uncertain_checkpoint_and_changed_request(){
    let host=HostStub::new(false,true);let mut req=request("uncertain");req.limits.max_tokens=Some(10000);
    assert!(run(&host,req.clone()).await.is_err());assert!(resume(&host,req.clone()).await.is_err());
    req.instruction="different task".into();assert!(resume(&host,req).await.is_err());assert_eq!(host.calls.load(Ordering::SeqCst),1);
}
#[tokio::test]
async fn streamed_run_forwards_provider_deltas(){
    let host=HostStub::new(false,false);
    let ChannelRun{mut events,result,..}=run_streamed(&host,request("stream"),RunOptions::default()).unwrap();
    let(result,seen)=tokio::join!(result,async move{let mut seen=vec![];while let Some(event)=events.recv().await{seen.push(event);}seen});
    assert_eq!(result.unwrap().tool_calls,1);assert_eq!(seen.iter().filter(|e|matches!(e,Event::ModelDelta{..})).count(),2);
    assert!(seen.iter().any(|e|matches!(e,Event::Verified)));
}
#[tokio::test]
async fn step_gates_actual_operations(){
    let host=HostStub::new(false,false);let ChannelRun{control,mut events,result}=run_channel(&host,request("step"),RunOptions::default());
    control.pause();
    let drive=async{control.step();let mut n=0;while let Some(event)=events.recv().await{
        match event{Event::ModelFinished{..}|Event::ToolFinished{..}=>{assert!(control.is_paused());n+=1;control.step();},Event::Verified=>break,_=>{}}
    }n};
    let(result,n)=tokio::join!(result,drive);assert!(result.is_ok());assert_eq!(n,3);
}
struct DenyTool;
impl NativeHook for DenyTool{fn before(&self,c:HookContext)->NativeFuture<'_,HookDecision>{Box::pin(async move{
    Ok(if matches!(c.operation,Operation::Tool{..}){HookDecision::Reject("denied".into())}else{HookDecision::Allow})
})}}
#[tokio::test]
async fn hook_denies_real_tool_io(){
    let host=HostStub::new(false,false);let mut options=RunOptions::default();options.hooks.push(&DenyTool);
    assert!(run_with_options(&host,request("deny"),options).await.is_err());assert_eq!(host.tools.load(Ordering::SeqCst),0);
}
#[test]
fn sync_runs_same_native_engine(){let host=HostStub::new(false,false);assert_eq!(run_sync(&host,request("sync")).unwrap().total_tokens(),Some(32));}
#[tokio::test]
async fn sync_rejects_nested_runtime(){let host=HostStub::new(false,false);assert!(run_sync(&host,request("nested")).is_err());assert_eq!(host.calls.load(Ordering::SeqCst),0);}
