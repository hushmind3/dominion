use super::tool::{Permission, ToolSpec};

/// What a policy may decide for one tool call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

/// The single place that decides whether a tool may run. Products replace
/// this without touching the loop.
pub trait Policy {
    fn decide(&self, spec: &ToolSpec) -> Decision;
}

/// Read-only tools run; writes, execution and network need approval.
pub struct ReadOnlyPolicy;

impl Policy for ReadOnlyPolicy {
    fn decide(&self, spec: &ToolSpec) -> Decision {
        match spec.permission {
            Permission::Read => Decision::Allow,
            _ => Decision::Ask,
        }
    }
}

/// Trusted local runs and tests.
pub struct AllowAll;

impl Policy for AllowAll {
    fn decide(&self, _spec: &ToolSpec) -> Decision {
        Decision::Allow
    }
}

/// Sealed evaluation: reads only, every other class denied.
pub struct DenyWrites;

impl Policy for DenyWrites {
    fn decide(&self, spec: &ToolSpec) -> Decision {
        match spec.permission {
            Permission::Read => Decision::Allow,
            _ => Decision::Deny,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolSpec;
    use serde_json::json;

    fn spec(permission: Permission) -> ToolSpec {
        ToolSpec {
            name: "t".into(),
            description: "d".into(),
            parameters: json!({}),
            permission,
        }
    }

    #[test]
    fn read_only_allows_reads_and_asks_for_writes() {
        assert_eq!(ReadOnlyPolicy.decide(&spec(Permission::Read)), Decision::Allow);
        assert_eq!(ReadOnlyPolicy.decide(&spec(Permission::Write)), Decision::Ask);
        assert_eq!(ReadOnlyPolicy.decide(&spec(Permission::Execute)), Decision::Ask);
    }

    #[test]
    fn deny_writes_refuses_every_mutating_class() {
        assert_eq!(DenyWrites.decide(&spec(Permission::Read)), Decision::Allow);
        for permission in [Permission::Write, Permission::Execute, Permission::Network, Permission::Dangerous] {
            assert_eq!(DenyWrites.decide(&spec(permission)), Decision::Deny);
        }
    }
}
