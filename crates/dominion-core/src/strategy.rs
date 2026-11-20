//! Narrow host-supplied planning and decision interface.
//!
//! The native runner owns model calls, tool approvals, effects, journals,
//! cancellation and verification. It knows no algorithm implementation name,
//! improvement store, decomposition kernel or model-specific prompt.
use serde_json::Value;

/// Runtime-enforced limits, not permission to exceed the caller's quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnLimits {
    pub max_rounds: usize,
    pub max_tools: usize,
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
    }
}
