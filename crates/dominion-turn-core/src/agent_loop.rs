//! Contracts for swappable agent algorithms. Runtime admission remains in
//! Driver: plugins can suggest a transition but cannot invent tool authority.
use crate::driver::{Driver,Step};
use std::future::Future;
use serde_json::{json, Value};

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub struct RoundObservation {
    pub proposed_calls:usize,
    pub has_plan:bool,
}
/// Agents may change loop/plan policy; the core alone decides what executes.
pub trait AgentLoop:Send {
    fn name(&self)->&'static str;
    fn after_model(&mut self,observation:RoundObservation)->Step;
}
/// The provider transcript representation of a tool observation. The host
/// may persist this exact content; the core owns only its protocol shape, not
/// the truth of a tool effect or the authority to perform it.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ToolFeedback {
    pub call_id:String,
    pub content:String,
}
impl ToolFeedback {
    pub fn from_content(call_id:&str,content:impl Into<String>)->Self{
        Self{call_id:call_id.into(),content:content.into()}
    }
    pub fn from_value(call_id:&str,result:&Value)->Self{
        Self::from_content(call_id,result.to_string())
    }
    pub fn not_dispatched(call_id:&str,reason:&str)->Self{
        Self::from_value(call_id,&json!({"status":"not_dispatched",
            "dispatched":false,"reason":reason}))
    }
    /// Transport ambiguity is NOT a confirmed failure or a completed effect.
    /// The host's durable effect ledger is responsible for reconciliation.
    pub fn outcome_unknown(call_id:&str,error:&str)->Self{
        Self::from_value(call_id,&json!({"status":"outcome_unknown",
            "outcomeUnknown":true,"error":error}))
    }
    pub fn provider_message(&self)->Value{
        json!({"role":"tool","tool_call_id":self.call_id,"content":self.content})
    }
}

#[derive(Debug)]
pub struct PortRunResult{
    pub response:String,
    pub model_rounds:usize,
    pub tool_calls:usize,
    pub workflow:&'static str,
}
#[derive(Clone,Debug,PartialEq)]
pub struct ToolRequest {
    pub id:String,
    pub name:String,
    pub arguments:Value,
}
#[derive(Clone,Debug)]
pub struct ModelResponse {
    pub content:String,
    pub calls:Vec<ToolRequest>,
}
/// One immutable admission ticket for an actual provider response. Product
/// Chat persists the assistant tool-call envelope BEFORE batch admission,
/// while a standalone SDK can admit immediately. Neither host may alter the
/// IDs/tool names between the decision and effect boundary.
#[derive(Debug,Clone)]
pub struct PreparedResponse{
    step:Step,
    calls:Vec<(String,String)>,
}
impl PreparedResponse{
    pub fn step(&self)->Step{self.step}
    pub fn call_count(&self)->usize{self.calls.len()}
}

/// The single provider-independent round coordinator shared by both the
/// product Chat runtime and the standalone port-driven AgentLoop. Strategy
/// decisions cannot bypass the driver; no product, model, tool or I/O code
/// belongs here. Host-specific streaming and tool scheduling stay adapters.
pub struct Harness<'a,A:AgentLoop+?Sized>{
    driver:&'a mut Driver,
    algorithm:&'a mut A,
}
impl<'a,A:AgentLoop+?Sized> Harness<'a,A>{
    pub fn new(driver:&'a mut Driver,algorithm:&'a mut A)->Self{
        Self{driver,algorithm}
    }
    pub fn workflow(&self)->&'static str{self.algorithm.name()}

