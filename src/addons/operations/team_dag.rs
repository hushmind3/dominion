//! Native Lead/Worker task graph admission on the existing job journal.
//! Edges refer only to older sibling jobs from the same parent session;
//! creation-order edges are acyclic without a model-side cycle detector.
use crate::{db::Store,state::AppState};
use serde_json::{json,Value};
use std::collections::HashSet;
const MAX_EDGES:usize=16;

#[derive(Debug,PartialEq,Eq)]
pub enum Gate { Ready, Waiting, Blocked(String) }

/// Reject foreign, missing, repeated, future and self dependencies before
/// reserving budget or preparing isolated worktrees.
pub fn validate_edges(
    store:&Store,parent:&str,new_job:&str,value:&Value,
)->Result<Vec<String>,String>{
    if value.is_null(){return Ok(Vec::new());}
    let ids=value.as_array().ok_or("dependsOnJobIds는 작업 ID 배열이어야 합니다.")?;
    if ids.len()>MAX_EDGES{return Err("작업 의존성은 최대 16개입니다.".into());}
    let mut unique=HashSet::new();
    let mut out=Vec::with_capacity(ids.len());
    for raw in ids{
        let id=raw.as_str().filter(|x|!x.is_empty()&&x.len()<=256)
            .ok_or("작업 의존성 ID 형식이 잘못되었습니다.")?;
        if id==new_job||!unique.insert(id){return Err("자기 참조 또는 중복된 작업 의존성입니다.".into());}
        let prior=store.get("job",id).ok_or("존재하지 않는 선행 작업입니다.")?;
        if prior["kind"]!="prompt"||prior["input"]["delegation"]!=true
            ||prior["input"]["parentSessionId"]!=parent {
            return Err("다른 사용 세션이나 비위임 작업을 선행 작업으로 지정할 수 없습니다.".into());
        }
        out.push(id.to_string());
    }
    Ok(out)
}

/// All edges must resolve inside the same parent session, and an isolated
/// writer is consumable only after its result diff has been frozen.
pub fn gate(store:&Store,job:&Value)->Gate{
    let Some(dependencies)=job["input"]["dependsOnJobIds"].as_array() else {
        return if job["input"]["dependsOnJobIds"].is_null(){
            Gate::Ready
        }else{Gate::Blocked("의존성 ID 목록이 손상되었습니다.".into())};
    };
    if dependencies.is_empty(){return Gate::Ready;}
    let Some(parent)=job["input"]["parentSessionId"].as_str() else{
        return Gate::Blocked("상위 세션 식별자가 없습니다.".into());
    };
    for raw in dependencies{
        let Some(id)=raw.as_str() else{
            return Gate::Blocked("선행 작업 ID가 문자열이 아닙니다.".into());
        };
        if job["id"].as_str()==Some(id){
            return Gate::Blocked("작업이 자신을 기다리는 순환입니다.".into());
        }
        let Some(dep)=store.get("job",id) else{
            return Gate::Blocked(format!("선행 작업이 삭제되었습니다: {id}"));
        };
        if dep["kind"]!="prompt"||dep["input"]["delegation"]!=true
            ||dep["input"]["parentSessionId"]!=parent {
            return Gate::Blocked(format!("선행 작업 접근 권한이 없습니다: {id}"));
        }
        match dep["status"].as_str(){
            Some("completed")=>{
                if let Some(session)=dep["input"]["sessionId"].as_str()
                    .and_then(|session|store.get("session",session)) {
                    if session["delegationWorkspace"].is_object()
                        && dep["delegationPatchSha256"].as_str().is_none(){
                        return Gate::Waiting;
                    }
                }else{
                    return Gate::Blocked(format!("선행 작업 세션이 없습니다: {id}"));
                }
            }
            Some("queued"|"running")=>return Gate::Waiting,
            Some("failed"|"cancelled"|"needs_review")=>{
                return Gate::Blocked(format!("선행 작업이 완료되지 않았습니다: {id}"));
            }
            _=>return Gate::Blocked(format!("알 수 없는 선행 작업 상태: {id}")),
        }
    }
    Gate::Ready
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
