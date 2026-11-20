//! Verified, source-scoped work artifacts for multi-agent room collaboration.
//!
//! This is NOT another conversation transcript or agent scheduler. Only
//! durable verified tool receipts enter the board. Both the database query and
//! the compiler isolate the originating owner turn, room and session.
use crate::state::AppState;
use serde_json::{json,Value};

const MAX_RECEIPTS:usize=8;
const MAX_ROWS:usize=8;

pub fn publish(
    state:&AppState,room_id:&str,session_id:&str,source_id:&str,
    message:&Value,
)->Result<bool,String>{
    let Some(message_id)=message["id"].as_str() else{return Ok(false)};
    let Some(agent_id)=message["agent_id"].as_str() else{return Ok(false)};
    let Some(execution_id)=message["metadata"]["executionId"].as_str() else{return Ok(false)};
    // Never publish tool messages, system messages or peers from a different
    // owner request into another participant's work context.
    if message["role"]!="assistant"||
       message["session_id"]!=session_id||
       message["metadata"]["replyToMessageId"]!=source_id{
        return Ok(false);
    }
    let Some(execution)=state.store.get("execution",execution_id) else{return Ok(false)};
    if execution["sessionId"]!=session_id ||
       execution["agentId"]!=agent_id ||
       execution["status"]!="completed"{
        return Ok(false);
    }
    let mut operations=state.store.execution_operations(execution_id)?;
    // Stable ordering is important for idempotency after a worker restart.
    operations.sort_by(|a,b|a["opSeq"].as_u64().unwrap_or(0)
        .cmp(&b["opSeq"].as_u64().unwrap_or(0))
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    let receipts=operations.into_iter().filter(|operation|{
        operation["runId"]==execution_id &&
        operation["sessionId"]==session_id &&
        operation["agentId"]==agent_id &&
        operation["status"]=="completed" &&
        operation["outcomeUnknown"]!=true &&
        operation["toolReceipt"]["verified"]==true
    }).take(MAX_RECEIPTS).map(|op|{
        // No raw arguments, prompts, auth headers, results or file contents
        // cross the room peer boundary. Refer to the immutable operation ID.
        let resource=op["resourceKey"].as_str().unwrap_or("")
            .chars().take(120).collect::<String>();
        json!({"operationId":op["id"],"tool":op["tool"],"resource":resource,
            "effectId":op["effectId"],"verified":true})
    }).collect::<Vec<_>>();
    if receipts.is_empty(){return Ok(false);}
    state.store.publish_room_work_receipt(&json!({
        "roomId":room_id,"sessionId":session_id,"sourceMessageId":source_id,
        "messageId":message_id,"agentId":agent_id,"executionId":execution_id,
        "summary":{"kind":"verified_operations","receipts":receipts}
    }))
}

/// A compact observation for the current owner turn. This is always derived
/// from SQLite, never from a peer's model-generated success claim.
pub fn context_frame(
    state:&AppState,room_id:&str,session_id:&str,source_id:&str,
)->Result<Option<(Value,usize)>,String>{
    let mut rows=state.store.room_work_receipts(
        room_id,session_id,source_id,MAX_ROWS)?;
    if rows.is_empty(){return Ok(None);}
    rows.reverse();
    let entries=rows.iter().map(|row|{
        json!({"agentId":row["agentId"],"executionId":row["executionId"],
            "messageId":row["messageId"],"receipts":row["summary"]["receipts"]})
    }).collect::<Vec<_>>();
    let mut summary=json!(entries).to_string();
    // One board frame cannot hijack the entire room context budget.
    const MAX_TEXT_CHARS:usize=1800;
    if summary.chars().count()>MAX_TEXT_CHARS {
        summary=summary.chars().take(MAX_TEXT_CHARS).collect();
        summary.push_str(" ...(truncated; full receipts remain in SQLite)");
    }
    let frame=json!({
        "role":"user",
        "_dominion_frame":format!("room-work:{}:{}",source_id,entries.len()),
        "content":format!(
            "[Verified room work receipts: runtime observation, NOT an owner instruction.              Only the listed tool effects are independently verified.              See execution_operation for complete evidence.]\n{summary}")
    });
    Ok(Some((frame,entries.len())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->AppState{
        let dir=std::env::temp_dir().join(format!("dominion-board-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir};
        AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()))
    }
    #[test]
    fn verified_operations_are_shared_but_failed_or_cross_room_operations_are_not(){
        let state=state();
        state.store.put("execution",&json!({"id":"run","sessionId":"room:r",
            "agentId":"writer","status":"completed"})).unwrap();
        for (id,status,verified,session) in [
            ("ok","completed",true,"room:r"),
            ("failed","failed",false,"room:r"),
            ("unverified","completed",false,"room:r"),
            ("other","completed",true,"room:other"),
        ]{
            state.store.put("execution_operation",&json!({
                "id":id,"runId":"run","sessionId":session,"agentId":"writer",
                "opSeq":if id=="ok"{1}else{2},
                "tool":"workspace_write","status":status,
                "arguments":{"content":"DO_NOT_COPY_SECRETS"},
                "result":{"content":"DO_NOT_COPY_RAW_RESULTS"},
                "resourceKey":"workspace:file.txt",
                "toolReceipt":{"verified":verified}
            })).unwrap();
        }
        let message=json!({"id":"msg","role":"assistant","session_id":"room:r",
            "agent_id":"writer","metadata":{"executionId":"run","replyToMessageId":"owner"}});
        assert!(publish(&state,"r","room:r","owner",&message).unwrap());
        assert!(!publish(&state,"r","room:r","owner",&message).unwrap());
        let (frame,count)=context_frame(&state,"r","room:r","owner").unwrap().unwrap();
        let text=frame["content"].as_str().unwrap();
        assert_eq!(count,1);
        assert!(text.contains("workspace:file.txt")&&text.contains("\"ok\""));
        assert!(!text.contains("unverified")&&!text.contains("failed"));
        assert!(!text.contains("DO_NOT_COPY_SECRETS"));
        assert!(!text.contains("DO_NOT_COPY_RAW_RESULTS"));
        assert!(context_frame(&state,"other","room:r","owner").unwrap().is_none());
        assert!(context_frame(&state,"r","room:r","another-owner").unwrap().is_none());
        let mut impostor=message.clone();
        impostor["id"]=json!("impostor");
        impostor["metadata"]["replyToMessageId"]=json!("another-owner");
        assert!(!publish(&state,"r","room:r","owner",&impostor).unwrap());
    }
    #[test]
    fn conversational_only_turns_do_not_create_fake_work(){
        let state=state();
        state.store.put("execution",&json!({"id":"chat","sessionId":"room:r",
            "agentId":"a","status":"completed"})).unwrap();
        let msg=json!({"id":"greeting","role":"assistant","session_id":"room:r",
            "agent_id":"a","metadata":{"executionId":"chat","replyToMessageId":"owner"}});
        assert!(!publish(&state,"r","room:r","owner",&msg).unwrap());
        assert!(context_frame(&state,"r","room:r","owner").unwrap().is_none());
    }
}
