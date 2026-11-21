//! Remote SDK research Worker for the native Lead/Worker DAG.
//! The coordinator cannot claim native verified tool receipts for remote work.
use crate::{addons::{jobs::JobContext,team_orchestrator},state::AppState};
use serde_json::{json,Value};
use chrono::Utc;

fn owns(state:&AppState,job:&Value)->Result<(),String>{
    let team_id=job["input"]["teamJobId"].as_str().ok_or("remote team missing")?;
    let id=job["id"].as_str().ok_or("remote child missing")?;
    let team=state.store.get("job",team_id).ok_or("remote lead missing")?;
    if team["kind"]!="team"||job["kind"]!="prompt"
        ||job["input"]["delegation"]!=true||job["input"]["remoteWorker"]!=true
        ||team["input"]["sessionId"]!=job["input"]["parentSessionId"]
        ||!team["input"]["childJobIds"].as_array().is_some_and(|items|
            items.iter().any(|child|child.as_str()==Some(id)))
        ||!state.store.get("session",job["input"]["sessionId"].as_str().unwrap_or(""))
            .is_some_and(|s|s["teamChildJobId"]==id
                &&s["teamJobId"]==team_id
                &&s["delegationProfile"]=="general"){
        return Err("unowned or unsupported remote Team node".into());
    }
    Ok(())
}
fn eligible(row:&Value,job:&Value)->bool{
    row["status"]=="owner_accepted"&&row["sourceJobId"]==job["id"]
        &&row["teamJobId"]==job["input"]["teamJobId"]
        &&row["sessionId"]==job["input"]["sessionId"]
        &&row["workerReport"]["outcome"]=="completed"
        &&row["workerReport"]["remoteNativeExecutionId"].as_str()
            .is_some_and(|s|!s.is_empty()&&s.len()<=256)
        &&row["workerReport"]["answer"].as_str()
            .is_some_and(|s|!s.trim().is_empty()&&s.len()<=12_000)
}
fn change(state:&AppState,id:&str,from:&str,status:&str,reason:&str){
    if let Some(saved)=state.store.patch_if("remote_job",id,"status",&json!(from),
        &json!({"status":status,"reviewReason":reason,"leaseId":null,
            "cancelRequested":status=="cancelled",
            "updatedAt":chrono::Utc::now().to_rfc3339()})){
        state.bus.record("remote_job",&saved);
    }
}
pub fn evidence(state:&AppState,job:&Value,id:&str)->Result<Value,String>{
    owns(state,job)?;
    let remote=state.store.get("remote_job",id).ok_or("remote evidence missing")?;
    if !eligible(&remote,job){return Err("remote evidence not owner accepted".into());}
    Ok(json!({"remoteJobId":id,
        "remoteExecutionId":remote["workerReport"]["remoteNativeExecutionId"],
        "nativeVerified":false,"evidenceLevel":"remote_owner_reviewed"}))
}
pub async fn run(ctx:&JobContext,job:&Value)->Result<Value,String>{
    owns(&ctx.state,job)?;
    let id=job["id"].as_str().ok_or("remote child missing")?;
    let team_id=job["input"]["teamJobId"].as_str().unwrap_or("");
    let session=job["input"]["sessionId"].as_str().ok_or("remote session missing")?;
    let parent=job["input"]["parentSessionId"].as_str().unwrap_or("");
    let generation=job["input"]["parentGeneration"].as_u64().ok_or("parent epoch missing")?;
    let prompt=job["input"]["prompt"].as_str().filter(|s|!s.is_empty()&&s.len()<=16_000)
        .ok_or("remote prompt is missing")?;
    let text=format!("{}{}",prompt,
        team_orchestrator::dependency_evidence(&ctx.state,job)?);
    let remote_id=format!("remote:{id}");
    // Record the replay fence BEFORE publishing the remote work ticket.
    ctx.checkpoint(json!({"sessionId":session,"remoteJobId":remote_id,
        "promptState":{"inFlight":true}}))?;
    let candidate=json!({"id":remote_id,"kind":"remote_sdk","status":"queued",
        "sourceJobId":id,"teamJobId":team_id,"sessionId":session,
        "parentSessionId":parent,"instruction":text,"maxRounds":24,
        "attempts":0,"nativeVerified":false,
        "createdAt":chrono::Utc::now().to_rfc3339()});
    if let Some(created)=ctx.state.store.put_if_absent("remote_job",&candidate){
        ctx.state.bus.record("remote_job",&created);
    }
    // One scheduler tick observes the durable ticket, then yields the local
    // worker slot. No local worker waits an hour for a remote approval.
    let record=ctx.state.store.get("remote_job",&remote_id)
            .ok_or("remote job has no durable record")?;
        if record["sourceJobId"]!=id||record["teamJobId"]!=team_id
            ||record["sessionId"]!=session||record["instruction"]!=text{
            return Err("remote job identity mismatch".into());
        }
        if ctx.state.is_cancelled(parent)||ctx.state.is_cancelled(session)
            ||ctx.state.cancellation_generation(parent)!=generation
            ||ctx.state.store.get("job",id)
                .is_none_or(|j|j["status"]=="cancelled"||j["cancelRequested"]==true){
            for status in ["queued","leased","needs_review"]{
                change(&ctx.state,&remote_id,status,"cancelled","owner_cancelled");
            }
            return Ok(json!({"status":"cancelled","remoteJobId":remote_id}));
        }
        if eligible(&record,job){
            let answer=record["workerReport"]["answer"].as_str().unwrap_or("");
            let saved=ctx.state.store.write_message(&json!({
                "session_id":session,"role":"assistant",
                "agent_id":job["input"]["agentId"],"content":answer,
                "metadata":{"authorKind":"agent","verification":"remote_owner_reviewed",
                    "remoteJobId":remote_id,"remoteWorkerId":record["workerId"],
                    "remoteNativeExecutionId":record["workerReport"]["remoteNativeExecutionId"],
                    "remoteRuntime":"dominion_sdk","nativeToolReceipts":false}
            })).ok_or("remote report could not be journaled")?;
            ctx.state.bus.message(&saved);
            let result=json!({"sessionId":session,"messageId":saved["id"],
                "remoteJobId":remote_id,"remoteVerified":false});
            ctx.checkpoint(json!({"promptState":{"inFlight":false,"result":result}}))?;
            return Ok(result);
        }
        match record["status"].as_str(){
            Some("owner_accepted"|"owner_rejected"|"cancelled")=>
                return Ok(json!({"status":"needs_review","remoteJobId":remote_id,
                    "reason":"unusable or rejected remote evidence"})),
            Some("needs_review") if record["workerReport"].is_null()=>
                return Ok(json!({"status":"needs_review","remoteJobId":remote_id,
                    "reason":"remote execution outcome unknown"})),
            Some("leased") if record["leaseExpiresAt"].as_str()
                .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_none_or(|t|t<=chrono::Utc::now())=>{
                    change(&ctx.state,&remote_id,"leased","needs_review","lease_expired");
                    return Ok(json!({"status":"needs_review","remoteJobId":remote_id,
                        "reason":"remote lease expired"}));
                },
            Some("queued"|"leased"|"needs_review")=>{},
            _=>return Err("invalid remote job state".into())
        }
        let stale=record["createdAt"].as_str()
            .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_none_or(|created|Utc::now().signed_duration_since(created)
                >chrono::Duration::hours(1));
        if stale{
            for status in ["queued","leased"]{
                change(&ctx.state,&remote_id,status,"needs_review","deadline_unknown");
            }
            return Ok(json!({"status":"needs_review","remoteJobId":remote_id,
                "reason":"remote work or owner review timed out"}));
        }
        Err("deferred: remote result awaiting terminal evidence and owner review".into())
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn sample()->(AppState,Value){
        let root=std::env::temp_dir().join(format!("dominion-remote-team-{}",uuid::Uuid::new_v4()));
        let cfg=Config{port:0,data_dir:root,workspace:std::env::temp_dir(),
            web_dir:std::env::temp_dir()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"lead"})).unwrap();
        state.store.put("session",&json!({"id":"child","teamJobId":"team",
            "teamChildJobId":"worker","delegationProfile":"general"})).unwrap();
        state.store.put("job",&json!({"id":"team","kind":"team",
            "input":{"sessionId":"lead","childJobIds":["worker"]}})).unwrap();
        let job=state.store.put("job",&json!({"id":"worker","kind":"prompt",
            "status":"running","input":{"remoteWorker":true,"delegation":true,
                "teamJobId":"team","sessionId":"child","parentSessionId":"lead",
                "parentGeneration":state.cancellation_generation("lead"),
                "prompt":"Research","agentId":"a"}})).unwrap();
        (state,job)
    }
    #[tokio::test]
    async fn remotely_accepted_research_commits_message_without_native_tool_receipt(){
        let (state,job)=sample();
        state.store.put("remote_job",&json!({"id":"remote:worker","status":"owner_accepted",
            "sourceJobId":"worker","teamJobId":"team","sessionId":"child",
            "instruction":"Research","workerId":"node-1",
            "workerReport":{"outcome":"completed","answer":"Remote evidence",
                "remoteNativeExecutionId":"remote-execution"}})).unwrap();
        let outcome=run(&JobContext::new(state.clone(),"worker".into()),&job).await.unwrap();
        assert_eq!(outcome["remoteVerified"],false);
        assert_eq!(evidence(&state,&job,"remote:worker").unwrap()["nativeVerified"],false);
        let msg=state.store.message(outcome["messageId"].as_str().unwrap()).unwrap();
        assert_eq!(msg["metadata"]["nativeToolReceipts"],false);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }

    #[tokio::test]
    async fn awaiting_remote_review_yields_scheduler_slot_without_duplicate_dispatch(){
        let (state,job)=sample();
        let ctx=JobContext::new(state.clone(),"worker".into());
        let first=run(&ctx,&job).await.unwrap_err();
        assert!(first.starts_with("deferred:"));
        assert_eq!(state.store.all("remote_job").len(),1);
        let second=run(&ctx,&job).await.unwrap_err();
        assert!(second.starts_with("deferred:"));
        assert_eq!(state.store.all("remote_job").len(),1);
        let remote="remote:worker";
        state.store.patch_if("remote_job",remote,"status",&json!("queued"),
            &json!({"status":"owner_accepted","workerId":"other-machine",
                "workerReport":{"outcome":"completed","answer":"Owner reviewed",
                    "remoteNativeExecutionId":"real-remote-run"}})).unwrap();
        let result=run(&ctx,&job).await.unwrap();
        assert_eq!(result["remoteJobId"],remote);
        assert_eq!(state.store.all("remote_job").len(),1);
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[test]
    fn foreign_or_unreviewed_remote_results_are_rejected(){
        let (state,job)=sample();
        assert!(owns(&state,&job).is_ok());
        let value=json!({"status":"owner_accepted","sourceJobId":"wrong",
            "teamJobId":"team","sessionId":"child","workerReport":{
                "outcome":"completed","answer":"unsafe","remoteNativeExecutionId":"fake"}});
        assert!(!eligible(&value,&job));
        let mut value=value;value["sourceJobId"]=json!("worker");
        value["status"]=json!("needs_review");
        assert!(!eligible(&value,&job));
        let _=std::fs::remove_dir_all(&state.config.data_dir);
    }
}
