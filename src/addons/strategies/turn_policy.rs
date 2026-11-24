//! Product-owned strategy choices, plugged into the provider-neutral
//! native TurnStrategyPort. The core never sees algorithm IDs or their code.
use crate::{addons::{decompose,procedure,runtime_hooks},engine::policy,state::AppState};
use dominion_core::strategy::{TurnLimits,TurnStrategyPort};
use serde_json::{json,Value};

pub struct ScopedAlgorithms<'a> {
    state: &'a AppState,
    session_id: &'a str,
    agent_id: Option<&'a str>,
    execution_id: &'a str,
}
impl<'a> ScopedAlgorithms<'a> {
    pub fn new(
        state:&'a AppState,session_id:&'a str,agent_id:Option<&'a str>,
        execution_id:&'a str,
    )->Self{
        Self{state,session_id,agent_id,execution_id}
    }
    fn enabled(&self)->bool{
        runtime_hooks::strategies_enabled(self.state,self.session_id,self.agent_id)
    }
}

pub use dominion_algorithms::planning::needs_plan;

impl TurnStrategyPort for ScopedAlgorithms<'_> {
    fn execution_limits(&self,default_rounds:usize,default_tools:usize)->TurnLimits{
        let chosen=runtime_hooks::choose_for_agent(
            self.state,"execution.budget",self.agent_id,Some(self.execution_id));
        TurnLimits{
            max_rounds:chosen.as_ref()
                .and_then(|a|a["parameters"]["maxRounds"].as_u64())
                .unwrap_or(default_rounds as u64).clamp(2,24) as usize,
            max_tools:chosen.as_ref()
                .and_then(|a|a["parameters"]["maxTools"].as_u64())
                .unwrap_or(default_tools as u64).clamp(4,120) as usize,
        }
    }
    fn discovery_overridden(&self,reason:&str) {
        runtime_hooks::mark_unrated(self.state,self.execution_id,"tool.discovery",reason);
    }
    fn tool_discovery(&self,is_room:bool)->Value{
        if is_room {return Value::Null;}
        runtime_hooks::choose_for_agent(
            self.state,"tool.discovery",self.agent_id,Some(self.execution_id))
            .unwrap_or(Value::Null)
    }
    fn decompose(&self,input:&str,checks:&Value)->Value{
        if !self.enabled(){
            return json!({"kernel":"disabled","subtasks":[],"qualityScore":null});
        }
        let chosen=runtime_hooks::choose_decomposition_for_agent(
            self.state,self.agent_id,Some(self.execution_id),input)
            .unwrap_or(Value::Null);
        let result=decompose::decompose_with_acceptance(input,&chosen,checks);
        if result["qualityScore"].is_null(){
            runtime_hooks::mark_unrated(self.state,self.execution_id,
                "task.decomposition",
                "postcondition graph pending real native verification receipts");
        }
        result
    }
    fn planning_policy(&self)->Option<Value>{
        self.enabled().then(||policy::active(self.state,"planning")).flatten()
    }
    fn needs_plan(
        &self,policy:Option<&Value>,input:&str,
        acceptance:&Option<(String,Value)>,subtask_count:usize,
    )->bool{
        if !self.enabled(){return false;}
        policy.and_then(|p|policy::outline_decision(&p["parameters"],input.chars().count()))
            .unwrap_or_else(||needs_plan(input,acceptance)||subtask_count>1)
    }
    fn harness_plan(&self,candidates:&[Value],input:&str)->Vec<Value>{
        if !self.enabled(){return vec![];}
        candidates.iter()
            .filter(|s|s["assistanceMode"].as_str()==Some("harness_assisted")
                &&procedure::applies(s,input,None,None))
            .map(|s|json!({
                "strategyId":s["id"],"revision":s["revision"],
                "nodes":s["verification"]["nodes"].as_array().into_iter()
                    .flatten().filter_map(|n|n["id"].as_str()).collect::<Vec<_>>(),
            }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn planning_heuristic_keeps_legacy_semantics(){
        let checks=Some(("file".to_string(),json!([{"type":"file_exists","path":"x"}])));
        assert!(!needs_plan("report.txt 파일을 만들어줘",&checks));
        assert!(needs_plan("파일을 만들고 그리고 내용도 확인해",&checks));
        assert!(needs_plan("report.txt 파일을 만들어줘",&None));
        assert!(needs_plan(&"가".repeat(300),&checks));
    }
    #[test]
    fn disabled_algorithm_layer_is_an_actual_noop(){
        let root=std::env::temp_dir().join(format!(
            "dominion-strategy-port-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"native",
            "extensions":{"strategies":false}})).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"native",
            "status":"running"})).unwrap();
        let decision=ScopedAlgorithms::new(&state,"native",None,"run");
        assert_eq!(decision.execution_limits(8,40),TurnLimits{max_rounds:8,max_tools:40});
        assert!(decision.tool_discovery(false).is_null());
        assert!(decision.planning_policy().is_none());
        assert!(!decision.needs_plan(None,"복잡한 작업; 여러 단계",&None,3));
        let parsed=decision.decompose("hello and write a file",&Value::Null);
        assert_eq!(parsed["subtasks"],json!([]));
        assert_eq!(parsed["kernel"],"disabled");
        assert!(decision.harness_plan(&[],"hello").is_empty());
        assert!(state.store.all("algorithm_usage").is_empty());
        let _=std::fs::remove_dir_all(root);
    }
}
