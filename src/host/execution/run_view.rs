//! Unified run read-model for Chat, Room, Task Team, external bridges and SDK.
//! This is a projection of the durable job/execution journals, not another
//! execution loop or another source of truth. No prompts or tool args leak.
use crate::db::Store;
use dominion_agent_sdk::{RunPhase,RunRef,RunSurface};
use serde_json::{json,Value};
pub fn normalize(store:&Store,kind:&str,record:&Value)->Option<Value>{
    let id=record["id"].as_str()?;
    let surface=match kind{
        "job"=>match record["kind"].as_str(){
            Some("room"|"meeting")=>RunSurface::Room,
            Some("team")=>RunSurface::Team,
            Some("external_agent")=>RunSurface::ExternalAgent,
            _=>RunSurface::Sdk,
        },
        "execution"=>{
            if record["roomId"].is_string()||record["roomRunId"].is_string(){
                RunSurface::Room
            }else if store.get("job",id).is_some(){
                RunSurface::Sdk
            }else{RunSurface::Chat}
        },
        _=>return None,
    };
    let parent=record["input"]["parentRunId"].as_str()
        .or_else(||record["parentRunId"].as_str())
        .map(str::to_string);
    let run=RunRef::new(surface,id,parent).ok()?;
    let status=record["status"].as_str().unwrap_or("");
    let raw=if status=="running" {
        record["phase"].as_str().unwrap_or(status)
    }else{status};
    let phase=RunPhase::from_host_status(raw)
        .or_else(||RunPhase::from_host_status(status))
        .unwrap_or(RunPhase::NeedsReview);
    Some(json!({
        "ref":run,
        "phase":phase,
        "status":status,
        "terminal":phase.terminal(),
        "attempt":record["attempts"],
        "sessionId":record["sessionId"],
        "providerId":record["nativeRoute"]["providerId"],
        "model":record["nativeRoute"]["model"],
        "reviewRequired":phase==RunPhase::NeedsReview,
        "startedAt":record["startedAt"],
        "finishedAt":record["finishedAt"],
        "sourceJournal":kind
    }))
}
pub fn lookup(store:&Store,id:&str)->Option<Value>{
    if id.len()>256||id.trim().is_empty(){return None;}
    store.get("job",id).map(|record|("job",record))
        .or_else(||store.get("execution",id).map(|record|("execution",record)))
        .and_then(|(kind,record)|normalize(store,kind,&record))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn run_contract_projects_all_surfaces_without_exposing_tool_args(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,kind,surface,status) in [
            ("team","team","team","completed"),
            ("room","room","room","running"),
            ("worker","prompt","sdk","needs_review"),
            ("external","external_agent","external_agent","cancelled")
        ]{
            store.put("job",&json!({"id":id,"kind":kind,"status":status,
                "input":{"secret":"PRIVATE_PROMPT",
                    "parentRunId":if id=="worker"{"team"}else{""}}})).unwrap();
            let actual=lookup(&store,id);
            if id=="worker"{
                assert!(actual.is_none(),
                    "Invalid parent marker must not silently forge a RunRef");
                continue;
            }
            let result=actual.unwrap();
            assert_eq!(result["ref"]["surface"],surface);
            assert_eq!(result["ref"]["version"],1);
            assert!(!result.to_string().contains("PRIVATE_PROMPT"));
        }
        store.put("execution",&json!({"id":"chat-id","status":"running",
            "phase":"waiting_approval"})).unwrap();
        let chat=lookup(&store,"chat-id").unwrap();
        assert_eq!(chat["ref"]["surface"],"chat");
        assert_eq!(chat["phase"],"waiting_approval");
        assert_eq!(chat["terminal"],false);
    }
    #[test] fn parent_id_and_child_status_are_independent(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("job",&json!({"id":"lead","kind":"team",
            "status":"running"})).unwrap();
        store.put("job",&json!({"id":"worker","kind":"prompt",
            "status":"failed","input":{"parentRunId":"lead",
                "teamTaskId":"a"}})).unwrap();
        let child=lookup(&store,"worker").unwrap();
        assert_eq!(child["ref"]["parentRunId"],"lead");
        assert_eq!(child["phase"],"failed");
        assert_eq!(child["terminal"],true);
        assert_eq!(lookup(&store,"lead").unwrap()["phase"],"running");
    }
}
