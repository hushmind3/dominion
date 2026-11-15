//! Deterministic acceptance of exact owner-required shell checks.
//! Tool schemas and the authority to execute are provided by the Rust host,
//! never by the model. A successful check is invalidated by later side effects.
use crate::{Criterion, CriterionKind, Receipt, ToolSpec};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandState {
    /// The newest matching attempt succeeded after every intervening effect.
    Passed,
    /// No eligible attempt exists for the current workspace revision.
    Pending,
    /// The newest matching attempt failed; require a repair or explicit retry.
    Failed,
}

pub(crate) fn required_command(criterion: &Criterion) -> Option<&str> {
    (criterion.kind == CriterionKind::ToolSucceeded
        && criterion.tool.as_deref() == Some("shell_exec"))
        .then_some(criterion.contains.as_deref()).flatten()
        .filter(|command| !command.trim().is_empty())
}

/// A shell command that is itself an owner-required acceptance check is a
/// verifier, not an edit barrier. Every other non-read-only tool call may have
/// changed external state, even if it returned an error or a partial result.
fn is_effect(receipt: &Receipt, checks: &[Criterion],
    catalog: &HashMap<String, ToolSpec>) -> bool {
    if receipt.tool == "shell_exec" {
        if let Some(command) = receipt.arguments["command"].as_str() {
            if checks.iter().any(|check| required_command(check) == Some(command)) {
                return false;
            }
        }
    }
    !catalog.get(&receipt.tool).is_some_and(|spec| spec.read_only)
}

pub(crate) fn command_state(receipts: &[Receipt], checks: &[Criterion],
    catalog: &HashMap<String, ToolSpec>, command: &str) -> CommandState {
    let latest_effect = receipts.iter().rposition(|r| is_effect(r, checks, catalog));
    let latest_attempt = receipts.iter().enumerate().rev().find(|(_, r)| {
        r.tool == "shell_exec"
            && r.arguments["command"].as_str() == Some(command)
    });
    match latest_attempt {
        Some((index, receipt)) if latest_effect.is_none_or(|effect| index > effect) =>
            if receipt.success { CommandState::Passed } else { CommandState::Failed },
        _ => CommandState::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog() -> HashMap<String, ToolSpec> {
        ["read", "write", "shell_exec", "mcp__external"].into_iter()
            .map(|name| (name.into(),ToolSpec { name:name.into(),
                description:String::new(),schema:json!({}),read_only:name=="read" }))
            .collect()
    }
    fn check(command:&str) -> Criterion {
        Criterion {kind:CriterionKind::ToolSucceeded, path:None,
            contains:Some(command.into()),tool:Some("shell_exec".into())}
    }
    fn receipt(id:&str, tool:&str, command:&str, success:bool) -> Receipt {
        Receipt {id:id.into(),tool:tool.into(),arguments:json!({"command":command}),
            success,result:json!({}),step:1}
    }
    #[test]
    fn only_the_exact_command_is_accepted() {
        let checks=vec![check("cargo test")];
        let receipts=vec![receipt("t","shell_exec","cargo check",true)];
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo test"),CommandState::Pending);
    }
    #[test]
    fn later_edit_even_failed_invalidates_a_pass() {
        let checks=vec![check("cargo test")];
        let receipts=vec![receipt("pass","shell_exec","cargo test",true),
            receipt("possibly_partial","write","",false)];
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo test"),CommandState::Pending);
    }
    #[test]
    fn last_failed_test_invalidates_earlier_success() {
        let checks=vec![check("cargo test")];
        let receipts=vec![receipt("first","shell_exec","cargo test",true),
            receipt("last","shell_exec","cargo test",false)];
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo test"),CommandState::Failed);
    }
    #[test]
    fn multiple_required_tests_do_not_invalidate_each_other() {
        let checks=vec![check("cargo test"),check("cargo clippy")];
        let receipts=vec![receipt("t","shell_exec","cargo test",true),
            receipt("c","shell_exec","cargo clippy",true)];
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo test"),CommandState::Passed);
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo clippy"),CommandState::Passed);
    }
    #[test]
    fn unrelated_shell_and_unknown_tools_invalidate_tests() {
        let checks=vec![check("cargo test")];
        let receipts=vec![receipt("pass","shell_exec","cargo test",true),
            receipt("format","shell_exec","cargo fmt",true)];
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo test"),CommandState::Pending);
        let receipts=vec![receipt("pass","shell_exec","cargo test",true),
            receipt("external","unknown_tool","",false)];
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo test"),CommandState::Pending);
    }
    #[test]
    fn intervening_read_does_not_invalidate_the_pass() {
        let checks=vec![check("cargo test")];
        let receipts=vec![receipt("pass","shell_exec","cargo test",true),
            receipt("read","read","",true)];
        assert_eq!(command_state(&receipts,&checks,&catalog(),"cargo test"),CommandState::Passed);
    }
}
