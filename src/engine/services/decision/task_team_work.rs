//! Durable ownership and evidence for one ProgramIR Task Team node.
//! Every tool call is still performed by the single native tool gateway.
//! These records are execution decisions, not model-authored claims.
use crate::{engine::memory,state::AppState};
use serde_json::{json,Value};

fn id(execution_id:&str,revision:&str,parent_id:&str,index:usize)->String{
    format!("team-work-{}",memory::stable_id(&[
        execution_id,revision,parent_id,&index.to_string()
    ]))
}
fn matching(row:&Value,execution_id:&str,session_id:&str,revision:&str,
    parent_id:&str,index:usize,node:&Value)->bool{
    row["executionId"]==execution_id && row["sessionId"]==session_id
    && row["programRevision"]==revision && row["teamNodeId"]==parent_id
    && row["index"]==index && row["nodeId"]==node["id"]
    && row["nodeType"]==node["type"]
    && row["agentId"]==node["agentId"] && row["dependsOn"]==node["dependsOn"]
}
/// Idempotent assignment. No untrusted peer or model claim can impersonate
/// another run; execution ownership must already be committed by the host.
pub fn assign(
    state:&AppState,session_id:&str,execution_id:&str,revision:&str,
    parent_id:&str,index:usize,node:&Value,
)->Result<Value,String>{
    let run=state.store.get("execution",execution_id)
        .ok_or("Task Team execution missing")?;
    if run["sessionId"]!=session_id || run["status"]!="running"{
        return Err("Task Team execution is not live or belongs to another session".into());
    }
    if revision.trim().is_empty(){return Err("Task Team revision required".into());}
    let work_id=id(execution_id,revision,parent_id,index);
    let new=json!({
        "id":work_id,"kind":"program_team_node",
        "executionId":execution_id,"sessionId":session_id,
        "programRevision":revision,"teamNodeId":parent_id,
        "index":index,"nodeId":node["id"],"nodeType":node["type"],
        "agentId":node["agentId"],"dependsOn":node["dependsOn"],
        "status":"assigned","attempt":0,
    });
    if let Some(saved)=state.store.put_if_absent("task_team_work",&new){
        state.bus.record("task_team_work",&saved);
        return Ok(saved);
    }
    state.store.get("task_team_work",&work_id)
        .filter(|prior|matching(prior,execution_id,session_id,
            revision,parent_id,index,node))
        .ok_or("Task Team assignment scope/revision changed on replay".into())
}
/// A crash during a running effect leaves a fence, not an automatic replay.
/// The owner must inspect the tool receipt before issuing a new revision.
pub fn claim(state:&AppState,assigned:&Value)->Result<Value,String>{
    let id=assigned["id"].as_str().ok_or("Task Team assignment ID missing")?;
    let live=state.store.get("execution",assigned["executionId"].as_str()
        .ok_or("Task Team execution ID missing")?)
        .ok_or("Task Team execution disappeared")?;
    if live["status"]!="running" || live["sessionId"]!=assigned["sessionId"]{
        return Err("Task Team execution lost its owner".into());
    }
    let saved=state.store.patch_if("task_team_work",id,"status",
        &json!("assigned"),&json!({"status":"running","attempt":1,
            "startedAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("Task Team work already started or needs review; unsafe replay denied")?;
    state.bus.record("task_team_work",&saved);
    Ok(saved)
}
fn tool_receipt(state:&AppState,assignment:&Value,node:&Value)
    ->Result<Value,String>{
    let execution=assignment["executionId"].as_str().ok_or("missing execution ID")?;
    let session=assignment["sessionId"].as_str().ok_or("missing session ID")?;
    let id=node["id"].as_str().filter(|s|!s.is_empty())
        .ok_or("tool node identity missing")?;
    let call_prefix=format!("program-{id}-");
    let tool=if node["type"]=="delegate"{"delegate_task"}
        else{node["tool"].as_str().ok_or("missing tool name")?};
    let entries=state.store.execution_operations(execution)?;
    let entry=entries.into_iter().rev().find(|op|{
        op["runId"]==execution && op["sessionId"]==session
            && op["tool"]==tool
            && op["callId"].as_str().is_some_and(|c|c.starts_with(&call_prefix))
            && matches!(op["status"].as_str(),Some("completed"|"failed"))
            && op["outcomeUnknown"]!=true
            && op["effectState"]!="effect_unknown"
            && op["effectState"]!="needs_review"
    }).ok_or("Task Team tool completion lacks a matching committed receipt")?;
    Ok(entry)
}
/// A parent can settle only when its checkpoint reports this child complete.
/// Tool/delegation claims additionally require the host's committed operation,
/// including its real status. Observed failure is never reported as verified.
pub fn settle(
    state:&AppState,assignment:&Value,node:&Value,checkpoint:&Value,
)->Result<Value,String>{
    let node_id=node["id"].as_str().filter(|s|!s.is_empty());
    if node_id.is_some_and(|id|
        !checkpoint["completedOperationIds"].as_array()
            .is_some_and(|done|done.iter().any(|v|v==id))){
        return Err("Task Team node has no completed checkpoint".into());
    }
    let evidence=if matches!(node["type"].as_str(),Some("call_tool"|"delegate")){
        let operation=tool_receipt(state,assignment,node)?;
        let success=operation["status"]=="completed";
        json!({
            "kind":if success{"tool_completed_observed"}else{"tool_failed_observed"},
            "operationId":operation["id"],"toolVerified":
                operation["toolReceipt"]["verified"]==true,
        })
    }else{
        json!({"kind":"interpreter_completed","toolVerified":false})
    };
    let id=assignment["id"].as_str().ok_or("Task Team ID missing")?;
    let saved=state.store.patch_if("task_team_work",id,"status",
        &json!("running"),&json!({"status":"completed","evidence":evidence,
            "completedAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("Task Team completion is stale or was never claimed")?;
    state.bus.record("task_team_work",&saved);
    Ok(saved)
}
pub fn block(state:&AppState,assignment:&Value,reason:&str)->Result<Value,String>{
    let id=assignment["id"].as_str().ok_or("Task Team ID missing")?;
    let saved=state.store.patch_if("task_team_work",id,"status",
        &json!("running"),&json!({"status":"needs_review",
            "reason":reason.chars().take(160).collect::<String>()}))
        .ok_or("Task Team failure transition is stale")?;
    state.bus.record("task_team_work",&saved);
    Ok(saved)
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-team-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("execution",&json!({"id":"run-a","sessionId":"s",
            "status":"running","agentId":"lead"})).unwrap();
        (state,dir)
    }
    #[test]
    fn team_work_can_finish_only_after_host_operation_and_checkpoint(){
        let (state,dir)=state();
        let node=json!({"type":"call_tool","id":"write","tool":"workspace_write",
            "inputs":{"path":"x","content":"ok"}});
        let first=assign(&state,"s","run-a","r1","team",0,&node).unwrap();
        assert_eq!(first["status"],"assigned");
        assert_eq!(assign(&state,"s","run-a","r1","team",0,&node).unwrap()["id"],first["id"]);
        assert!(assign(&state,"wrong","run-a","r1","team",0,&node).is_err());
        let live=claim(&state,&first).unwrap();
        assert!(claim(&state,&first).is_err());
        let checkpoint=json!({"completedOperationIds":["write"]});
        assert!(settle(&state,&live,&node,&checkpoint).is_err());
        state.store.put("execution_operation",&json!({"id":"o1",
            "runId":"run-a","sessionId":"s","tool":"workspace_write",
            "callId":"program-write-first","status":"completed",
            "toolReceipt":{"verified":true},"effectState":"verified"})).unwrap();
        assert!(settle(&state,&live,&node,
            &json!({"completedOperationIds":[]})).is_err());
        let done=settle(&state,&live,&node,&checkpoint).unwrap();
        assert_eq!(done["status"],"completed");
        assert_eq!(done["evidence"]["operationId"],"o1");
        assert_eq!(done["evidence"]["toolVerified"],true);
        assert!(settle(&state,&live,&node,&checkpoint).is_err());
        assert!(claim(&state,&first).is_err());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn interrupted_work_fences_replay_and_never_invents_success(){
        let (state,dir)=state();
        let node=json!({"type":"delegate","id":"a","agentId":"worker"});
        let entry=assign(&state,"s","run-a","r1","team",1,&node).unwrap();
        let started=claim(&state,&entry).unwrap();
        assert!(claim(&state,&entry).is_err());
        let stopped=block(&state,&started,"tool result unknown").unwrap();
        assert_eq!(stopped["status"],"needs_review");
        assert!(settle(&state,&stopped,&node,
            &json!({"completedOperationIds":["a"]})).is_err());
        let replay=assign(&state,"s","run-a","r1","team",1,&node).unwrap();
        assert_eq!(replay["status"],"needs_review");
        assert!(claim(&state,&replay).is_err());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
}