//! Execution controls, events and hooks. No alternate agent loop.
use super::{run_internal,run_with_options,Event,NativeFuture,NativeHost,RunRequest,RunResult,SUSPENDED};
use serde_json::Value;
use std::{future::Future,pin::Pin,time::Duration};
use tokio::sync::{mpsc,watch};
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum ModelDelta{Text(String),ToolCall{index:usize,id:Option<String>,name:Option<String>,arguments_delta:String}}
/// An external operation boundary, not an internal CPU instruction.
#[derive(Clone,Debug)]
pub enum Operation{Model,Tool{id:String,name:String,arguments:Value},Evidence{criterion:Value},Verify}
#[derive(Clone,Debug)]
pub struct HookContext{pub run_id:String,pub operation:Operation,pub model_calls:usize,pub tool_calls:usize}
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum HookDecision{Allow,Reject(String)}
/// Await approval or veto admission. Allow cannot override native permissions.
pub trait NativeHook:Send+Sync{
    fn before(&self,_context:HookContext)->NativeFuture<'_,HookDecision>{Box::pin(async{Ok(HookDecision::Allow)})}
    fn event(&self,_run_id:&str,_event:&Event){}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Mode{Running,Paused,Steps(usize),Suspending,Cancelled}
#[derive(Clone)]
pub struct RunControl{tx:watch::Sender<Mode>}
impl Default for RunControl{fn default()->Self{Self::new()}}
impl RunControl{
    pub fn new()->Self{let(tx,_)=watch::channel(Mode::Running);Self{tx}}
    fn set(&self,mode:Mode){self.tx.send_if_modified(|m|{if *m==Mode::Cancelled{return false;}*m=mode;true});}
    pub fn pause(&self){self.set(Mode::Paused);}
    pub fn continue_run(&self){self.set(Mode::Running);}
    pub fn step(&self){self.step_n(1);}
    pub fn step_n(&self,n:usize){
        if n==0{return;}
        self.tx.send_if_modified(|m|{if *m==Mode::Cancelled{return false;}*m=Mode::Steps(match *m{Mode::Steps(k)=>k.saturating_add(n),_=>n});true});
    }
    /// Finish current batch, then durably suspend before the next model call.
    /// Unlike pause(), this permits pending effects to settle. May finish
    /// instead if no further model call is needed.
    pub fn suspend(&self){self.set(Mode::Suspending);}
    pub fn cancel(&self){self.tx.send_replace(Mode::Cancelled);}
    pub fn is_cancelled(&self)->bool{*self.tx.borrow()==Mode::Cancelled}
    pub fn is_paused(&self)->bool{*self.tx.borrow()==Mode::Paused}
    pub(super) async fn wait(&self,op:&Operation,check:impl Fn()->Result<(),String>)->Result<bool,String>{
        let mut rx=self.tx.subscribe();
        loop{
            check()?;
            let mut result=None;
            self.tx.send_if_modified(|m|{
                match *m{
                    Mode::Running=>result=Some(Ok(false)),Mode::Paused=>{},
                    Mode::Steps(n)=>{result=Some(Ok(false));*m=if n<=1{Mode::Paused}else{Mode::Steps(n-1)};return true;},
                    Mode::Suspending=>result=Some(Ok(matches!(op,Operation::Model))),
                    Mode::Cancelled=>result=Some(Err("native run cancelled".into())),
                }
                false
            });
            if let Some(result)=result{return result;}
            let _=tokio::time::timeout(Duration::from_millis(100),rx.changed()).await;
        }
    }
}
pub struct RunOptions<'a>{
    pub control:Option<RunControl>,pub hooks:Vec<&'a dyn NativeHook>,pub streaming:bool,
    pub max_verification_retries:usize,
    /// Opt-in provider projection owned by the SDK, not by each host.
    pub provider_context:Option<super::ProviderContext>,
    pub(super) events:Option<mpsc::UnboundedSender<Event>>,
}
impl Default for RunOptions<'_>{fn default()->Self{Self{control:None,hooks:vec![],streaming:false,max_verification_retries:2,provider_context:None,events:None}}}
/// Poll result and drain events concurrently. No detached task. The channel
/// is unbounded to avoid losing provider deltas; consumers must drain it.
pub struct ChannelRun<'a>{
    pub control:RunControl,pub events:mpsc::UnboundedReceiver<Event>,
    pub result:Pin<Box<dyn Future<Output=Result<RunResult,String>>+'a>>,
}
fn channel<'a>(host:&'a dyn NativeHost,request:RunRequest,mut options:RunOptions<'a>,resume:bool)->ChannelRun<'a>{
    let control=options.control.clone().unwrap_or_default();options.control=Some(control.clone());
    let(tx,events)=mpsc::unbounded_channel();options.events=Some(tx.clone());
    let result=Box::pin(async move{
        let result=run_internal(host,request,options,resume).await;
        if let Err(error)=&result{if error!=SUSPENDED{let _=tx.send(Event::Failed{error:error.clone()});}}
        result
    });
    ChannelRun{control,events,result}
}
pub fn run_channel<'a>(host:&'a dyn NativeHost,request:RunRequest,options:RunOptions<'a>)->ChannelRun<'a>{channel(host,request,options,false)}
pub fn run_streamed<'a>(host:&'a dyn NativeHost,request:RunRequest,mut options:RunOptions<'a>)->Result<ChannelRun<'a>,String>{
    if !host.supports_streaming(){return Err("host has no streaming model port".into());}options.streaming=true;
    Ok(channel(host,request,options,false))
}
pub fn resume_channel<'a>(host:&'a dyn NativeHost,request:RunRequest,options:RunOptions<'a>)->ChannelRun<'a>{channel(host,request,options,true)}
pub fn run_sync(host:&dyn NativeHost,request:RunRequest)->Result<RunResult,String>{run_sync_with_options(host,request,RunOptions::default())}
pub fn run_sync_with_options(host:&dyn NativeHost,request:RunRequest,options:RunOptions<'_>)->Result<RunResult,String>{
    if tokio::runtime::Handle::try_current().is_ok(){return Err("run_sync cannot block inside an async runtime; await run".into());}
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e|e.to_string())?.block_on(run_with_options(host,request,options))
}
