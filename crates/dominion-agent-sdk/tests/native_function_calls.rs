//! A provider produces typed function calls, not a JSON message claiming to
//! be a tool call. The Rust kernel must dispatch them with normal approvals,
//! receipt persistence, dependency checks, and terminal verification.
use dominion_agent_sdk::{
    Action,BoxFuture,Criterion,CriterionKind,Decision,Evidence,Journal,Kernel,
    Limits,Model,ModelReply,ModelTurn,Receipt,RunRequest,Snapshot,Status,Strategy,
    ToolSpec,Tools,
};
use serde_json::{json,Value};
use std::{collections::VecDeque,sync::Mutex};

struct NativeProvider {queue:Mutex<VecDeque<Decision>>, calls:Mutex<usize>}
impl Model for NativeProvider {
    fn complete<'a>(&'a self,_:&'a str,_:&'a str)
        ->BoxFuture<'a,Result<ModelReply,String>> {
        Box::pin(async {Err("JSON text planner must not be used for native tool calls".into())})
    }
    fn complete_with_tools<'a>(&'a self,_system:&'a str,_prompt:&'a str,
        tools:&'a [ToolSpec],_history:&'a [Receipt])
        ->BoxFuture<'a,Result<ModelTurn,String>> {
        Box::pin(async move {
            assert!(tools.iter().any(|s|s.name=="workspace_write"));
            *self.calls.lock().unwrap()+=1;
            let decision=self.queue.lock().unwrap().pop_front().ok_or("turn exhausted")?;
            Ok(ModelTurn{reply:ModelReply{content:"native-model-output".into(),
                input_tokens:Some(10),output_tokens:Some(5)},
                decision:Some(decision)})
        })
    }
}
struct Writer {called:Mutex<Vec<String>>}
impl Tools for Writer {
    fn definitions(&self)->Vec<ToolSpec> {
        vec![ToolSpec{name:"workspace_write".into(),description:"Write".into(),
            schema:json!({"type":"object"}),read_only:false}]
    }
    fn execute<'a>(&'a self,action:&'a Action)
        ->BoxFuture<'a,Result<Value,String>> {
        Box::pin(async move {
            self.called.lock().unwrap().push(action.id.clone());
            Ok(json!({"written":true,"path":action.arguments["path"]}))
        })
    }
}
struct NoFileChecks;
impl Evidence for NoFileChecks {
    fn check<'a>(&'a self,_:&'a Criterion)
        ->BoxFuture<'a,Result<bool,String>>{Box::pin(async{Ok(true)})}
}
#[derive(Default)]
struct JournalMem {snapshots:Mutex<Vec<Snapshot>>}
impl Journal for JournalMem {
    fn save(&self,s:&Snapshot)->Result<(),String>{
        self.snapshots.lock().unwrap().push(s.clone());Ok(())
    }
}
fn request()->RunRequest {
    RunRequest{id:"native-wire".into(),
        instruction:"Implement code in a source file".into(),
        owner_instructions:"Preserve tool evidence".into(),
        strategy:Strategy::Balanced,limits:Limits{
            max_steps:6,max_model_calls:6,max_tool_calls:96,max_estimated_tokens:20_000,
            max_parallel_reads:2,max_result_chars:1500},
        allowed_tools:vec!["workspace_write".into()],checks:vec![],
        require_evidence:true}
}
fn act(id:&str,name:&str)->Action {
    Action{id:id.into(),tool:name.into(),
        arguments:json!({"path":"src/a.rs","content":"implemented"}),
        depends_on:vec![]}
}
#[tokio::test]
async fn provider_native_function_call_executes_and_completes_without_json_decode() {
    let provider=NativeProvider{
        queue:Mutex::new(VecDeque::from(vec![
            Decision{plan:vec!["Write source".into()],
                actions:vec![act("call_native_1","workspace_write")],answer:None,candidate_paths:vec![]},
            Decision{plan:vec![],actions:vec![],answer:Some("Completed source".into()),candidate_paths:vec![]},
        ])),calls:Mutex::new(0)};
    let writer=Writer{called:Mutex::new(vec![])};
    let journal=JournalMem::default();
    let kernel=Kernel{model:&provider,tools:&writer,evidence:&NoFileChecks,journal:&journal};
    let result=kernel.run(request()).await.unwrap();
    assert!(matches!(result.status,Status::Completed),"{result:?}");
    assert_eq!(result.answer.as_deref(),Some("Completed source"));
    assert_eq!(*writer.called.lock().unwrap(),vec!["call_native_1"]);
    assert_eq!(*provider.calls.lock().unwrap(),2);
    assert!(journal.snapshots.lock().unwrap().iter().any(|s|
        s.in_flight.iter().any(|a|a.id=="call_native_1")));
}
#[tokio::test]
async fn provider_native_unauthorized_calls_are_rejected_before_host_effect() {
    let provider=NativeProvider{queue:Mutex::new(VecDeque::from(vec![
        Decision{plan:vec![],actions:vec![act("bad_call","secret_host_tool")],
            answer:None,candidate_paths:vec![]},
        Decision{plan:vec![],actions:vec![act("valid_call","workspace_write")],
            answer:None,candidate_paths:vec![]},
        Decision{plan:vec![],actions:vec![],answer:Some("Completed".into()),candidate_paths:vec![]},
    ])),calls:Mutex::new(0)};
    let writer=Writer{called:Mutex::new(vec![])};
    let journal=JournalMem::default();
    let kernel=Kernel{model:&provider,tools:&writer,evidence:&NoFileChecks,journal:&journal};
    let result=kernel.run(request()).await.unwrap();
    assert!(matches!(result.status,Status::Completed),"{result:?}");
    assert_eq!(*writer.called.lock().unwrap(),vec!["valid_call"]);
    assert!(result.checks.iter().all(|c|c.satisfied));
}

#[tokio::test]
async fn explicit_owner_cancel_during_generation_is_terminal_not_replayed() {
    use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
    struct CancelledModel {flag:Arc<AtomicBool>}
    impl Model for CancelledModel {
        fn complete<'a>(&'a self,_:&'a str,_:&'a str)
            ->BoxFuture<'a,Result<ModelReply,String>> {
            Box::pin(async move {
                self.flag.store(true,Ordering::SeqCst);
                Err("Native model stream stopped after owner cancellation".into())
            })
        }
    }
    struct CancelJournal {
        flag:Arc<AtomicBool>,
        saved:Mutex<Vec<Snapshot>>,
    }
    impl Journal for CancelJournal {
        fn save(&self,s:&Snapshot)->Result<(),String>{
            self.saved.lock().unwrap().push(s.clone());Ok(())
        }
        fn cancelled(&self,_:&str)->bool {
            self.flag.load(Ordering::SeqCst)
        }
    }
    let flag=Arc::new(AtomicBool::new(false));
    let model=CancelledModel{flag:flag.clone()};
    let journal=CancelJournal{flag,saved:Mutex::new(Vec::new())};
    let host=Writer{called:Mutex::new(Vec::new())};
    let kernel=Kernel{model:&model,tools:&host,evidence:&NoFileChecks,journal:&journal};
    let result=kernel.run(request()).await.unwrap();
    assert!(matches!(result.status,Status::Cancelled));
    assert!(!result.model_in_flight);
    assert!(host.called.lock().unwrap().is_empty());
    assert!(matches!(journal.saved.lock().unwrap().last().unwrap().status,
        Status::Cancelled));
}
