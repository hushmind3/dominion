//! Dependency-aware, bounded tool dispatch for the Dominion-owned Rust kernel.
//!
//! The model proposes actions; only this scheduler determines execution order.
//! Model order is retained for side effects, and read-only batches are parallel
//! only when none of their members depends on another member of that batch.
use crate::{Action, Receipt, ToolSpec};
use std::collections::{HashMap, HashSet};

pub(crate) fn arrange(
    proposed: Vec<Action>,
    catalog: &HashMap<String, ToolSpec>,
    prior: &[Receipt],
    max_parallel_reads: usize,
) -> Result<Vec<Vec<Action>>, String> {
    if max_parallel_reads == 0 { return Err("Parallel-read limit must be positive".into()); }
    // All historical IDs stay reserved, including failed or skipped receipts.
    // Only a successful receipt can satisfy a new plan's prerequisite.
    let mut known: HashSet<String> = prior.iter().map(|receipt| receipt.id.clone()).collect();
    let mut available: HashSet<String> = prior.iter()
        .filter(|receipt| receipt.success).map(|receipt| receipt.id.clone()).collect();
    let mut pending: Vec<Action> = Vec::new();
    let mut groups: Vec<Vec<Action>> = Vec::new();

    for action in proposed {
        if action.id.trim().is_empty() || action.id.len() > 128 {
            return Err("Invalid tool operation id".into());
        }
        if !known.insert(action.id.clone()) {
            return Err(format!("Repeated tool operation id: {}", action.id));
        }
        let spec = catalog.get(&action.tool)
            .ok_or_else(|| format!("Tool not authorized: {}", action.tool))?;
        // The source order is a hard barrier for writes. A dependency on a
        // future action is rejected for a new plan rather than silently
        // reordering a potentially mutating command across another command.
        for dependency in &action.depends_on {
            if dependency == &action.id || !available.contains(dependency) {
                return Err(format!(
                    "Dependency {dependency} of {} must refer to an earlier action or receipt",
                    action.id
                ));
            }
        }
        // A proposed predecessor becomes available only after its dependencies
        // validate. Dispatch still checks the resulting receipt before use.
        available.insert(action.id.clone());
        let depends_on_pending = pending.iter().any(|previous| {
            action.depends_on.iter().any(|dependency| dependency == &previous.id)
        });
        if !pending.is_empty() && (!spec.read_only
            || pending.len() >= max_parallel_reads || depends_on_pending)
        {
            groups.push(std::mem::take(&mut pending));
        }
        if spec.read_only { pending.push(action); }
        else { groups.push(vec![action]); }
    }
    if !pending.is_empty() { groups.push(pending); }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read(name: &str) -> ToolSpec {
        ToolSpec{name:name.into(),description:String::new(),schema:serde_json::json!({}),
            read_only:true}
    }
    fn write(name: &str) -> ToolSpec { ToolSpec{read_only:false,..read(name)} }
    fn action(id:&str,tool:&str,depends_on:&[&str]) -> Action {
        Action{id:id.into(),tool:tool.into(),arguments:serde_json::json!({}),
            depends_on:depends_on.iter().map(|s|s.to_string()).collect()}
    }
    fn catalog() -> HashMap<String,ToolSpec> {
        [read("read"),write("write")].into_iter().map(|s|(s.name.clone(),s)).collect()
    }
    #[test]
    fn independent_reads_share_a_bounded_batch() {
        let groups=arrange(vec![
            action("a","read",&[]),action("b","read",&[]),
            action("c","read",&[]),action("d","read",&[])],
            &catalog(),&[],2).unwrap();
        assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(),vec![2,2]);
    }
    #[test]
    fn dependent_read_waits_for_its_prerequisite() {
        let groups=arrange(vec![action("a","read",&[]),action("b","read",&["a"]),
            action("c","read",&[])],&catalog(),&[],4).unwrap();
        assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(),vec![1,2]);
    }
    #[test]
    fn side_effects_are_serial_ordering_barriers() {
        let groups=arrange(vec![action("a","read",&[]),action("b","read",&[]),
            action("c","write",&["a"]),action("d","read",&[])],
            &catalog(),&[],4).unwrap();
        assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(),vec![2,1,1]);
        assert_eq!(groups[1][0].id,"c");
    }
    #[test]
    fn forward_or_unknown_dependencies_and_reused_ids_are_rejected() {
        assert!(arrange(vec![action("a","read",&["b"]),action("b","read",&[])],
            &catalog(),&[],2).is_err());
        assert!(arrange(vec![action("a","read",&[]),action("a","read",&[])],
            &catalog(),&[],2).is_err());
        let prior=vec![Receipt{id:"a".into(),tool:"read".into(),arguments:serde_json::json!({}),
            success:true,result:serde_json::json!({}),step:1}];
        assert!(arrange(vec![action("a","read",&[])],&catalog(),&prior,2).is_err());
        assert!(arrange(vec![action("b","read",&["a"])],&catalog(),&prior,2).is_ok());
    }
    #[test]
    fn unsuccessful_prior_receipts_cannot_authorize_dependent_actions() {
        for result in [serde_json::json!({"error":"permission denied"}),
            serde_json::json!({"skipped":true})] {
            let prior=vec![Receipt{id:"preflight".into(),tool:"read".into(),
                arguments:serde_json::json!({}),success:false,result,step:1}];
            assert!(arrange(vec![action("apply","write",&["preflight"])],
                &catalog(),&prior,2).is_err(),
                "failed and skipped receipts are not successful prerequisites");
            assert!(arrange(vec![action("preflight","read",&[])],
                &catalog(),&prior,2).is_err(),
                "failed receipts also reserve their operation IDs");
            assert!(arrange(vec![action("recheck","read",&[]),
                action("apply","write",&["recheck"])],
                &catalog(),&prior,2).is_ok(),
                "a new validated predecessor can be scheduled");
        }
    }
    #[test]
    fn no_undeclared_tool_and_no_unbounded_parallelism() {
        assert!(arrange(vec![action("a","shell_exec",&[])],&catalog(),&[],2).is_err());
        assert!(arrange(vec![action("a","read",&[])],&catalog(),&[],0).is_err());
    }
}