    /// All hosts use the same core scheduling decision for starting the next
    /// model turn. Streaming and durable effects stay host callbacks; neither
    /// Chat nor the SDK owns a separate provider-round limit anymore.
    pub fn next_round(&self,finished:bool,cancelled:bool)
        -> Result<Option<usize>, &'static str>{
        self.driver.next_model_round(finished,cancelled)
    }

    /// Decide a whole model response, with exactly the provider's real tool
    /// IDs. No tool authority is reserved yet: Chat may first commit the
    /// assistant proposal and then persist precise rejected-call feedback.
    pub fn prepare_response<'b>(
        &mut self,calls:impl IntoIterator<Item=(&'b str,&'b str)>,
        has_plan:bool,enabled:bool,
    )->Result<PreparedResponse,&'static str>{
        let calls=calls.into_iter().map(|(id,name)|
            (id.to_string(),name.to_string())).collect::<Vec<_>>();
        let step=self.authorize_step(
            RoundObservation{proposed_calls:calls.len(),has_plan},enabled)?;
        if matches!(step,Step::Execute{..}){
            Driver::validate_call_ids(calls.iter().map(|(id,_)|id.as_str()))?;
        }
        Ok(PreparedResponse{step,calls})
    }
    /// The only post-proposal batch admission path. Rechecks both the
    /// currently advertised tools and turn-local call IDs atomically.
    pub fn admit_prepared(
        &mut self,prepared:&PreparedResponse,visible:&[Value],enabled:bool,
    )->Result<(),&'static str>{
        match prepared.step{
            Step::Answer=>Ok(()),
            Step::Execute{..}=>self.admit_batch(
                prepared.calls.iter().map(|(id,name)|
                    (id.as_str(),name.as_str())),visible,enabled),
        }
    }
    /// Return the driver's future directly; no extra async state machine.
    /// Native streaming completions are large futures and nested wrappers
    /// can overflow the small macOS test-thread stack on room replays.
    pub fn request_model<'b,T:'b,F>(
        &'b mut self,cancelled:bool,request:F,
    )->impl Future<Output=Result<T,String>> + 'b
    where F:Future<Output=Result<T,String>> + 'b{
        self.driver.request_model(cancelled,request)
    }
    pub fn authorize_step(
        &mut self,observation:RoundObservation,tools_enabled:bool,
    )->Result<Step,&'static str>{
        self.driver.authorize_step(self.algorithm,observation,tools_enabled)
    }
    pub fn admit_batch<'b>(
        &mut self,calls:impl IntoIterator<Item=(&'b str,&'b str)>,
        visible_tools:&[Value],enabled:bool,
    )->Result<(),&'static str>{
        self.driver.admit_batch(calls,visible_tools,enabled)
    }
    /// Core-owned port execution: no parallel product-side model/tool loop
    /// in the standalone strategy crate. Host owns actual approvals/effects.
    pub async fn drive_ports<M:ModelPort,T:ToolPort>(
        &mut self,model:&mut M,tools:&mut T,
        history:&mut Vec<Value>,visible_tools:&[Value],
        tools_enabled:bool,has_plan:bool,
    )->Result<PortRunResult,String>{
    let mut rounds=0usize;
    let mut dispatched=0usize;
    while self.next_round(false,model.cancelled()||tools.cancelled())
        .map_err(str::to_string)?.is_some() {
        let response:ModelResponse=self.request_model(
            model.cancelled()||tools.cancelled(),
            model.complete(history,visible_tools),
        ).await?;
        rounds+=1;
        let prepared=self.prepare_response(
            response.calls.iter().map(|call|(call.id.as_str(),call.name.as_str())),
            has_plan,tools_enabled,
        ).map_err(str::to_string)?;
        match prepared.step(){
            Step::Answer=>{
                history.push(json!({"role":"assistant","content":response.content}));
                if let Some(reason)=model.verify_candidate(&response.content).await?{
                    history.push(json!({"role":"user","content":format!("Independent verification rejected the answer. Repair the task or answer using this feedback: {reason}")}));
                    continue;
                }
                return Ok(PortRunResult{
                    response:response.content,model_rounds:rounds,
                    tool_calls:dispatched,workflow:self.workflow(),
                });
            }
            Step::Execute{..}=>{
                // Reserve the full batch before any external effect. A
                // hidden tool, duplicate call ID or exhausted budget denies
                // every call in the proposal, not just the later ones.
                self.admit_prepared(&prepared,visible_tools,tools_enabled)
                    .map_err(str::to_string)?;
                let calls=response.calls.iter().map(|c|json!({
                    "id":c.id,"type":"function",
                    "function":{"name":c.name,"arguments":c.arguments.to_string()}
                })).collect::<Vec<_>>();
                history.push(json!({"role":"assistant","content":response.content,
                    "tool_calls":calls}));
                for (index,call) in response.calls.iter().enumerate() {
                    if model.cancelled()||tools.cancelled(){
                        // Complete transcript pairing without claiming that any
                        // remaining call reached the host.
                        for skipped in &response.calls[index..] {
                            history.push(ToolFeedback::not_dispatched(&skipped.id,
                                "execution cancelled").provider_message());
                        }
                        return Err("execution cancelled".into());
                    }
                    match tools.dispatch(call).await {
                        Ok(output)=>{
                            history.push(ToolFeedback::from_value(&call.id,&output)
                                .provider_message());
                            dispatched+=1;
                        }
                        Err(error)=>{
                            // A transport error cannot establish whether an
                            // external effect happened: never auto-replay it.
                            history.push(ToolFeedback::outcome_unknown(&call.id,&error)
                                .provider_message());
                            for skipped in &response.calls[index+1..] {
                                history.push(ToolFeedback::not_dispatched(&skipped.id,
                                    "prior tool failure").provider_message());
                            }
                            return Err(error);
                        }
                    }
                }
            }
        }
    }
    Err("model round limit exceeded".into())
    }
}

