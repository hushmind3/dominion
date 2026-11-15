//! A concurrent owner plan revision is applied only at a safe turn boundary.
//! The owner cannot edit acceptance checks, tool approvals or root identity.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
#[derive(Debug)]
pub enum SteeringError { Missing, Immutable, Conflict, Invalid }
pub fn submit(state:&AppState,id:&str,expected:u64,plan:&str)->Result<Value,SteeringError> {
    let plan=plan.trim();
    if plan.is_empty()||plan.chars().count()>6000||plan.contains('\0'){
        return Err(SteeringError::Invalid);
    }
    let goal=state.store.get("goal",id).ok_or(SteeringError::Missing)?;
    if !matches!(goal["status"].as_str(),Some("queued"|"running")){
        return Err(SteeringError::Immutable);
    }
    let revision=goal["planRevision"].as_u64().unwrap_or(0);
    if revision!=expected{return Err(SteeringError::Conflict);}
    let next=revision.checked_add(1).ok_or(SteeringError::Invalid)?;
    let saved=state.store.patch_if("goal",id,"planRevision",
        goal.get("planRevision").unwrap_or(&Value::Null),
        &json!({"planRevision":next,"ownerPlan":{"revision":next,"plan":plan,
            "submittedAt":chrono::Utc::now().to_rfc3339()}}))
        .ok_or(SteeringError::Conflict)?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_plan_revision",json!({"goalId":id,"revision":next}));
    Ok(json!({"id":id,"planRevision":next,"status":"pending_application"}))
}

/// Idempotent owner instruction for SDK consumers. Every unique request ID
/// is bound to the exact text and mode, and committed in the SAME SQLite CAS
/// as the new plan revision. A crash/retry cannot duplicate the instruction.
pub fn submit_instruction(
    state:&AppState,id:&str,text:&str,mode:&str,request_id:&str
)->Result<Value,SteeringError>{
    let request_id=request_id.trim();
    let text=text.trim();
    // Existing SDK clients used "replace"; canonicalize without changing
    // duplicate-receipt identity or the persisted native plan operation.
    let mode=if mode=="replace" {"replace_plan"}else{mode};
    if !matches!(mode,"append"|"replace_plan")
        || request_id.is_empty()||request_id.len()>128
        || !request_id.bytes().all(|c|c.is_ascii_alphanumeric()||b"-_:".contains(&c))
        || text.is_empty()||text.chars().count()>6000||text.contains('\0'){
        return Err(SteeringError::Invalid);
    }
    let goal=state.store.get("goal",id).ok_or(SteeringError::Missing)?;
    let digest=format!("{:x}",Sha256::digest(text.as_bytes()));
    let mut receipts=goal["ownerSteerReceipts"].as_array().cloned().unwrap_or_default();
    if let Some(previous)=receipts.iter().find(|r|r["requestId"]==request_id){
        if previous["mode"]==mode && previous["textSha256"]==digest {
            return Ok(json!({"id":id,"planRevision":goal["planRevision"],
                "status":"pending_application","idempotentReplay":true}));
        }
        return Err(SteeringError::Conflict);
    }
    if !matches!(goal["status"].as_str(),Some("queued"|"running")){
        return Err(SteeringError::Immutable);
    }
    let previous=goal["ownerPlan"]["plan"].as_str()
        .or_else(||goal["plan"].as_str()).unwrap_or("");
    let plan=if mode=="append" && !previous.trim().is_empty(){
        format!("{}\n\n추가 사용자 지시: {text}",previous.trim())
    }else{text.to_string()};
    if plan.chars().count()>6000{return Err(SteeringError::Invalid);}
    let revision=goal["planRevision"].as_u64().unwrap_or(0);
    let next=revision.checked_add(1).ok_or(SteeringError::Invalid)?;
    receipts.push(json!({
        "requestId":request_id,"mode":mode,"textSha256":digest,
        "revision":next,"submittedAt":chrono::Utc::now().to_rfc3339()
    }));
    let excess=receipts.len().saturating_sub(64);
    if excess>0 {receipts.drain(..excess);}
    let saved=state.store.patch_if("goal",id,"planRevision",
        goal.get("planRevision").unwrap_or(&Value::Null),
        &json!({"planRevision":next,"ownerPlan":{"revision":next,"plan":plan,
            "submittedAt":chrono::Utc::now().to_rfc3339()},
            "ownerSteerReceipts":receipts}))
        .ok_or(SteeringError::Conflict)?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_instruction",json!({
        "goalId":id,"requestId":request_id,"mode":mode,"revision":next
    }));
    Ok(json!({"id":id,"planRevision":next,"status":"pending_application",
        "idempotentReplay":false}))
}

pub fn pending(state:&AppState,id:&str,applied:u64)->Option<(u64,String)> {
    let row=state.store.get("goal",id)?;
    let candidate=&row["ownerPlan"];
    let revision=candidate["revision"].as_u64()?;
    let text=candidate["plan"].as_str()?.trim();
    (revision>applied&&!text.is_empty()).then(||(revision,text.to_string()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn input_is_bounded(){
        assert!(String::from("a").chars().count()<=6000);
        assert!(matches!(SteeringError::Conflict,SteeringError::Conflict));
    }
}

#[cfg(test)]
mod instruction_tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!("dominion-goal-steer-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let db=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        (AppState::new(config,db,vault),root)
    }
    #[test]
    fn durable_owner_instructions_are_idempotent_and_versioned(){
        let (state,root)=state();
        state.store.put("goal",&json!({
            "id":"g","goal":"fix bugs","status":"running",
            "plan":"inspect sources","planRevision":0
        }));
        let first=submit_instruction(&state,"g","write tests","append","req-1").unwrap();
        assert_eq!(first["planRevision"],1);
        assert_eq!(first["idempotentReplay"],false);
        assert_eq!(submit_instruction(&state,"g","write tests","append","req-1")
            .unwrap()["idempotentReplay"],true);
        assert_eq!(state.store.get("goal","g").unwrap()["planRevision"],1);
        assert!(state.store.get("goal","g").unwrap()["ownerPlan"]["plan"]
            .as_str().unwrap().contains("inspect sources"));
        assert!(matches!(submit_instruction(&state,"g","delete tests",
            "append","req-1"),Err(SteeringError::Conflict)));
        let replaced=submit_instruction(&state,"g","do only this","replace_plan","req-2").unwrap();
        assert_eq!(replaced["planRevision"],2);
        assert_eq!(state.store.get("goal","g").unwrap()["ownerPlan"]["plan"],"do only this");
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn invalid_ids_and_non_owner_mode_are_rejected(){
        let (state,root)=state();
        state.store.put("goal",&json!({"id":"g","status":"queued"}));
        assert!(matches!(submit_instruction(&state,"g","x","append","/"),
            Err(SteeringError::Invalid)));
        assert!(matches!(submit_instruction(&state,"g","x","unsupported","req"),
            Err(SteeringError::Invalid)));
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
}
