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

/// Outcomes keep their original indexes even when a faster sibling returns
/// first. On cancellation or unexpected dispatch failure the pool stops
/// replenishing but drains all started futures before returning.
pub(super) struct RollingOutcome<T,E>{
    pub results:Vec<Option<Result<T,E>>>,
    pub cancelled:bool,
    pub failed:bool,
    pub unstarted:usize,
}

pub(super) async fn run_rolling<I,T,E,F,Fut,C>(
    inputs:Vec<I>,capacity:usize,run:F,should_cancel:C
)->RollingOutcome<T,E>
where
    F:Fn(I)->Fut,
    Fut:std::future::Future<Output=Result<T,E>>,
    C:Fn()->bool,
{
    use futures_util::stream::{FuturesUnordered,StreamExt};
    let total=inputs.len();
    let mut waiting=inputs.into_iter().enumerate();
    let make=|(index,item):(usize,I)|{
        let work=run(item);
        async move{(index,work.await)}
    };
    let mut running=FuturesUnordered::new();
    let mut cancelled=should_cancel();
    let mut failed=false;
    if !cancelled{
        for _ in 0..capacity.max(1).min(16){
            if let Some(item)=waiting.next(){running.push(make(item));}
        }
    }
    let mut results=(0..total).map(|_|None).collect::<Vec<Option<Result<T,E>>>>();
    while !running.is_empty(){
        if should_cancel(){cancelled=true;}
        let next=tokio::time::timeout(
            std::time::Duration::from_millis(150),running.next()).await;
        let Ok(Some((index,result)))=next else{continue;};
        if result.is_err(){failed=true;}
        results[index]=Some(result);
        // A completion can itself trigger cancellation. Check again BEFORE
        // admitting another call into the bounded pool.
        if should_cancel(){cancelled=true;}
        if !cancelled && !failed{
            if let Some(item)=waiting.next(){running.push(make(item));}
        }
    }
    RollingOutcome{results,cancelled,failed,unstarted:waiting.count()}
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
    #[tokio::test]
    async fn rolling_pool_respects_cap_commits_order_and_reuses_open_slots(){
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let active=Arc::new(AtomicUsize::new(0));
        let peak=Arc::new(AtomicUsize::new(0));
        let a=active.clone();let p=peak.clone();
        let outcome=run_rolling((0usize..10).collect(),3,move |id|{
            let a=a.clone();let p=p.clone();
            async move{
                let current=a.fetch_add(1,Ordering::SeqCst)+1;
                p.fetch_max(current,Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(
                    if id==0{35}else{4}
                )).await;
                a.fetch_sub(1,Ordering::SeqCst);
                Ok::<usize,&'static str>(id)
            }
        },||false).await;
        assert!(!outcome.cancelled && !outcome.failed);
        assert_eq!(outcome.unstarted,0);
        assert_eq!(active.load(Ordering::SeqCst),0);
        assert_eq!(peak.load(Ordering::SeqCst),3);
        assert_eq!(outcome.results.into_iter().map(|v|v.unwrap().unwrap())
            .collect::<Vec<_>>(),(0usize..10).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn unexpected_dispatch_failure_drains_started_tools_without_replenishing(){
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let started=Arc::new(AtomicUsize::new(0));
        let alive=Arc::new(AtomicUsize::new(0));
        let s=started.clone();let a=alive.clone();
        let outcome=run_rolling((0usize..8).collect(),3,move |id|{
            let s=s.clone();let a=a.clone();
            async move{
                s.fetch_add(1,Ordering::SeqCst);
                a.fetch_add(1,Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(
                    if id==0{1}else{30}
                )).await;
                a.fetch_sub(1,Ordering::SeqCst);
                if id==0{Err("real internal failure")}else{Ok(id)}
            }
        },||false).await;
        assert!(outcome.failed);
        assert!(!outcome.cancelled);
        assert_eq!(started.load(Ordering::SeqCst),3);
        assert_eq!(alive.load(Ordering::SeqCst),0);
        assert_eq!(outcome.unstarted,5);
        assert_eq!(outcome.results.iter().filter(|v|v.is_some()).count(),3);
        assert!(matches!(outcome.results[0],Some(Err("real internal failure"))));
    }

    #[tokio::test]
    async fn owner_cancel_drains_inflight_and_never_launches_queued_calls(){
        use std::sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}};
        let stopped=Arc::new(AtomicBool::new(false));
        let started=Arc::new(AtomicUsize::new(0));
        let flag=stopped.clone();let count=started.clone();let observe=stopped.clone();
        let outcome=run_rolling((0usize..8).collect(),3,move |id|{
            let flag=flag.clone();let count=count.clone();
            async move{
                count.fetch_add(1,Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(
                    if id==0{2}else{25}
                )).await;
                if id==0{flag.store(true,Ordering::SeqCst);}
                Ok::<usize,&'static str>(id)
            }
        },move ||observe.load(Ordering::SeqCst)).await;
        assert!(outcome.cancelled);
        assert!(!outcome.failed);
        assert_eq!(started.load(Ordering::SeqCst),3);
        assert_eq!(outcome.unstarted,5);
        assert_eq!(outcome.results.iter().filter(|v|v.is_some()).count(),3);
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
