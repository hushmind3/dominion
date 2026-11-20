//! Contracts for swappable agent algorithms. Runtime admission remains in
//! Driver: plugins can suggest a transition but cannot invent tool authority.
use crate::driver::{Driver,Step};
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
/// Implemented in the gateway (actual provider, streaming and auth live there).
pub trait ModelPort {
    async fn complete(&mut self, transcript:&[Value], visible_tools:&[Value])
        ->Result<ModelResponse,String>;
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
