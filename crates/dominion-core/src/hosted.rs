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
    /// A new model decision cannot overtake an uncertain tool or paid call.
    pub fn next_round(&mut self,budget:&mut BudgetLedger,store:&dyn Journal)->Result<Option<usize>,String>{
        if store.cancelled(&self.native.id.to_string()){return Err("실행이 중단되었습니다.".into());}
        if self.native.terminal(){return Ok(None);}
        if self.model_in_flight || self.native.in_flight.is_some() || !self.native.read_batch_in_flight.is_empty(){
            return Err("결과가 불명확한 실행을 먼저 확인해야 합니다.".into());
        }
        if !self.native.queued_calls.is_empty(){return Err("대기 중인 도구를 처리하기 전에 모델을 다시 호출할 수 없습니다.".into());}
        let round=budget.next_round();
        if let Some(round)=round{self.step=round;self.native.steps=round;store.save(self)?;}
        Ok(round)
    }
    pub fn begin_model(&mut self,store:&dyn Journal)->Result<(),String>{
        if self.model_in_flight||self.native.in_flight.is_some()||!self.native.read_batch_in_flight.is_empty(){return Err("이전 실행 결과가 확인되지 않았습니다.".into());}
        if store.cancelled(&self.native.id.to_string()){return Err("실행이 중단되었습니다.".into());}
        self.model_in_flight=true;self.model_calls+=1;store.save(self)
    }
    pub fn finish_model(&mut self,store:&dyn Journal)->Result<(),String>{self.model_in_flight=false;store.save(self)}
    pub fn complete(&mut self,answer:String,store:&dyn Journal)->Result<(),String>{
        if self.model_in_flight||self.native.in_flight.is_some()||!self.native.read_batch_in_flight.is_empty()||!self.native.queued_calls.is_empty(){return Err("진행 중인 작업의 결과 없이 완료할 수 없습니다.".into());}
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
    if snapshot.native.in_flight.is_some()||!snapshot.native.read_batch_in_flight.is_empty()||!snapshot.native.queued_calls.is_empty(){return Err("이전 도구의 결과를 먼저 확인해야 합니다.".into());}
    let calls=actions.iter().map(|a|ToolCall{id:a.id.clone(),name:a.tool.clone(),arguments:a.arguments.clone()}).collect::<Vec<_>>();
    journal::validate_new_calls(&snapshot.native,&calls)?;
    let definitions=if snapshot.native.advertised_tools.is_empty(){dispatcher.definitions()}else{snapshot.native.advertised_tools.clone()};
    for (action,call) in actions.iter().zip(&calls){
        if !definitions.is_empty(){tool_schema::admit(call,&definitions).map_err(|e|format!("{}: {}",e.code.as_str(),e.reason))?;}
        if action.depends_on.iter().any(|id|!snapshot.receipts.iter().any(|r|&r.id==id&&r.success)){
            return Err(format!("도구 {}의 선행 작업이 성공하지 않았습니다.",action.id));
        }
    }
    snapshot.native.queued_calls=calls.clone();
    snapshot.native.messages.push(json!({"role":"assistant","content":"","tool_calls":calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments.to_string()}})).collect::<Vec<_>>()}));
    let parallel=calls.len()>1&&calls.iter().all(|c|journal::parallel_safe(&c.name));
    if parallel{
        journal::begin_read_batch(&mut snapshot.native,&calls)?;store.save(snapshot)?;
        let results=futures_util::future::join_all(actions.iter().map(|a|dispatcher.execute(a))).await;
        let observations=results.iter().map(|r|match r{Ok(r) if r.success&&succeeded(&r.observation)=>Ok(r.observation.to_string()),Ok(r)=>Err(r.observation.to_string()),Err(e)=>Err(e.clone())}).collect::<Vec<_>>();
        journal::finish_read_batch(&mut snapshot.native,&calls,&observations)?;
        for ((action,result),observed) in actions.iter().zip(&results).zip(&observations){snapshot.receipts.push(Receipt{id:action.id.clone(),tool:action.tool.clone(),arguments:action.arguments.clone(),success:observed.is_ok(),result:match result{Ok(r)=>r.observation.clone(),Err(e)=>json!({"error":e})},step:snapshot.step});}
        store.save(snapshot)?;
        return Ok(results.into_iter().map(|r|r.map(|r|r.output)).collect());
    }
    let mut outputs=Vec::new();
    for (action,call) in actions.iter().zip(&calls){
        if store.cancelled(&snapshot.native.id.to_string()){store.save(snapshot)?;return Err("실행이 중단되었습니다.".into());}
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
    #[tokio::test]
    async fn failed_write_ahead_storage_prevents_dispatch(){let mut s=run();let d=Tool(AtomicUsize::new(0));let j=Memory{saved:Mutex::new(None),fail:true};assert!(dispatch_batch(&mut s,&[action()],&d,&j).await.is_err());assert_eq!(d.0.load(Ordering::SeqCst),0);}
    #[tokio::test]
    async fn native_journal_finishes_exact_call_and_blocks_replay(){let mut s=run();let d=Tool(AtomicUsize::new(0));let j=Memory{saved:Mutex::new(None),fail:false};dispatch_batch(&mut s,&[action()],&d,&j).await.unwrap();assert!(s.native.in_flight.is_none());assert!(s.native.queued_calls.is_empty());assert_eq!(s.receipts.len(),1);assert!(dispatch_batch(&mut s,&[action()],&d,&j).await.is_err());assert_eq!(d.0.load(Ordering::SeqCst),1);}
}