/// Implemented in the gateway (actual provider, streaming and auth live there).
pub trait ModelPort {
    async fn complete(&mut self, transcript:&[Value], visible_tools:&[Value])
        ->Result<ModelResponse,String>;
    /// Optional final-answer verification gate; ordinary raw ports retain
    /// their existing behavior and do not claim independent verification.
    async fn verify_candidate(&mut self,_answer:&str)->Result<Option<String>,String>{Ok(None)}
    fn cancelled(&self)->bool {false}
}
/// Approval, sandboxing, effect recording and actual tool execution live here.
pub trait ToolPort {
    async fn dispatch(&mut self, call:&ToolRequest)->Result<Value,String>;
    fn cancelled(&self)->bool {false}
}
impl Driver {
    /// Fail closed if a pluggable algorithm attempts to rewrite the model's
    /// actual tool count or execute tools prohibited by the owning user.
    pub fn authorize_step<A:AgentLoop+?Sized>(&self, algorithm:&mut A,
        observation:RoundObservation,tools_enabled:bool)->Result<Step,&'static str>{
        let permitted=self.select_step(observation.proposed_calls,tools_enabled)?;
        if algorithm.after_model(observation)!=permitted {
            return Err("agent loop attempted to alter model tool-call authority");
        }
        Ok(permitted)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    struct EscalatingLoop;
    impl AgentLoop for EscalatingLoop{
        fn name(&self)->&'static str {"bad"}
        fn after_model(&mut self,_:RoundObservation)->Step{Step::Execute{calls:10}}
    }
    #[test]
    fn algorithm_cannot_invent_tool_calls_or_reenable_disabled_tooling(){
        let driver=Driver::new(3,2);
        let mut bad=EscalatingLoop;
        assert_eq!(driver.authorize_step(&mut bad,RoundObservation{
            proposed_calls:0,has_plan:false},true),
            Err("agent loop attempted to alter model tool-call authority"));
        assert_eq!(driver.authorize_step(&mut bad,RoundObservation{
            proposed_calls:1,has_plan:true},false),
            Err("tool execution disabled"));
    }
    #[test]
    fn harness_guards_strategy_and_atomically_admits_each_provider_batch(){
        struct Safe;
        impl AgentLoop for Safe{
            fn name(&self)->&'static str{"safe"}
            fn after_model(&mut self,observation:RoundObservation)->Step{
                if observation.proposed_calls==0{Step::Answer}
                else{Step::Execute{calls:observation.proposed_calls}}
            }
        }
        let mut driver=Driver::new(2,2);
        let mut policy=Safe;
        let mut harness=Harness::new(&mut driver,&mut policy);
        let observed=RoundObservation{proposed_calls:2,has_plan:false};
        assert_eq!(harness.workflow(),"safe");
        assert_eq!(harness.authorize_step(observed,true),
            Ok(Step::Execute{calls:2}));
        let visible=vec![json!({"name":"read"})];
        assert_eq!(harness.admit_batch(
            [("a","read"),("b","hidden")],&visible,true),
            Err("tool was not advertised"));
        assert!(harness.admit_batch(
            [("a","read"),("b","read")],&visible,true).is_ok());
        assert_eq!(harness.admit_batch(
            [("a","read")],&visible,true),
            Err("duplicate or missing tool call id"));
    }

