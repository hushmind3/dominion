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
    && row["requestedAgentId"]==node["agentId"]
    && row["fallbackAgentIds"]==node["fallbackAgentIds"]
    && row["dependsOn"]==node["dependsOn"]
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
        "sourceMessageId":run["sourceMessageId"],
        "index":index,"nodeId":node["id"],"nodeType":node["type"],
        "agentId":node["agentId"],"requestedAgentId":node["agentId"],
        "fallbackAgentIds":node["fallbackAgentIds"],
        "dependsOn":node["dependsOn"],
        "leadAgentId":run["agentId"],"onFailure":node["onFailure"],
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
/// Unavailable workers are replaced ONLY while the original assignment has
/// never started. Any claimed/delegated/unknown effect remains a review fence.
pub fn reassign_unstarted(
    state:&AppState,assignment:&Value,node:&Value,
)->Result<Value,String>{
    if node["type"]!="delegate"{return Ok(assignment.clone());}
    let original=node["agentId"].as_str().ok_or("original delegate missing")?;
    let id=assignment["id"].as_str().ok_or("Task Team assignment ID missing")?;
    let available=|agent:&str|state.store.get("agent",agent)
        .is_some_and(|row|row["disabled"]!=true && row["enabled"]!=false);
    if available(original){return Ok(assignment.clone());}
    if assignment["status"]!="assigned"{
        return Err("delegate unavailable after execution started; review required".into());
    }
    let replacement=node["fallbackAgentIds"].as_array()
        .and_then(|items|items.iter().filter_map(Value::as_str)
            .find(|candidate|*candidate!=original && available(candidate)))
        .ok_or("no registered backup delegate available")?;
    if assignment["agentId"]==replacement && assignment["reassignedFrom"]==original{
        return Ok(assignment.clone());
    }
    let run_id=assignment["executionId"].as_str().ok_or("parent run missing")?;
    let run=state.store.get("execution",run_id)
        .ok_or("Task Team parent run missing")?;
    if run["status"]!="running" || run["sessionId"]!=assignment["sessionId"]{
        return Err("Task Team parent owner is not live".into());
    }
    let guards=[("status",json!("assigned")),("agentId",assignment["agentId"].clone())];
    let refs=guards.iter().map(|(key,value)|(*key,value)).collect::<Vec<_>>();
    if let Some(saved)=state.store.patch_if_all_merge(
        "task_team_work",id,&refs,
        &json!({"agentId":replacement,"reassignedFrom":original,
            "reassignmentReason":"original_agent_unavailable_before_dispatch",
            "reassignedAt":chrono::Utc::now().to_rfc3339()}),&[]
    ){
        state.bus.record("task_team_work",&saved);
        return Ok(saved);
    }
    state.store.get("task_team_work",id)
        .filter(|row|row["status"]=="assigned"&&row["agentId"]==replacement
            &&row["requestedAgentId"]==original)
        .ok_or("Task Team claim/reassign collision; no extra work dispatched".into())
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
/// Deterministic parent result, generated only from committed worker records.
/// A failed tool may be treated as an observation only when the owning
/// ProgramIR explicitly requests "onFailure":"continue"; never verified.
pub fn finish_team(
    state:&AppState,assignments:&[Value],checkpoint:&Value,
)->Result<Value,String>{
    let first=assignments.first().ok_or("empty Task Team")?;
    let execution_id=first["executionId"].as_str().ok_or("team execution missing")?;
    let revision=first["programRevision"].as_str().ok_or("team revision missing")?;
    let scope=first["teamNodeId"].as_str().ok_or("team scope missing")?;
    let session_id=first["sessionId"].as_str().ok_or("team session missing")?;
    let mut evidence=Vec::new();
    let mut verified=true;
    let mut failures=0usize;
    for member in assignments{
        let id=member["id"].as_str().ok_or("team work ID missing")?;
        let persisted=state.store.get("task_team_work",id)
            .ok_or("team work disappeared")?;
        if persisted["status"]!="completed"
            || persisted["executionId"]!=execution_id
            || persisted["programRevision"]!=revision
            || persisted["sessionId"]!=session_id
            || persisted["teamNodeId"]!=scope {
            return Err("team contains unfinished or foreign work".into());
        }
        if let Some(node_id)=persisted["nodeId"].as_str(){
            if !checkpoint["completedOperationIds"].as_array()
                .is_some_and(|completed|completed.iter().any(|id|id==node_id)){
                return Err("team member is not completed in its checkpoint".into());
            }
        }
        let status=persisted["evidence"]["kind"].as_str()
            .ok_or("team member has no committed evidence")?;
        if status=="tool_failed_observed"{
            if persisted["onFailure"]!="continue"{
                return Err("failed team work has no explicit continue policy".into());
            }
            failures+=1;
        }
        let member_verified=persisted["evidence"]["toolVerified"]==true;
        verified&=member_verified;
        evidence.push(json!({
            "workId":id,"nodeId":persisted["nodeId"],
            "agentId":persisted["agentId"],
            "status":status,"operationId":persisted["evidence"]["operationId"],
            "verified":member_verified,
        }));
    }
    let id=format!("team-result-{}",memory::stable_id(&[
        execution_id,revision,scope]));
    let result=json!({
        "id":id,"kind":"program_task_team",
        "executionId":execution_id,"sessionId":session_id,
        "programRevision":revision,"teamNodeId":scope,
        "status":"completed","members":evidence,"memberCount":assignments.len(),
        "observedFailures":failures,
        "evidenceVerified":verified,
    });
    if let Some(saved)=state.store.put_if_absent("task_team_result",&result){
        state.bus.record("task_team_result",&saved);
        return Ok(saved);
    }
    state.store.get("task_team_result",&id).filter(|old|
        old["executionId"]==execution_id
        && old["sessionId"]==session_id
        && old["programRevision"]==revision
        && old["teamNodeId"]==scope
        && old["members"]==result["members"]
    ).ok_or_else(||"team result changed on replay".to_string())
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
    fn team_result_requires_all_workers_and_is_not_a_narrative_success(){
        let (state,dir)=state();
        let a=json!({"id":"read","type":"call_tool","tool":"workspace_read"});
        let b=json!({"id":"write","type":"call_tool","tool":"workspace_write"});
        let worker_a=assign(&state,"s","run-a","r1","team",0,&a).unwrap();
        let worker_b=assign(&state,"s","run-a","r1","team",1,&b).unwrap();
        let claim_a=claim(&state,&worker_a).unwrap();
        let _claim_b=claim(&state,&worker_b).unwrap();
        let checkpoint=json!({"completedOperationIds":["read","write"]});
        assert!(finish_team(&state,&[worker_a.clone(),worker_b.clone()],&checkpoint).is_err());
        state.store.put("execution_operation",&json!({
            "id":"read-op","runId":"run-a","sessionId":"s",
            "tool":"workspace_read","callId":"program-read-call",
            "status":"completed","toolReceipt":{"verified":true}
        })).unwrap();
        settle(&state,&claim_a,&a,&checkpoint).unwrap();
        assert!(finish_team(&state,&[worker_a.clone(),worker_b.clone()],&checkpoint).is_err());
        state.store.put("execution_operation",&json!({
            "id":"write-op","runId":"run-a","sessionId":"s",
            "tool":"workspace_write","callId":"program-write-call",
            "status":"completed","toolReceipt":{"verified":true}
        })).unwrap();
        settle(&state,&worker_b,&b,&checkpoint).unwrap();
        let merged=finish_team(&state,&[worker_a.clone(),worker_b.clone()],
            &checkpoint).unwrap();
        assert_eq!(merged["status"],"completed");
        assert_eq!(merged["memberCount"],2);
        assert_eq!(merged["evidenceVerified"],true);
        assert_eq!(finish_team(&state,&[worker_a,worker_b],&checkpoint).unwrap()["id"],merged["id"]);
        assert_eq!(state.store.all("task_team_result").len(),1);
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn unavailable_worker_fails_over_before_dispatch_only(){
        let (state,dir)=state();
        state.store.put("agent",&json!({"id":"backup","name":"backup"})).unwrap();
        let node=json!({"id":"delegate1","type":"delegate",
            "agentId":"missing","fallbackAgentIds":["backup"]});
        let first=assign(&state,"s","run-a","r1","team",3,&node).unwrap();
        let updated=reassign_unstarted(&state,&first,&node).unwrap();
        assert_eq!(updated["agentId"],"backup");
        assert_eq!(updated["requestedAgentId"],"missing");
        assert_eq!(reassign_unstarted(&state,&updated,&node).unwrap()["id"],first["id"]);
        let started=claim(&state,&updated).unwrap();
        assert!(reassign_unstarted(&state,&started,&node).is_err());
        assert!(claim(&state,&first).is_err());
        let unexpected=json!({"id":"delegate1","type":"delegate",
            "agentId":"missing","fallbackAgentIds":["unauthorized"]});
        assert!(assign(&state,"s","run-a","r1","team",3,&unexpected).is_err());
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