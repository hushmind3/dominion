//! Narrow host-supplied planning and decision interface.
//!
//! The native runner owns model calls, tool approvals, effects, journals,
//! cancellation and verification. It knows no algorithm implementation name,
//! improvement store, decomposition kernel or model-specific prompt.
use serde::{Deserialize,Serialize};
use serde_json::Value;

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

/// Optional intelligence supplied by a product host. This contract is
/// provider-agnostic and does not depend on any product/add-on implementation.
pub trait TurnStrategyPort {
    fn execution_limits(&self, default_rounds: usize, default_tools: usize) -> TurnLimits;
    fn tool_discovery(&self, is_room: bool) -> Value;
    fn discovery_overridden(&self, reason: &str);
    fn decompose(&self, input: &str, checks: &Value) -> Value;
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
    }
}