    #[test]
    fn staged_response_admission_preserves_host_persistence_and_rejects_bad_siblings(){
        struct Policy;
        impl AgentLoop for Policy{
            fn name(&self)->&'static str{"react"}
            fn after_model(&mut self,observation:RoundObservation)->Step{
                if observation.proposed_calls==0{Step::Answer}
                else{Step::Execute{calls:observation.proposed_calls}}
            }
        }
        let mut driver=Driver::new(4,3);
        let mut policy=Policy;
        let mut harness=Harness::new(&mut driver,&mut policy);
        let visible=vec![json!({"name":"read"})];
        let bad=harness.prepare_response([
            ("r1","read"),("r2","hidden")],false,true).unwrap();
        assert_eq!(bad.step(),Step::Execute{calls:2});
        assert_eq!(harness.admit_prepared(&bad,&visible,true),
            Err("tool was not advertised"));
        let good=harness.prepare_response([
            ("r1","read"),("r2","read")],false,true).unwrap();
        assert!(harness.admit_prepared(&good,&visible,true).is_ok());
        assert_eq!(harness.admit_prepared(&good,&visible,true),
            Err("duplicate or missing tool call id"));
        assert_eq!(harness.prepare_response([
            ("duplicate","read"),("duplicate","read")],false,true)
            .unwrap_err(),"duplicate or missing tool call id");
        let answer=harness.prepare_response(std::iter::empty(),false,true).unwrap();
        assert_eq!(answer.step(),Step::Answer);
        assert_eq!(answer.call_count(),0);
        assert!(harness.admit_prepared(&answer,&visible,true).is_ok());
    }

    #[test]
    fn host_round_gate_reuses_exact_driver_limits_and_stop_rules(){
        struct Safe;
        impl AgentLoop for Safe{
            fn name(&self)->&'static str{"safe"}
            fn after_model(&mut self,observation:RoundObservation)->Step{
                if observation.proposed_calls==0{Step::Answer}
                else{Step::Execute{calls:observation.proposed_calls}}
            }
        }
        let mut driver=Driver::new(1,2);
        let mut policy=Safe;
        let mut harness=Harness::new(&mut driver,&mut policy);
        assert_eq!(harness.next_round(false,false),Ok(Some(0)));
        assert_eq!(harness.next_round(true,false),Ok(None));
        assert_eq!(harness.next_round(false,true),Err("execution cancelled"));
        let result=std::future::ready(Ok::<_,String>("mock-model"));
        // Model invocation itself consumes a round, not the scheduling probe.
        let _=tokio::runtime::Builder::new_current_thread().enable_all()
            .build().unwrap().block_on(harness.request_model(false,result)).unwrap();
        assert_eq!(harness.next_round(false,false),Ok(None));
    }

    #[test]
    fn shared_tool_feedback_preserves_host_content_and_marks_unexecuted_effects(){
        let actual=json!({"content":"tool output","success":true});
        let observed=ToolFeedback::from_value("call-1",&actual);
        assert_eq!(observed.provider_message()["content"],actual.to_string());
        assert_eq!(observed.provider_message()["tool_call_id"],"call-1");
        let skipped=ToolFeedback::not_dispatched("call-2","cancelled");
        let payload:Value=serde_json::from_str(&skipped.content).unwrap();
        assert_eq!(payload["status"],"not_dispatched");
        assert_eq!(payload["dispatched"],false);
        let unknown=ToolFeedback::outcome_unknown("call-3","timeout");
        let payload:Value=serde_json::from_str(&unknown.content).unwrap();
        assert_eq!(payload["outcomeUnknown"],true);
        assert_ne!(payload["status"],"completed");
    }

}
