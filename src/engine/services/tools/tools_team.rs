//! Managed Lead/Worker execution over the existing Rust job and tool gateway.
//! No second model loop: every teammate is a normal durable delegated job.
//! A stopped parent turn can later resume the SAME team by ID; uncertain
//! launches are fenced instead of implicitly spawning duplicate workers.
use super::*;
use serde_json::{json,Value};
use std::time::{Duration,Instant};

const MAX_TEAM_NODES:usize=8;
const MAX_READERS:usize=4;
const UNKNOWN_LAUNCH_MS:i64=30_000;

fn task_id(team_id:&str,node_id:&str)->String{
    format!("managed-team-node-{}",crate::engine::memory::stable_id(&[team_id,node_id]))
}
fn live_job(status:&str)->bool{
    matches!(status,"queued"|"running")
}
fn finished_job(status:&str)->bool{
    matches!(status,"completed"|"failed"|"cancelled"|"needs_review"|"interrupted")
}
fn plan(nodes:&[Value])->Result<Vec<Vec<usize>>,String>{
    if nodes.is_empty()||nodes.len()>MAX_TEAM_NODES{
        return Err("Team requires 1..8 agent tasks".into());
    }
    let mut converted=Vec::with_capacity(nodes.len());
    for node in nodes{
        let id=node["id"].as_str().filter(|s|!s.trim().is_empty()
            && s.len()<=48 && s.chars().all(|c|c.is_ascii_alphanumeric() || c=='-' || c=='_'))
            .ok_or("Every team task needs a short stable ASCII id")?;
        let agent=node["agentId"].as_str().filter(|s|!s.trim().is_empty())
            .ok_or("Every team task requires an agentId")?;
        let prompt=node["prompt"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=4000)
            .ok_or("Every team task requires a 1..4000 byte prompt")?;
        let _=(agent,prompt);
        if node["dependsOn"].is_null(){}else if !node["dependsOn"].is_array(){
            return Err(format!("{id}: dependsOn must be an array"));
        }
        if node.get("readOnly").is_some()&&!node["readOnly"].is_boolean(){
            return Err(format!("{id}: readOnly must be boolean"));
        }
        converted.push(json!({
            "id":id,"dependsOn":node["dependsOn"],
            "type":if node["readOnly"]==false {"delegate"} else {"observe"}
        }));
    }
    let scheduled=dominion_agent_loop::task_team::schedule(&converted)?;
    Ok(scheduled.waves.into_iter().flat_map(|wave|
        wave.chunks(MAX_READERS).map(|chunk|chunk.to_vec()).collect::<Vec<_>>()
    ).collect())
}
fn read_team(ctx:&ToolContext<'_>,id:&str)->Result<Value,String>{
    let record=ctx.store.get("managed_team",id)
        .ok_or("Managed team not found")?;
    if record["leadSessionId"]!=ctx.session_id{
        return Err("Managed team belongs to another lead session".into());
    }
    Ok(record)
}
fn member(ctx:&ToolContext<'_>,team_id:&str,node:&Value)->Result<Value,String>{
    let id=task_id(team_id,node["id"].as_str().ok_or("Missing team node ID")?);
    let saved=ctx.store.get("managed_team_task",&id)
        .ok_or("Managed team member not committed")?;
    if saved["teamId"]!=team_id || saved["nodeId"]!=node["id"]
        || saved["agentId"]!=node["agentId"]
        || saved["prompt"]!=node["prompt"]
        || saved["dependsOn"]!=node["dependsOn"]
        || saved["readOnly"]!=json!(node["readOnly"]!=false){
        return Err("Managed team task identity or plan changed after commit".into());
    }
    Ok(saved)
}
fn summary(ctx:&ToolContext<'_>,team:&Value)->Result<Value,String>{
    let id=team["id"].as_str().ok_or("Team ID missing")?;
    let nodes=team["tasks"].as_array().ok_or("Team nodes missing")?;
    let mut results=Vec::with_capacity(nodes.len());
    let mut complete=true;
    let mut blocked=false;
    let mut verified=true;
    for node in nodes{
        let row=member(ctx,id,node)?;
        let status=row["status"].as_str().unwrap_or("unknown");
        if status!="completed"{complete=false;verified=false;}
        if matches!(status,"failed"|"needs_review"|"cancelled"|"interrupted"){blocked=true;}
        verified&=row["verified"]==true;
        results.push(json!({
            "id":row["nodeId"],"agentId":row["agentId"],
            "readOnly":row["readOnly"],"dependsOn":row["dependsOn"],
            "status":status,"jobId":row["jobId"],
            "sessionId":row["workerSessionId"],
            "verified":row["verified"]==true,
            "evidence":row["resultEvidence"],
            "error":row["error"],
        }));
    }
    Ok(json!({
        "teamId":id,"leadSessionId":team["leadSessionId"],
        "status":if blocked {"needs_review"}else if complete {"completed"}else{"running"},
        "evidenceVerified":complete && verified,
        "taskCount":nodes.len(),"tasks":results,
        "hint":if blocked {"Review failed or uncertain workers; do not automatically replay effects"}
            else if complete {"Use task_result(jobId) for full native operation evidence"}
            else {"Resume safely with team_execute({teamId}) or poll team_status({teamId})"},
    }))
}
/// Observe terminal job state from the SAME session-bound job record. This is
/// not an assistant-authored success assertion.
fn reconcile(ctx:&ToolContext<'_>,team_id:&str,node:&Value)->Result<Value,String>{
    let row=member(ctx,team_id,node)?;
    let id=row["id"].as_str().ok_or("Team member ID missing")?;
    let status=row["status"].as_str().unwrap_or("");
    if status=="launching"{
        // A crashed launcher may have queued a job before persisting its ID.
        // Recover the uniquely tagged child, not a guessed duplicate.
        let matches=ctx.store.all("job").into_iter().filter(|job|
            job["input"]["teamId"]==team_id
            &&job["input"]["teamNodeId"]==node["id"]
            &&job["input"]["parentSessionId"]==ctx.session_id
        ).collect::<Vec<_>>();
        if matches.len()==1{
            let job=&matches[0];
            let updated=ctx.store.patch_if("managed_team_task",id,"status",
                &json!("launching"),&json!({
                    "status":"running","jobId":job["id"],
                    "workerSessionId":job["input"]["sessionId"],
                }));
            return Ok(updated.unwrap_or_else(||row.clone()));
        }
        let launched=row["launchStartedMs"].as_i64().unwrap_or(0);
        if matches.len()>1 || chrono::Utc::now().timestamp_millis()-launched>UNKNOWN_LAUNCH_MS{
            let reason=if matches.len()>1{"ambiguous duplicate worker launches"}
                else{"worker launch outcome unknown; explicit review required"};
            let updated=ctx.store.patch_if("managed_team_task",id,"status",
                &json!("launching"),&json!({"status":"needs_review","error":reason}));
            return Ok(updated.unwrap_or(row));
        }
        return Ok(row);
    }
    if status!="running"{return Ok(row);}
    let job_id=row["jobId"].as_str().ok_or("Running team node missing job ID")?;
    let job=ctx.store.get("job",job_id).ok_or("Team worker job disappeared")?;
    let observed=job["status"].as_str().unwrap_or("");
    if !finished_job(observed){return Ok(row);}
    let successful=observed=="completed";
    // A complete job is an observed completion. It is NOT necessarily a
    // verified artifact; that stronger claim requires native verification.
    let verified=successful && job["result"]["verified"]==true;
    let update=json!({
        "status":if successful{"completed"}else{"needs_review"},
        "jobStatus":observed,
        "verified":verified,
        "resultEvidence":job["result"]["verification"],
        "error":job["error"],
        "settledAt":chrono::Utc::now().to_rfc3339(),
    });
    Ok(ctx.store.patch_if("managed_team_task",id,"status",
        &json!("running"),&update).unwrap_or(row))
}
async fn launch(ctx:&ToolContext<'_>,team_id:&str,node:&Value)->Result<Value,String>{
    let current=member(ctx,team_id,node)?;
    if current["status"]!="assigned"{return Ok(current);}
    let id=current["id"].as_str().ok_or("Team task ID missing")?;
    let claim=ctx.store.patch_if("managed_team_task",id,"status",
        &json!("assigned"),&json!({"status":"launching",
            "launchStartedMs":chrono::Utc::now().timestamp_millis()}));
    let Some(claim)=claim else{return member(ctx,team_id,node);};
    ctx.state.bus.record("managed_team_task",&claim);
    let delegated=delegate(ctx,&json!({
        "agentId":node["agentId"],"prompt":node["prompt"],
        "readOnly":node["readOnly"]!=false,"wait":false,
        "teamId":team_id,"teamNodeId":node["id"],
    })).await;
    match delegated{
        Ok(started)=>{
            let job_id=started["jobId"].as_str().ok_or("Delegation returned no job ID")?;
            let updated=ctx.store.patch_if("managed_team_task",id,"status",
                &json!("launching"),&json!({"status":"running",
                    "jobId":job_id,"workerSessionId":started["sessionId"]}))
                .ok_or("Team worker launch lost its claim")?;
            ctx.state.bus.record("managed_team_task",&updated);
            Ok(updated)
        }
        Err(reason)=>{
            let updated=ctx.store.patch_if("managed_team_task",id,"status",
                &json!("launching"),&json!({
                    "status":"needs_review","error":reason.chars().take(360).collect::<String>()
                }));
            if let Some(updated)=updated{
                ctx.state.bus.record("managed_team_task",&updated);
            }
            Err(reason)
        }
    }
}
fn create(ctx:&ToolContext<'_>,args:&Value,run_id:&str)->Result<Value,String>{
    let tasks=args["tasks"].as_array().ok_or("tasks array required")?;
    let waves=plan(tasks)?;
    if delegation_depth(ctx.store,ctx.session_id)>=MAX_DELEGATION_DEPTH{
        return Err("Maximum delegated agent depth reached".into());
    }
    // All members and scopes must be valid BEFORE any persisted side effects.
    for task in tasks {
        let agent=task["agentId"].as_str().ok_or("agentId required")?;
        let record=ctx.store.get("agent",agent).ok_or("Unknown team worker")?;
        if record["enabled"]==false || record["disabled"]==true{
            return Err(format!("Team worker disabled: {agent}"));
        }
    }
    let canonical=serde_json::to_string(tasks).map_err(|e|e.to_string())?;
    let team_id=format!("managed-team-{}",crate::engine::memory::stable_id(&[
        ctx.session_id,run_id,&canonical,
    ]));
    let created=json!({"id":team_id,"kind":"managed_agent_team",
        "leadSessionId":ctx.session_id,"leadAgentId":ctx.agent_id,
        "originRunId":run_id,"tasks":tasks,"waves":waves,
        "status":"running"});
    let team=ctx.store.put_if_absent("managed_team",&created)
        .unwrap_or_else(||ctx.store.get("managed_team",&team_id).unwrap_or(Value::Null));
    if team["leadSessionId"]!=ctx.session_id||team["originRunId"]!=run_id
        ||team["tasks"]!=json!(tasks){
        return Err("Managed team identity collision on replay".into());
    }
    for node in tasks {
        let member_id=task_id(&team_id,node["id"].as_str().unwrap_or(""));
        let value=json!({
            "id":member_id,"teamId":team_id,"leadSessionId":ctx.session_id,
            "nodeId":node["id"],"agentId":node["agentId"],
            "prompt":node["prompt"],"dependsOn":node["dependsOn"],
            "readOnly":node["readOnly"]!=false,
            "status":"assigned",
        });
        let saved=ctx.store.put_if_absent("managed_team_task",&value);
        if let Some(saved)=saved{ctx.state.bus.record("managed_team_task",&saved);}
        member(ctx,&team_id,node)?;
    }
    Ok(team)
}
/// Resume an already committed DAG by its ID, without resubmitting the plan.
/// New calls and restarts both refer to the same immutable source plan.
pub(super) async fn execute(
    ctx:&ToolContext<'_>,args:&Value,run_id:&str,
)->Result<Value,String>{
    if args["teamId"].is_string() && args["tasks"].is_array(){
        return Err("Use either a new tasks plan or an existing teamId".into());
    }
    let team=if let Some(id)=args["teamId"].as_str(){
        read_team(ctx,id)?
    }else{create(ctx,args,run_id)?};
    let team_id=team["id"].as_str().ok_or("Team ID missing")?;
    let nodes=team["tasks"].as_array().ok_or("Team DAG missing")?;
    let waves=team["waves"].as_array().ok_or("Team schedule missing")?;
    let wait=args["wait"].as_bool().unwrap_or(true);
    let max_secs=args["timeoutSeconds"].as_u64().unwrap_or(120).clamp(5,300);
    let deadline=Instant::now()+Duration::from_secs(max_secs);
    for wave in waves{
        let indices=wave.as_array().ok_or("Invalid team wave")?;
        // A writer gets an exclusive wave. A read-only wave can launch
        // multiple real job workers BEFORE awaiting their results.
        loop {
            if ctx.state.is_cancelled(ctx.session_id){
                return Err(format!("Lead cancelled; durable team {team_id} remains inspectable"));
            }
            let mut all_done=true;
            for index in indices{
                let i=index.as_u64().ok_or("Invalid team node index")? as usize;
                let node=nodes.get(i).ok_or("Team node index out of range")?;
                let row=reconcile(ctx,team_id,node)?;
                match row["status"].as_str().unwrap_or(""){
                    "completed"=>{},
                    "needs_review"|"failed"|"cancelled"|"interrupted"=>{
                        return summary(ctx,&team);
                    }
                    "assigned"=>{
                        let _=launch(ctx,team_id,node).await?;
                        all_done=false;
                    }
                    "running"|"launching"=>all_done=false,
                    _=>return Err("Unknown team task status".into()),
                }
            }
            if all_done {break;}
            if !wait || Instant::now()>=deadline{return summary(ctx,&team);}
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    let result=summary(ctx,&team)?;
    if result["status"]=="completed"{
        if let Some(saved)=ctx.store.patch_if("managed_team",team_id,
            "status",&json!("running"),&json!({
                "status":"completed","evidenceVerified":result["evidenceVerified"],
                "completedAt":chrono::Utc::now().to_rfc3339(),
            })){
            ctx.state.bus.record("managed_team",&saved);
        }
    }
    Ok(result)
}
pub(super) fn status(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let id=args["teamId"].as_str().filter(|s|!s.is_empty()).ok_or("teamId required")?;
    let team=read_team(ctx,id)?;
    // This call never starts work or implicitly commits an observed job as
    // success. Resume with team_execute when new dependencies become ready.
    summary(ctx,&team)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn lead_plan_runs_independent_research_before_a_single_writer(){
        let tasks=vec![
            json!({"id":"research_a","agentId":"a","prompt":"Collect sources"}),
            json!({"id":"research_b","agentId":"b","prompt":"Inspect tests"}),
            json!({"id":"implement","agentId":"coder","prompt":"Implement","readOnly":false,
                "dependsOn":["research_a","research_b"]}),
            json!({"id":"verify","agentId":"reviewer","prompt":"Review",
                "dependsOn":["implement"]}),
        ];
        assert_eq!(plan(&tasks).unwrap(),vec![vec![0,1],vec![2],vec![3]]);
        assert!(plan(&[
            json!({"id":"a","agentId":"a","prompt":"x","dependsOn":["b"]}),
            json!({"id":"b","agentId":"b","prompt":"y","dependsOn":["a"]})
        ]).is_err());
        assert!(plan(&[
            json!({"id":"a","agentId":"a","prompt":"x","dependsOn":["missing"]})
        ]).is_err());
    }
    #[test]
    fn writer_barriers_apply_even_without_model_supplied_dependencies(){
        let tasks=vec![
            json!({"id":"read","agentId":"a","prompt":"x"}),
            json!({"id":"write","agentId":"b","prompt":"y","readOnly":false}),
            json!({"id":"read_after","agentId":"c","prompt":"z"}),
        ];
        assert_eq!(plan(&tasks).unwrap(),vec![vec![0],vec![1],vec![2]]);
        assert!(plan(&[json!({"id":"bad/id","agentId":"a","prompt":"x"})]).is_err());
    }
    #[test]
    fn no_more_than_four_read_only_workers_share_a_wave(){
        let tasks=(0..8).map(|i|json!({"id":format!("r{i}"),
            "agentId":"a","prompt":"Research"})).collect::<Vec<_>>();
        assert_eq!(plan(&tasks).unwrap(),vec![vec![0,1,2,3],vec![4,5,6,7]]);
        assert!(live_job("running"));
        assert!(finished_job("completed"));
        assert!(!finished_job("running"));
    }
}
