//! The only product adapter that resolves versioned strategies for chat turns.
//! It supplies typed decisions to the runtime; the pure algorithm library has
//! no knowledge of AppState, providers, sessions, MCP, HTTP or credentials.
use crate::state::AppState;
use dominion_agent_kernel::decision_policy::{
    self,RetryChoice,ToolCombination,ToolOrder,WriteChoice,
};
use serde_json::Value;

#[derive(Clone,Copy)]
pub struct TurnPolicy<'a>{
    state:&'a AppState,
    agent_id:Option<&'a str>,
    execution_id:&'a str,
}
impl<'a> TurnPolicy<'a>{
    pub fn new(state:&'a AppState,agent_id:Option<&'a str>,execution_id:&'a str)->Self{
        Self{state,agent_id,execution_id}
    }
    fn choose(&self,kind:&str,receipt:bool)->Option<Value>{
        crate::engine::algorithms::choose_for_agent(
            self.state,kind,self.agent_id,receipt.then_some(self.execution_id),
        )
    }
    pub fn budget(&self,rounds:usize,tools:usize)->(usize,usize){
        let choice=self.choose("execution.budget",true);
        let rounds=choice.as_ref().and_then(|v|v["parameters"]["maxRounds"].as_u64())
            .unwrap_or(rounds as u64).clamp(2,24) as usize;
        let tools=choice.as_ref().and_then(|v|v["parameters"]["maxTools"].as_u64())
            .unwrap_or(tools as u64).clamp(4,120) as usize;
        (rounds,tools)
    }
    pub fn discovery(&self,is_room:bool)->Value{
        if is_room {Value::Null}
        else {self.choose("tool.discovery",true).unwrap_or(Value::Null)}
    }
    pub fn decomposition(&self,input:&str)->Value{
        crate::engine::algorithms::choose_decomposition_for_agent(
            self.state,self.agent_id,Some(self.execution_id),input,
        ).unwrap_or(Value::Null)
    }
    pub fn decompose_request(&self,input:&str)->Value{
        let selected=self.decomposition(input);
        crate::engine::decompose::decompose(input,&selected)
    }
    pub fn plan_heuristic(input:&str,checks:&Option<(String,Value)>)->bool{
        decision_policy::needs_plan(input,checks)
    }
    pub fn skip_extra_planner(input:&str,checks:&Option<(String,Value)>)->bool{
        decision_policy::local_multifile_skips_extra_planner(input,checks)
    }
    pub fn plan_scores(plan:&str,input:&str)->(Value,Value){
        (crate::engine::plan_quality::score(plan,input),
         crate::engine::plan_quality::score_independent(plan,input))
    }
    pub fn workflow(&self,plan_requested:bool)->dominion_agent_loop::ActiveLoop{
        dominion_agent_loop::select(plan_requested)
    }
    pub fn order(&self)->ToolOrder{
        let choice=self.choose("execution.ordering",true);
        ToolOrder::from_implementation(choice.as_ref()
            .and_then(|v|v["implementation"].as_str()))
    }
    pub fn combination(&self)->ToolCombination{
        let choice=self.choose("tool.combination",false);
        ToolCombination::from_implementation(choice.as_ref()
            .and_then(|v|v["implementation"].as_str()))
    }
    pub fn note_combination(&self){
        let _=self.choose("tool.combination",true);
    }
    pub fn write_choice(&self)->WriteChoice{
        let choice=self.choose("tool.selection",true);
        WriteChoice::from_implementation(choice.as_ref()
            .and_then(|v|v["implementation"].as_str()))
    }
    pub fn retry_choice(&self)->RetryChoice{
        let choice=self.choose("recovery.strategy",true);
        RetryChoice::from_implementation(choice.as_ref()
            .and_then(|v|v["implementation"].as_str()))
    }
    pub fn signal(&self,kind:&str,ok:bool){
        crate::engine::algorithms::mark_signal(self.state,self.execution_id,kind,ok);
    }
    pub fn unrated(&self,kind:&str,reason:&str){
        crate::engine::algorithms::mark_unrated(self.state,self.execution_id,kind,reason);
    }
    pub fn active(&self,slot:&str)->Option<Value>{
        crate::engine::policy::active(self.state,slot)
    }
    pub fn file_guard(&self)->Option<Value>{
        crate::engine::policy::portable_file_guard(self.state)
            .or_else(||self.active("mutation.preflight"))
    }
    pub fn recovery_limits(policy:Option<&Value>)->(usize,usize){
        policy.map(|v|crate::engine::policy::recovery_limits(&v["parameters"]))
            .unwrap_or((2,2))
    }
    pub fn plan_requested(policy:Option<&Value>,input_len:usize,
        heuristic:bool,previous_panel_answer:bool,native_contract_plan:bool,
        mode:Option<&str>)->bool{
        let explicit=policy.and_then(|v|
            crate::engine::policy::outline_decision(&v["parameters"],input_len));
        decision_policy::admit_plan(explicit,heuristic,previous_panel_answer,
            native_contract_plan,mode)
    }
}
