//! Host-owned model-ordered tool scheduling, based on the public Codex and
//! DeepSeek Harness exclusive/parallel tool execution contracts.
//!
//! Scheduler metadata is not model-visible. Unknown tools remain exclusive.
//! Mutating operations settle before later reads, never the reverse.

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(super) enum ToolBatch {
    ParallelRead { start:usize,len:usize },
    Exclusive { index:usize },
}

/// Only trusted, stateless native reads can overlap.
pub(super) fn safe_to_overlap(name:&str)->bool{
    matches!(name,
        "workspace_list"|"workspace_read"|"workspace_read_range"|
        "file_list"|"file_read"|"web_search"|"http_fetch"|"memory_search"|
        "task_status"|"task_result"|"account_status")
}

/// Classify each *complete call*, including arguments, after the previous
/// exclusive barrier. Invalid argument JSON cannot be dispatched speculatively
/// in a parallel read group and remains on the ordinary tool-error path.
pub(super) fn parallel_prefix<'a>(
    calls:impl Iterator<Item=(&'a str,&'a str)>,capacity:usize
)->usize{
    if capacity<=1{return 0;}
    calls.take_while(|(name,arguments)|{
        safe_to_overlap(name)
            && serde_json::from_str::<serde_json::Value>(arguments)
                .is_ok_and(|value|value.is_object())
    }).count()
}

/// Partition one model batch into disjoint model-order execution barriers.
/// An exclusive call cannot be overtaken by a subsequent concurrent read.
pub(super) fn plan(names:&[&str],max_parallel:usize)->Vec<ToolBatch>{
    let cap=max_parallel.max(1).min(16);
    let mut batches=Vec::new();
    let mut index=0;
    while index<names.len(){
        if safe_to_overlap(names[index]) && cap>1 {
            let run=names[index..].iter().take(cap)
                .take_while(|name|safe_to_overlap(name)).count();
            if run>=2 {
                batches.push(ToolBatch::ParallelRead{start:index,len:run});
                index+=run;
                continue;
            }
        }
        batches.push(ToolBatch::Exclusive{index});
        index+=1;
    }
    batches
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_reorder_reads_across_a_mutating_barrier(){
        use ToolBatch::{Exclusive,ParallelRead};
        let calls=["workspace_read","workspace_read_range","workspace_write",
            "workspace_read","workspace_read_range","shell_exec",
            "file_read","mcp__external__read","workspace_read","workspace_read"];
        assert_eq!(plan(&calls,4),vec![
            ParallelRead{start:0,len:2},Exclusive{index:2},
            ParallelRead{start:3,len:2},Exclusive{index:5},
            Exclusive{index:6},Exclusive{index:7},
            ParallelRead{start:8,len:2},
        ]);
        assert_eq!(plan(&calls,1),(0..calls.len())
            .map(|index|Exclusive{index}).collect::<Vec<_>>());
    }
    #[test]
    fn live_argument_validation_fails_closed_and_rechecks_after_barrier(){
        let calls=[("workspace_read",r#"{"path":"a.rs"}"#),
            ("workspace_read_range",r#"{"path":"b.rs"}"#),
            ("workspace_write",r#"{"path":"a.rs","content":"new"}"#),
            ("workspace_read",r#"{"path":"a.rs"}"#),
            ("workspace_read",r#"{"path":"b.rs"}"#)];
        assert_eq!(parallel_prefix(calls.iter().copied(),4),2);
        assert_eq!(parallel_prefix(calls[2..].iter().copied(),4),0);
        assert_eq!(parallel_prefix(calls[3..].iter().copied(),4),2);
        assert_eq!(parallel_prefix([("workspace_read","not-json"),
            ("workspace_read",r#"{"path":"b"}"#)].into_iter(),4),0);
        assert_eq!(parallel_prefix([("workspace_read",r#"{"path":"a"}"#),
            ("workspace_read","[]")].into_iter(),4),1);
        assert_eq!(parallel_prefix([("workspace_read",r#"{"path":"a"}"#),
            ("workspace_read",r#"{"path":"b"}"#)].into_iter(),1),0);
    }
    #[test]
    fn parallel_reads_are_bounded_and_unknown_tools_are_exclusive(){
        assert_eq!(plan(&["workspace_read";10],4),vec![
            ToolBatch::ParallelRead{start:0,len:4},
            ToolBatch::ParallelRead{start:4,len:4},
            ToolBatch::ParallelRead{start:8,len:2},
        ]);
        for name in ["workspace_write","terminal_exec","github_api",
             "delegate_parallel","mcp__calendar__get","browser_control","shell_exec"]{
            assert!(!safe_to_overlap(name),"{name} must be exclusive");
        }
        assert!(safe_to_overlap("workspace_read"));
    }
}
