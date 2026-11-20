//! Contracts for swappable agent algorithms. Runtime admission remains in
//! Driver: plugins can suggest a transition but cannot invent tool authority.
use crate::driver::{Driver,Step};
use serde_json::Value;

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
}
