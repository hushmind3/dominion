//! Model-independent tool admission and batch ordering for native Rust SDK clients.
//!
//! No provider, DB, chat or async runtime dependencies. The host owns tool
//! execution, budget charging, approvals, cancellation, and durable receipts.
//! Unknown tools, writes, malformed arguments and opaque MCP methods are
//! always exclusive. No read is moved ahead of a preceding write.

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum ToolBatch {
    ParallelRead { start:usize,len:usize },
    Exclusive { index:usize },
}

/// The only shell observations proven inert without interpreting shell syntax.
/// Callers MUST check this exact predicate at both planning and runtime lock
/// admission. A near match with substitutions, pipes or flags stays exclusive.
pub fn inert_shell_command(args:&serde_json::Value)->bool{
    let Some(command)=args["command"].as_str().map(str::trim) else{return false};
    let mut words=command.split_ascii_whitespace();
    match words.next(){
        Some("sleep")=>{
            let value=words.next().and_then(|text|text.parse::<f64>().ok());
            words.next().is_none() && value.is_some_and(|seconds|
                seconds.is_finite() && (0.0..=60.0).contains(&seconds))
        },
        Some("pwd"|"whoami"|"uptime"|"date"|"uname"|"id")=>
            words.next().is_none(),
        _=>false,
    }
}

/// Conservative allowlist: a tool name is eligible for overlapping reads
/// only when the DOMINION host implements it as a native observation.
pub fn safe_to_overlap(name:&str)->bool{
    matches!(name,
        "workspace_list"|"workspace_read"|"workspace_read_range"|
        "file_list"|"file_read"|"web_search"|"http_fetch"|"memory_search"|
        "task_status"|"task_result"|"account_status"|"github_file_read")
}

/// Read only the contiguous safe prefix *and no more than the actual pool
/// capacity*. Calling code must apply its normal approval/cancellation and
/// budget gates before dispatch, independently of this classification.
pub fn parallel_prefix<'a>(
    calls:impl Iterator<Item=(&'a str,&'a str)>,capacity:usize
)->usize{
    if capacity<=1{return 0;}
    calls.take(capacity.min(16)).take_while(|(name,arguments)|{
        safe_to_overlap(name)
            && serde_json::from_str::<serde_json::Value>(arguments)
                .is_ok_and(|value|value.is_object())
    }).count()
}

/// Partition model-order calls. Mutating or unknown calls are hard exclusive
/// barriers. The final group can contain fewer than max_parallel items.
pub fn plan(names:&[&str],max_parallel:usize)->Vec<ToolBatch>{
    let cap=max_parallel.clamp(1,16);
    let mut groups=Vec::new();
    let mut index=0;
    while index<names.len(){
        if safe_to_overlap(names[index]) && cap>1{
            let run=names[index..].iter().take(cap)
                .take_while(|name|safe_to_overlap(name)).count();
            if run>=2{
                groups.push(ToolBatch::ParallelRead{start:index,len:run});
                index+=run;
                continue;
            }
        }
        groups.push(ToolBatch::Exclusive{index});
        index+=1;
    }
    groups
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn the_batch_admission_limit_is_enforced_before_any_tool_starts(){
        let calls=(0..40).map(|_|("workspace_read",r#"{"path":"src/lib.rs"}"#));
        assert_eq!(parallel_prefix(calls,4),4);
        let calls=(0..40).map(|_|("workspace_read",r#"{"path":"src/lib.rs"}"#));
        assert_eq!(parallel_prefix(calls,16),16);
        let calls=(0..40).map(|_|("workspace_read",r#"{"path":"src/lib.rs"}"#));
        assert_eq!(parallel_prefix(calls,100),16);
        let calls=(0..40).map(|_|("workspace_read",r#"{"path":"src/lib.rs"}"#));
        assert_eq!(parallel_prefix(calls,1),0);
    }
    #[test]
    fn invalid_arguments_or_unknown_tools_never_join_parallel_reads(){
        assert_eq!(parallel_prefix([
            ("workspace_read",r#"{"path":"a"}"#),
            ("workspace_read","[]"),
            ("workspace_read",r#"{"path":"b"}"#)
        ].into_iter(),4),1);
        assert_eq!(parallel_prefix([
            ("mcp__external__list",r#"{}"#),
            ("workspace_read",r#"{"path":"a"}"#)
        ].into_iter(),4),0);
        assert_eq!(parallel_prefix([
            ("workspace_read",r#"{"path":"a"}"#),
            ("shell_exec",r#"{"command":"rm -rf /"}"#),
            ("workspace_read",r#"{"path":"b"}"#)
        ].into_iter(),4),1);
        assert!(!safe_to_overlap("github_api"));
        assert!(!safe_to_overlap("browser_control"));
        assert!(!safe_to_overlap("workspace_write"));
    }
    #[test]
    fn tool_order_is_not_reordered_across_a_write_or_remote_side_effect(){
        use ToolBatch::{Exclusive,ParallelRead};
        let calls=[
            "workspace_read","workspace_read_range","workspace_write",
            "workspace_read","workspace_read","github_api",
            "workspace_read","file_read","mcp__host__get"
        ];
        assert_eq!(plan(&calls,4),vec![
            ParallelRead{start:0,len:2},Exclusive{index:2},
            ParallelRead{start:3,len:2},Exclusive{index:5},
            ParallelRead{start:6,len:2},Exclusive{index:8}
        ]);
        assert_eq!(plan(&["workspace_read";10],4),vec![
            ParallelRead{start:0,len:4},
            ParallelRead{start:4,len:4},
            ParallelRead{start:8,len:2}
        ]);
    }
    #[test]
    fn only_literal_inert_shell_observations_qualify_for_unlocked_execution(){
        use serde_json::json;
        for command in ["sleep 2.0","sleep 0","sleep 60","date","pwd","id"]{
            assert!(inert_shell_command(&json!({"command":command})), "{command}");
        }
        for command in ["sleep 61","sleep 1; touch bad","sleep $(touch bad)",
            "sleep 1 && rm -rf x","date > f","uname -a","git status",
            "cargo test","sleep 1 | cat"]{
            assert!(!inert_shell_command(&json!({"command":command})), "{command}");
        }
    }
    #[test]
    fn low_cap_runs_exclusively_in_original_model_order(){
        assert_eq!(plan(&["workspace_read","workspace_read","workspace_write"],1),
            vec![ToolBatch::Exclusive{index:0},ToolBatch::Exclusive{index:1},
                ToolBatch::Exclusive{index:2}]);
    }
}
