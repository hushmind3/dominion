//! Native Lead/Worker task graph admission on the existing job journal.
//! Edges refer only to older sibling jobs from the same parent session;
//! creation-order edges are acyclic without a model-side cycle detector.
use crate::{db::Store,state::AppState};
use serde_json::{json,Value};
pub use dominion_algorithms::team_dag::Gate;
pub fn validate_edges(store:&Store,parent:&str,new_job:&str,value:&Value)->Result<Vec<String>,String>{
    dominion_algorithms::team_dag::validate_edges(|id|store.get("job",id),parent,new_job,value)
}
pub fn gate(store:&Store,job:&Value)->Gate{
    dominion_algorithms::team_dag::gate(|id|store.get("job",id),|id|store.get("session",id),job)
}

/// Fail closed on an unsatisfied prerequisite before claiming a worker slot.
/// Terminalization is visible as an ordinary durable job status.
pub fn ready_or_block(state:&AppState,job:&Value)->bool{
    if job["input"]["delegation"]==true {
        if let Some(parent)=job["input"]["parentSessionId"].as_str(){
            let generation_changed=job["input"]["parentGeneration"].as_u64()
                .is_some_and(|generation|generation!=state.cancellation_generation(parent));
            if state.is_cancelled(parent)||generation_changed{
                if let Some(id)=job["id"].as_str(){
                    if let Some(saved)=state.store.patch_if("job",id,"status",
                        &json!("queued"),&json!({"status":"cancelled",
                            "cancelRequested":true,
                            "error":"부모 세션의 취소 또는 새 실행 세대 때문에 위임 작업을 시작하지 않습니다.",
                            "finishedAt":chrono::Utc::now().to_rfc3339()})){
                        state.bus.record("job",&saved);
                        state.store.audit("team.parent_cancelled",
                            json!({"jobId":id,"parentSessionId":parent}));
                    }
                }
                return false;
            }
        }
    }
    match gate(&state.store,job){
        Gate::Ready=>true,
        Gate::Waiting=>false,
        Gate::Blocked(error)=>{
            if let Some(id)=job["id"].as_str(){
                if let Some(saved)=state.store.patch_if("job",id,"status",
                    &json!("queued"),&json!({"status":"needs_review",
                        "error":format!("선행 작업 실패: {error}"),
                        "finishedAt":chrono::Utc::now().to_rfc3339()})){
                    state.bus.record("job",&saved);
                    state.store.audit("team.dependency_blocked",
                        json!({"jobId":id,"error":error}));
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn cancelled_parent_prevents_worker_admission_even_after_restart_epoch(){
        let state={
            let root=std::env::temp_dir().join(format!("dominion-dag-{}",uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let config=crate::config::Config{port:0,data_dir:root.clone(),
                web_dir:root.join("web"),workspace:root};
            AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
                crate::vault::Vault::open(&config.key_path()))
        };
        let generation=state.cancellation_generation("parent");
        let queued=state.store.put("job",&json!({
            "id":"worker","kind":"prompt","status":"queued",
            "input":{"delegation":true,"parentSessionId":"parent",
                "parentGeneration":generation}
        })).unwrap();
        assert!(ready_or_block(&state,&queued));
        state.cancel_session("parent");
        assert!(!ready_or_block(&state,&queued));
        assert_eq!(state.store.get("job","worker").unwrap()["status"],"cancelled");
    }
    #[test]
    fn sibling_graph_waits_for_completion_and_freeze_then_blocks_failed_nodes(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("session",&json!({"id":"child1","delegationWorkspace":{"path":"/tmp/test"}})).unwrap();
        store.put("job",&json!({"id":"j1","kind":"prompt","status":"running",
            "input":{"delegation":true,"sessionId":"child1","parentSessionId":"owner"}})).unwrap();
        let deps=json!(["j1"]);
        assert_eq!(validate_edges(&store,"owner","j2",&deps).unwrap(),vec!["j1"]);
        assert!(validate_edges(&store,"another","j2",&deps).is_err());
        assert!(validate_edges(&store,"owner","j1",&deps).is_err());
        assert!(validate_edges(&store,"owner","j2",&json!(["j1","j1"])).is_err());
        assert!(validate_edges(&store,"owner","j2",&json!(["missing"])).is_err());
        let task=json!({"id":"j2","input":{"parentSessionId":"owner","dependsOnJobIds":["j1"]}});
        assert_eq!(gate(&store,&task),Gate::Waiting);
        store.put("job",&json!({"id":"j1","kind":"prompt","status":"completed",
            "input":{"delegation":true,"sessionId":"child1","parentSessionId":"owner"}})).unwrap();
        assert_eq!(gate(&store,&task),Gate::Waiting,
            "status completed before worktree freeze is not final evidence");
        store.patch_if("job","j1","status",&json!("completed"),
            &json!({"delegationPatchSha256":"sha-proof"})).unwrap();
        assert_eq!(gate(&store,&task),Gate::Ready);
        store.patch_if("job","j1","status",&json!("completed"),
            &json!({"status":"failed"})).unwrap();
        assert!(matches!(gate(&store,&task),Gate::Blocked(_)));
    }
}
