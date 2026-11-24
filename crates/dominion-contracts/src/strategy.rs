//! Narrow host-supplied planning and decision interface.
//!
//! The native runner owns model calls, tool approvals, effects, journals,
//! cancellation and verification. It knows no algorithm implementation name,
//! improvement store, decomposition kernel or model-specific prompt.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};

/// Runtime-enforced limits, not permission to exceed the caller's quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnLimits {
    pub max_rounds: usize,
    pub max_tools: usize,
}


#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct PlanningDecision {
    pub schema_version:u16,
    pub requires_plan:bool,
    pub subtask_count:usize,
}
impl PlanningDecision {
    pub const SCHEMA_VERSION:u16=1;
    pub fn new(requires_plan:bool,subtask_count:usize)->Self{
        Self{schema_version:Self::SCHEMA_VERSION,requires_plan,subtask_count}
    }
}


/// A versioned, bounded selection made by optional discovery algorithms.
/// The native tool gateway still enforces actual tool permissions.
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct DiscoveryDecision {
    pub schema_version:u16,
    pub is_room:bool,
    pub selection:Value,
}
impl DiscoveryDecision{
    pub fn new(is_room:bool,selection:Value)->Self{
        let selection=if selection.is_object(){selection}else{Value::Null};
        Self{schema_version:1,is_room,selection}
    }
}

/// The strategy may propose subtasks but cannot mark them verified.
/// Retain its original payload for existing algorithms and journal readers.
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct DecompositionDecision{
    pub schema_version:u16,
    pub kernel:String,
    pub subtasks:Vec<Value>,
    pub quality_score:Option<f64>,
    pub payload:Value,
}
impl DecompositionDecision{
    pub fn from_payload(mut payload:Value)->Self{
        if !payload.is_object(){payload=json!({});}
        let subtasks=payload["subtasks"].as_array().cloned().unwrap_or_default();
        let subtasks=subtasks.into_iter().filter(|v|v.is_object()).take(64)
            .collect::<Vec<_>>();
        payload["subtasks"]=json!(subtasks);
        Self{schema_version:1,
            kernel:payload["kernel"].as_str().unwrap_or("unknown").to_string(),
            quality_score:payload["qualityScore"].as_f64(),
            subtasks,payload
        }
    }
}

/// Procedures are suggestions only; every side effect still enters core.
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ProcedureDecision{
    pub schema_version:u16,
    pub strategies:Vec<Value>,
}
impl ProcedureDecision{
    pub fn from_candidates(strategies:Vec<Value>)->Self{
        Self{schema_version:1,strategies:strategies.into_iter().filter(|s|s.is_object())
            .take(32).collect()}
    }
}

/// Optional intelligence supplied by a product host. This contract is
/// provider-agnostic and does not depend on any product/add-on implementation.
pub trait TurnStrategyPort {
    fn execution_limits(&self, default_rounds: usize, default_tools: usize) -> TurnLimits;
    fn tool_discovery(&self, is_room: bool) -> Value;
    fn discovery_decision(&self,is_room:bool)->DiscoveryDecision{
        DiscoveryDecision::new(is_room,self.tool_discovery(is_room))
    }
    fn discovery_overridden(&self, reason: &str);
    fn decompose(&self, input: &str, checks: &Value) -> Value;
    fn decomposition_decision(&self,input:&str,checks:&Value)->DecompositionDecision{
        DecompositionDecision::from_payload(self.decompose(input,checks))
    }
    fn planning_policy(&self) -> Option<Value>;
    fn needs_plan(
        &self,
        policy: Option<&Value>,
        input: &str,
        acceptance: &Option<(String, Value)>,
        subtask_count: usize,
    ) -> bool;
    /// Typed, versioned decision. Older strategies keep the exact same
    /// behavior through the default adapter; the Harness never parses an
    /// opaque JSON flag to decide whether to plan.
    fn planning_decision(&self,policy:Option<&Value>,input:&str,
        acceptance:&Option<(String,Value)>,subtask_count:usize)->PlanningDecision{
        PlanningDecision::new(self.needs_plan(policy,input,acceptance,subtask_count),subtask_count)
    }
    fn harness_plan(&self, candidates: &[Value], input: &str) -> Vec<Value>;
    fn procedure_decision(&self,candidates:&[Value],input:&str)->ProcedureDecision{
        ProcedureDecision::from_candidates(self.harness_plan(candidates,input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The execution core can operate against a no-intelligence host without
    // importing memory, planning strategies or any application module.
    struct NoStrategy;
    impl TurnStrategyPort for NoStrategy {
        fn execution_limits(&self, rounds: usize, tools: usize) -> TurnLimits {
            TurnLimits { max_rounds: rounds, max_tools: tools }
        }
        fn tool_discovery(&self, _: bool) -> Value { Value::Null }
        fn discovery_overridden(&self, _: &str) {}
        fn decompose(&self, _: &str, _: &Value) -> Value { Value::Null }
        fn planning_policy(&self) -> Option<Value> { None }
        fn needs_plan(&self, _: Option<&Value>, _: &str, _: &Option<(String, Value)>, _: usize) -> bool {
            false
        }
        fn harness_plan(&self, _: &[Value], _: &str) -> Vec<Value> { vec![] }
    }
    #[test]
    fn native_runtime_accepts_a_noop_strategy_port() {
        let port: &dyn TurnStrategyPort = &NoStrategy;
        assert_eq!(port.execution_limits(8,40),TurnLimits{max_rounds:8,max_tools:40});
        assert!(port.tool_discovery(false).is_null());
        assert!(port.harness_plan(&[],"hello").is_empty());
        let plan=port.planning_decision(None,"hello",&None,2);
        assert_eq!(plan,PlanningDecision::new(false,2));
        let value=serde_json::to_value(plan).unwrap();
        assert_eq!(value["schemaVersion"],1);
        assert_eq!(value["subtaskCount"],2);
        let discovery=port.discovery_decision(false);
        assert_eq!(discovery.schema_version,1);
        assert!(discovery.selection.is_null());
        let decomposition=port.decomposition_decision("hello",&Value::Null);
        assert_eq!(decomposition.schema_version,1);
        assert_eq!(decomposition.kernel,"unknown");
        assert!(decomposition.subtasks.is_empty());
        let procedure=port.procedure_decision(&[],"hello");
        assert_eq!(procedure.schema_version,1);
        assert!(procedure.strategies.is_empty());
        let bounded=DecompositionDecision::from_payload(json!({
            "kernel":"frontier-v1","subtasks":(0..100).map(|i|json!({"id":i}))
                .collect::<Vec<_>>(),"qualityScore":0.8
        }));
        assert_eq!(bounded.subtasks.len(),64);
        assert_eq!(bounded.payload["subtasks"].as_array().unwrap().len(),64);
        assert_eq!(bounded.quality_score,Some(0.8));
    }
}
