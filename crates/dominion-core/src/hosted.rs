//! Native sub4 execution state machine embedded in an existing product.
//!
//! SQLite/streaming/model selection/approvals remain host adapters. This module
//! owns lifecycle admission, source-bound tool intent, ordered read barriers,
//! durable receipts and recovery. There is no HTTP call or second agent engine.
use crate::{budget::BudgetLedger,domain::{CreateRun,Run,RunStatus,ToolCall},journal,tool_schema};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{future::Future,pin::Pin,ops::{Deref,DerefMut}};

pub type BoxFuture<'a,T>=Pin<Box<dyn Future<Output=T>+Send+'a>>;
pub use crate::domain::RunStatus as Status;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Receipt{pub id:String,pub tool:String,pub arguments:Value,pub success:bool,pub result:Value,pub step:usize}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Snapshot{
    /// Canonical sub4 Run; every effect transition uses journal.rs.
    pub native:Run,
    pub model_calls:usize,
    pub charged_tokens:u64,
    pub step:usize,
    pub receipts:Vec<Receipt>,
    pub model_in_flight:bool,
}
impl Deref for Snapshot{type Target=Run;fn deref(&self)->&Run{&self.native}}
impl DerefMut for Snapshot{fn deref_mut(&mut self)->&mut Run{&mut self.native}}
impl Snapshot{
    pub fn new(id:&str,instruction:&str,max_steps:usize)->Result<Self,String>{
        let mut native=Run::new(CreateRun{instruction:instruction.into(),budget_krw:None,max_steps:Some(max_steps.min(100).max(1))},max_steps)?;
        native.id=uuid::Uuid::parse_str(id).unwrap_or_else(|_|{use sha2::{Digest,Sha256};let hash=Sha256::digest(id.as_bytes());let mut bytes=[0;16];bytes.copy_from_slice(&hash[..16]);uuid::Uuid::from_bytes(bytes)});
        native.status=RunStatus::Running;
        Ok(Self{native,model_calls:0,charged_tokens:0,step:0,receipts:vec![],model_in_flight:false})
    }
    /// One source of truth for terminal readiness. Pending approvals and
    /// delegated children are unsettled even when no tool is currently in
    /// flight; they may not be bypassed by a budget/receipt shortcut.
    pub fn settled(&self) -> bool {
        !self.model_in_flight
            && self.native.in_flight.is_none()
            && self.native.read_batch_in_flight.is_empty()
            && self.native.queued_calls.is_empty()
            && self.native.pending_approval.is_none()
            && !self.native.cancel_after_tool
            && !self.native.waiting_for_children
    }
    /// A new model decision cannot overtake an uncertain tool or paid call.
    pub fn next_round(&mut self,budget:&mut BudgetLedger,store:&dyn Journal)->Result<Option<usize>,String>{
        if store.cancelled(&self.native.id.to_string()){return Err("실행이 중단되었습니다.".into());}
        if self.native.terminal(){return Ok(None);}
        if !self.settled() {
            return Err("보류 중인 도구·승인·하위 작업의 결과를 먼저 확인해야 합니다.".into());
        }
        let round=budget.next_round();
        if let Some(round)=round{self.step=round+1;self.native.steps=round+1;store.save(self)?;}
        Ok(round)
    }
    pub fn begin_model(&mut self,store:&dyn Journal)->Result<(),String>{
        if self.native.status!=RunStatus::Running{return Err("실행 중인 작업만 모델을 호출할 수 있습니다.".into());}
        if !self.settled(){return Err("이전 실행·승인·하위 작업 결과가 확인되지 않았습니다.".into());}
        if store.cancelled(&self.native.id.to_string()){return Err("실행이 중단되었습니다.".into());}
        self.model_in_flight=true;self.model_calls+=1;store.save(self)
    }
    pub fn finish_model(&mut self,store:&dyn Journal)->Result<(),String>{self.model_in_flight=false;store.save(self)}
    pub fn complete(&mut self,answer:String,store:&dyn Journal)->Result<(),String>{
        // A stop or owner cancellation can race with the final verified
        // checkpoint. Never turn a cancelled/failed run back into completed.
        if store.cancelled(&self.native.id.to_string()){
            return Err("실행이 중단되었습니다.".into());
        }
        if self.native.status!=RunStatus::Running {
            return Err("실행 중인 작업만 완료 상태로 전환할 수 있습니다.".into());
        }
        if !self.settled(){return Err("보류 중인 작업·승인·하위 작업을 건너뛰어 완료할 수 없습니다.".into());}
        self.native.status=RunStatus::Completed;self.native.answer=Some(answer);store.save(self)
    }
}
pub trait Journal:Send+Sync{
    fn save(&self,snapshot:&Snapshot)->Result<(),String>;
    fn cancelled(&self,_id:&str)->bool{false}
}
#[derive(Clone,Debug)]
pub struct Action{pub id:String,pub tool:String,pub arguments:Value,pub depends_on:Vec<String>}
pub struct DispatchResult<T>{pub output:T,pub observation:Value,pub success:bool}
pub trait Dispatcher:Send+Sync{
    type Output:Send;
    fn execute<'a>(&'a self,action:&'a Action)->BoxFuture<'a,Result<DispatchResult<Self::Output>,String>>;
    fn definitions(&self)->Vec<Value>{vec![]}
}
/// Only contiguous local observations may run together. A write/shell/network
/// operation is an exclusive barrier even if a caller requests parallel work.
pub fn read_batch_end<T>(calls:&[T],start:usize,max:usize,read:impl Fn(&T)->bool)->usize{
    if start>=calls.len(){return start;}
    if !read(&calls[start]){return start+1;}
    let mut end=start+1;
    while end<calls.len() && end-start<max.max(1) && read(&calls[end]){end+=1;}
    end
}
fn succeeded(value:&Value)->bool{
    if value["isError"]==true||value["ok"]==false||value["success"]==false
        ||value.get("error").is_some_and(|v|!v.is_null()&&v.as_str()!=Some("")){return false;}
    if ["statusCode","httpStatus"].iter().any(|k|value[k].as_u64().is_some_and(|status|status>=400)){return false;}
    if ["exitCode","exit_code","exit"].iter().any(|k|value[k].as_i64().is_some_and(|v|v!=0)){return false;}
    !value["status"].as_str().is_some_and(|v|matches!(v,"error"|"failed"|"cancelled"|"denied"|"timeout"))
}
/// Record intent, dispatch once, and atomically replace intent with receipts.
/// Storage failure before dispatch prevents effects. Storage failure afterwards
/// leaves the durable intent uncertain; recovery never replays that mutation.
pub async fn dispatch_batch<D:Dispatcher>(snapshot:&mut Snapshot,actions:&[Action],dispatcher:&D,store:&dyn Journal)->Result<Vec<Result<D::Output,String>>,String>{
    if snapshot.native.status!=RunStatus::Running||snapshot.model_in_flight{return Err("현재 실행 상태에서 도구를 시작할 수 없습니다.".into());}
    if store.cancelled(&snapshot.native.id.to_string()){return Err("실행이 중단되었습니다.".into());}
    if !snapshot.settled(){return Err("이전 도구·승인·하위 작업의 결과를 먼저 확인해야 합니다.".into());}
    let calls=actions.iter().map(|a|ToolCall{id:a.id.clone(),name:a.tool.clone(),arguments:a.arguments.clone()}).collect::<Vec<_>>();
    journal::validate_new_calls(&snapshot.native,&calls)?;
    let definitions=if snapshot.native.advertised_tools.is_empty(){dispatcher.definitions()}else{snapshot.native.advertised_tools.clone()};
    // Admission is a tool-level outcome, not a turn-level abort: a model that
    // names a tool outside the advertised catalog gets the error back and can
    // retry with discovery. Dispatch is still never called for that call.
    let admission=actions.iter().zip(&calls).map(|(_,call)|{
        if definitions.is_empty(){return Ok(());}
        tool_schema::admit(call,&definitions).map_err(|e|format!("{}: {}",e.code.as_str(),e.reason))
    }).collect::<Vec<Result<(),String>>>();
    for (action,rejected) in actions.iter().zip(&admission){
        if rejected.is_err(){continue;}
        if action.depends_on.iter().any(|id|!snapshot.receipts.iter().any(|r|&r.id==id&&r.success)){
            return Err(format!("도구 {}의 선행 작업이 성공하지 않았습니다.",action.id));
        }
    }
    snapshot.native.queued_calls=calls.clone();
    snapshot.native.messages.push(json!({"role":"assistant","content":"","tool_calls":calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments.to_string()}})).collect::<Vec<_>>()}));
    let parallel=calls.len()>1&&admission.iter().all(Result::is_ok)
        &&calls.iter().all(|c|journal::parallel_safe(&c.name));
    if parallel{
        // A model turn can legally request 16 calls, while a durable read
        // barrier admits at most eight. Partition into independently journaled
        // waves: a crash never loses which wave completed, and a cancellation
        // between waves must not launch the remaining reads.
        let mut outputs=Vec::with_capacity(actions.len());
        for (wave,calls_in_wave) in calls.chunks(8).enumerate(){
            if snapshot.native.status!=RunStatus::Running {
                return Err("실행이 중단되었습니다.".into());
            }
            if store.cancelled(&snapshot.native.id.to_string()){
                let skipped=journal::cancel_unstarted(&mut snapshot.native)?;
                for call in skipped {
                    snapshot.receipts.push(Receipt{id:call.id,tool:call.name,arguments:call.arguments,
                        success:false,result:json!({"error":"cancelled_before_dispatch"}),step:snapshot.step});
                }
                store.save(snapshot)?;
                return Err("실행이 중단되었습니다.".into());
            }
            let offset=wave*8;
            let actions_in_wave=&actions[offset..offset+calls_in_wave.len()];
            if calls_in_wave.len()==1 {
                // The last wave of a 9-call turn is a regular durable read,
                // not an invalid one-element parallel barrier.
                let action=&actions_in_wave[0];
                let call=&calls_in_wave[0];
                journal::begin(&mut snapshot.native,call)?;store.save(snapshot)?;
                let result=dispatcher.execute(action).await?;
                let success=result.success&&succeeded(&result.observation);
                let receipt=if success{Ok(result.observation.to_string())}else{Err(result.observation.to_string())};
                journal::finish(&mut snapshot.native,call,&receipt,false)?;
                snapshot.receipts.push(Receipt{id:action.id.clone(),tool:action.tool.clone(),
                    arguments:action.arguments.clone(),success,result:result.observation,step:snapshot.step});
                store.save(snapshot)?;
                outputs.push(Ok(result.output));
                continue;
            }
            journal::begin_read_batch(&mut snapshot.native,calls_in_wave)?;store.save(snapshot)?;
            let results=futures_util::future::join_all(
                actions_in_wave.iter().map(|a|dispatcher.execute(a))).await;
            let observations=results.iter().map(|r|match r{
                Ok(r) if r.success&&succeeded(&r.observation)=>Ok(r.observation.to_string()),
                Ok(r)=>Err(r.observation.to_string()),
                Err(e)=>Err(e.clone())
            }).collect::<Vec<_>>();
            journal::finish_read_batch(&mut snapshot.native,calls_in_wave,&observations)?;
            for ((action,result),observed) in actions_in_wave.iter().zip(&results).zip(&observations){
                snapshot.receipts.push(Receipt{id:action.id.clone(),tool:action.tool.clone(),
                    arguments:action.arguments.clone(),success:observed.is_ok(),
                    result:match result{Ok(r)=>r.observation.clone(),Err(e)=>json!({"error":e})},step:snapshot.step});
            }
            store.save(snapshot)?;
            outputs.extend(results.into_iter().map(|r|r.map(|r|r.output)));
        }
        return Ok(outputs);
    }
    let mut outputs=Vec::new();
    for ((action,call),rejected) in actions.iter().zip(&calls).zip(&admission){
        if store.cancelled(&snapshot.native.id.to_string()){
            let skipped=journal::cancel_unstarted(&mut snapshot.native)?;
            for call in skipped {
                snapshot.receipts.push(Receipt{id:call.id,tool:call.name,arguments:call.arguments,
                    success:false,result:json!({"error":"cancelled_before_dispatch"}),step:snapshot.step});
            }
            store.save(snapshot)?;
            return Err("실행이 중단되었습니다.".into());
        }
        if let Err(error)=rejected{
            journal::reject(&mut snapshot.native,call,error)?;
            snapshot.receipts.push(Receipt{id:action.id.clone(),tool:action.tool.clone(),
                arguments:action.arguments.clone(),success:false,
                result:json!({"error":error}),step:snapshot.step});
            store.save(snapshot)?;
            outputs.push(Err(error.clone()));
            continue;
        }
        journal::begin(&mut snapshot.native,call)?;store.save(snapshot)?;
        // A host error cannot establish whether an external effect happened.
        let result=dispatcher.execute(action).await?;
        let success=result.success&&succeeded(&result.observation);
        let receipt=if success{Ok(result.observation.to_string())}else{Err(result.observation.to_string())};
        journal::finish(&mut snapshot.native,call,&receipt,false)?;
        snapshot.receipts.push(Receipt{id:action.id.clone(),tool:action.tool.clone(),arguments:action.arguments.clone(),success,result:result.observation,step:snapshot.step});
        store.save(snapshot)?;outputs.push(Ok(result.output));
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests{
    use super::*;
    use std::sync::{Mutex,atomic::{AtomicUsize,Ordering}};
    struct Memory{saved:Mutex<Option<Snapshot>>,fail:bool}
    impl Journal for Memory{fn save(&self,s:&Snapshot)->Result<(),String>{if self.fail{return Err("disk failure".into());}*self.saved.lock().unwrap()=Some(s.clone());Ok(())}}
    struct Tool(AtomicUsize);
    impl Dispatcher for Tool{type Output=();fn execute<'a>(&'a self,_:&'a Action)->BoxFuture<'a,Result<DispatchResult<()>,String>>{Box::pin(async move{self.0.fetch_add(1,Ordering::SeqCst);Ok(DispatchResult{output:(),observation:json!({"written":true}),success:true})})}}
    fn run()->Snapshot{Snapshot::new(&uuid::Uuid::new_v4().to_string(),"create a file",8).unwrap()}
    fn action()->Action{Action{id:"c".into(),tool:"workspace_write".into(),arguments:json!({"path":"a","content":""}),depends_on:vec![]}}
    #[test]
    fn non_running_snapshot_cannot_admit_paid_model_call() {
        for status in [RunStatus::Queued, RunStatus::AwaitingApproval, RunStatus::Paused,
            RunStatus::Completed, RunStatus::Failed, RunStatus::Cancelled] {
            let mut snapshot = run();
            snapshot.native.status = status;
            let journal = Memory { saved: Mutex::new(None), fail: false };
            assert!(snapshot.begin_model(&journal).is_err());
            assert_eq!(snapshot.model_calls, 0);
            assert!(!snapshot.model_in_flight);
            assert!(journal.saved.lock().unwrap().is_none());
        }
    }
    #[test]
    fn cancelled_native_run_cannot_be_revived_by_late_completion() {
        struct CancelJournal;
        impl Journal for CancelJournal {
            fn save(&self,_:&Snapshot)->Result<(),String>{Ok(())}
            fn cancelled(&self,_:&str)->bool{true}
        }
        let mut snapshot=run();
        assert!(snapshot.complete("late".into(),&CancelJournal).is_err());
        assert_eq!(snapshot.native.status,RunStatus::Running);
        snapshot.native.status=RunStatus::Cancelled;
        let journal=Memory{saved:Mutex::new(None),fail:false};
        assert!(snapshot.complete("revive".into(),&journal).is_err());
        assert_eq!(snapshot.native.status,RunStatus::Cancelled);
    }

    #[test]
    fn pending_approval_and_unsettled_child_block_paid_calls_and_false_completion() {
        let journal=Memory{saved:Mutex::new(None),fail:false};
        let mut snapshot=run();
        snapshot.native.pending_approval=Some(ToolCall{
            id:"approve".into(),name:"workspace_write".into(),
            arguments:json!({"path":"x"})
        });
        assert!(!snapshot.settled());
        let mut budget=BudgetLedger::new(8,8,std::time::Duration::from_secs(60));
        assert!(snapshot.next_round(&mut budget,&journal).is_err());
        assert!(snapshot.begin_model(&journal).is_err());
        assert!(snapshot.complete("false".into(),&journal).is_err());
        snapshot.native.pending_approval=None;
        snapshot.native.waiting_for_children=true;
        assert!(!snapshot.settled());
        assert!(snapshot.next_round(&mut budget,&journal).is_err());
        assert!(snapshot.complete("false".into(),&journal).is_err());
        snapshot.native.waiting_for_children=false;
        assert!(snapshot.settled());
        assert_eq!(snapshot.next_round(&mut budget,&journal).unwrap(),Some(0));
    }

    #[tokio::test]
    async fn sixteen_read_calls_complete_as_ordered_durable_waves(){
        for count in [9usize,16] {
            let mut snapshot=run();
            let dispatcher=Tool(AtomicUsize::new(0));
            let store=Memory{saved:Mutex::new(None),fail:false};
            let actions=(0..count).map(|index|Action{
                id:format!("read-{index}"),tool:"read_file".into(),
                arguments:json!({"path":format!("file-{index}")}),depends_on:vec![],
            }).collect::<Vec<_>>();
            let outputs=dispatch_batch(&mut snapshot,&actions,&dispatcher,&store).await.unwrap();
            assert_eq!(outputs.len(),count);
            assert_eq!(dispatcher.0.load(Ordering::SeqCst),count);
            assert!(snapshot.native.queued_calls.is_empty());
            assert!(snapshot.native.read_batch_in_flight.is_empty());
            assert_eq!(snapshot.receipts.len(),count);
            let observed=snapshot.native.messages.iter()
                .filter(|message|message["role"]=="tool")
                .map(|message|message["tool_call_id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>();
            assert_eq!(observed,actions.iter().map(|a|a.id.clone()).collect::<Vec<_>>());
            assert_eq!(snapshot.native.events.iter()
                .filter(|event|event.kind=="read_batch_started").count(),
                if count==9 {1} else {2});
            assert_eq!(store.saved.lock().unwrap().as_ref().unwrap().receipts.len(),count);
        }
    }

    #[tokio::test]
    async fn cancelling_between_read_waves_aborts_tail_without_dispatch(){
        use std::sync::{Arc,atomic::AtomicBool};
        struct StopJournal{saved:Mutex<Option<Snapshot>>,stop:Arc<AtomicBool>}
        impl Journal for StopJournal{
            fn save(&self,s:&Snapshot)->Result<(),String>{
                *self.saved.lock().unwrap()=Some(s.clone());Ok(())
            }
            fn cancelled(&self,_:&str)->bool{self.stop.load(Ordering::SeqCst)}
        }
        struct StopOnRead{count:AtomicUsize,stop:Arc<AtomicBool>}
        impl Dispatcher for StopOnRead{
            type Output=();
            fn execute<'a>(&'a self,_:&'a Action)->BoxFuture<'a,Result<DispatchResult<()>,String>>{
                Box::pin(async move {
                    if self.count.fetch_add(1,Ordering::SeqCst)==0{
                        self.stop.store(true,Ordering::SeqCst);
                    }
                    Ok(DispatchResult{output:(),observation:json!({"ok":true}),success:true})
                })
            }
        }
        let stop=Arc::new(AtomicBool::new(false));
        let journal=StopJournal{saved:Mutex::new(None),stop:stop.clone()};
        let dispatcher=StopOnRead{count:AtomicUsize::new(0),stop};
        let mut snapshot=run();
        let actions=(0..9).map(|i|Action{id:format!("r{i}"),
            tool:"read_file".into(),arguments:json!({"path":i}),depends_on:vec![]})
            .collect::<Vec<_>>();
        assert!(dispatch_batch(&mut snapshot,&actions,&dispatcher,&journal).await.is_err());
        assert_eq!(dispatcher.count.load(Ordering::SeqCst),8);
        assert_eq!(snapshot.native.status,RunStatus::Cancelled);
        assert!(snapshot.native.queued_calls.is_empty());
        assert_eq!(snapshot.receipts.len(),9);
        assert_eq!(snapshot.receipts.iter().filter(|receipt|receipt.success).count(),8);
        assert_eq!(snapshot.native.messages.iter().filter(|m|m["role"]=="tool").count(),9);
        assert_eq!(journal.saved.lock().unwrap().as_ref().unwrap().receipts.len(),9);
    }

    #[tokio::test]
    async fn a_tool_outside_the_advertised_catalog_is_rejected_without_dispatch(){
        let mut s=run();
        s.native.advertised_tools=vec![json!({"type":"function","function":{
            "name":"dominion_tool_search","description":"d",
            "parameters":{"type":"object","properties":{},"additionalProperties":false}}})];
        let d=Tool(AtomicUsize::new(0));
        let j=Memory{saved:Mutex::new(None),fail:false};
        let a=Action{id:"c".into(),tool:"workspace_read".into(),
            arguments:json!({"path":"a"}),depends_on:vec![]};
        let results=dispatch_batch(&mut s,&[a],&d,&j).await.unwrap();
        assert_eq!(results.len(),1);
        assert!(results[0].is_err(),"the model must receive the rejection, not a dead turn");
        assert_eq!(d.0.load(Ordering::SeqCst),0,"a rejected tool never reaches the executor");
        assert!(s.native.queued_calls.is_empty());
        assert_eq!(s.receipts.len(),1);
        assert!(!s.receipts[0].success);
        assert!(s.native.messages.iter().any(|m|m["role"]=="tool"&&m["tool_call_id"]=="c"));
    }

    #[tokio::test]
    async fn failed_write_ahead_storage_prevents_dispatch(){let mut s=run();let d=Tool(AtomicUsize::new(0));let j=Memory{saved:Mutex::new(None),fail:true};assert!(dispatch_batch(&mut s,&[action()],&d,&j).await.is_err());assert_eq!(d.0.load(Ordering::SeqCst),0);}

    #[tokio::test]
    async fn cancelled_serial_batch_records_skipped_tools_without_dispatch(){
        use std::sync::{Arc,atomic::AtomicBool};
        struct CancelJournal{saved:Mutex<Option<Snapshot>>,stop:Arc<AtomicBool>}
        impl Journal for CancelJournal{
            fn save(&self,s:&Snapshot)->Result<(),String>{
                *self.saved.lock().unwrap()=Some(s.clone());Ok(())
            }
            fn cancelled(&self,_:&str)->bool{self.stop.load(Ordering::SeqCst)}
        }
        struct FirstThenCancel{count:AtomicUsize,stop:Arc<AtomicBool>}
        impl Dispatcher for FirstThenCancel{
            type Output=();
            fn execute<'a>(&'a self,_:&'a Action)->BoxFuture<'a,Result<DispatchResult<()>,String>>{
                Box::pin(async move{
                    self.count.fetch_add(1,Ordering::SeqCst);
                    self.stop.store(true,Ordering::SeqCst);
                    Ok(DispatchResult{output:(),observation:json!({"ok":true}),success:true})
                })
            }
        }
        let stop=Arc::new(AtomicBool::new(false));
        let journal=CancelJournal{saved:Mutex::new(None),stop:stop.clone()};
        let tool=FirstThenCancel{count:AtomicUsize::new(0),stop};
        let mut snapshot=run();
        let mut a=action();a.id="a".into();
        let mut b=action();b.id="b".into();
        let result=dispatch_batch(&mut snapshot,&[a,b],&tool,&journal).await;
        assert!(result.is_err());
        assert_eq!(tool.count.load(Ordering::SeqCst),1);
        assert_eq!(snapshot.native.status,RunStatus::Cancelled);
        assert!(snapshot.native.queued_calls.is_empty());
        assert_eq!(snapshot.receipts.len(),2);
        assert!(snapshot.receipts[0].success);
        assert!(!snapshot.receipts[1].success);
        let saved=journal.saved.lock().unwrap();
        assert_eq!(saved.as_ref().unwrap().native.status,RunStatus::Cancelled);
        for id in ["a","b"]{
            assert_eq!(snapshot.native.messages.iter().filter(|m|
                m["role"]=="tool"&&m["tool_call_id"]==id).count(),1);
        }
    }
    #[tokio::test]
    async fn native_journal_finishes_exact_call_and_blocks_replay(){let mut s=run();let d=Tool(AtomicUsize::new(0));let j=Memory{saved:Mutex::new(None),fail:false};dispatch_batch(&mut s,&[action()],&d,&j).await.unwrap();assert!(s.native.in_flight.is_none());assert!(s.native.queued_calls.is_empty());assert_eq!(s.receipts.len(),1);assert!(dispatch_batch(&mut s,&[action()],&d,&j).await.is_err());assert_eq!(d.0.load(Ordering::SeqCst),1);}
}
