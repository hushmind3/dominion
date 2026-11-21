//! Immutable owner-message and acceptance-contract ingress for a model turn.
//! The requested source, attachment grants and frozen verification checks are
//! bound BEFORE a model or any tool begins to execute.
use super::*;

pub(super) struct TurnSetup{
    pub session:Value,
    pub strategy_candidates:Vec<Value>,
    pub reply_to:Option<String>,
    pub acceptance:Option<(String,Value)>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    state:&AppState,request:&TurnRequest,session_id:&str,
    execution_id:&str,task_id:&str,record_input:bool,
    source_message_id:Option<&str>,
)->Result<TurnSetup,String>{
    let mut session = state
        .store
        .get("session", session_id)
        .ok_or("Session not found")?;
    // A completed SDK job must not leave tool gates, planning or budgets
    // active on the ordinary chat session that it used.
    let policy=crate::engine::execution_scope::active_panel_policy(state,&session);
    session["sdkUiPolicy"]=policy;
    let strategy_candidates = state
        .store
        .get("execution", execution_id)
        .and_then(|execution| execution["strategyCandidates"].as_array().cloned())
        .unwrap_or_default();

    let recorded_source = if record_input {
        let attachments =
            crate::engine::attachments::validate_ids(state, session_id, &request.attachment_ids)?;
        let user_message = state
            .store
            .write_message(&json!({
                "session_id": session_id,"role": "user","content": request.input,
                "agent_id": request.agent_id,"metadata":{"authorKind":"owner","attachments":attachments},
            }))
            .ok_or("사용자 메시지를 저장할 수 없습니다.")?;
        state.bus.message(&user_message);
        crate::engine::episode::attach_source(
            state,
            task_id,
            user_message["id"].as_str().unwrap_or(""),
            &request.input,
        );
        retitle_if_placeholder(state, &session, &request.input);
        user_message["id"].as_str().map(str::to_string)
    } else { None };
    let reply_to = if let Some(id) = source_message_id {
        let source = state
            .store
            .message(id)
            .ok_or("주인의 원문 메시지를 찾을 수 없습니다.")?;
        if source["session_id"] != session_id
            || source["role"] != "user"
            || source["metadata"]["authorKind"] != "owner"
        {
            return Err("그룹 대화 원문과 실행이 일치하지 않습니다.".into());
        }
        Some(id.to_string())
    } else {
        recorded_source.or_else(|| state
            .store
            .messages(session_id)
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["id"].as_str().map(str::to_string)))
    };
    // The acceptance baseline is captured BEFORE any tool runs. For rooms the
    // original owner message is the only source of truth; the synthesized
    // group prompt is never treated as an instruction.
    let acceptance_source = match source_message_id {
        Some(id) => state
            .store
            .message(id)
            .and_then(|message| message["content"].as_str().map(str::to_string)),
        None => Some(task_input(&session,&request.input).to_string()),
    };
    let mut acceptance = if !request.tools_enabled || !request.tools_execution_enabled
        || request.verification_owner.as_deref() == Some("parent") {
        // A parent verifier owns review for delegated child operations;
        // investigating and declining a change must not trigger forced writes.
        // The child still
        // records operations, deterministic evidence and cancellation; it must
        // not spend a second reviewer call on a synthesized derived prompt.
        None
    } else {
        source_message_id
            .and_then(|id| state.store.message(id))
            .and_then(|message| {
                let acceptance = &message["metadata"]["acceptance"];
                Some((acceptance["request"].as_str()?.to_owned(), acceptance["checks"].clone()))
            })
            .or_else(|| acceptance_source.as_deref().and_then(|text| prepare_owner_action(state, text)))
    };
    // An ambiguous literal replacement cannot be frozen into one expected
    // hash. Drop only that unverifiable check so the model still gets a turn;
    // the task then remains observable but is not reported as verified.
    let mut demote_all = false;
    if let Some((_, checks)) = &mut acceptance {
        let before = checks.as_array().map(Vec::len).unwrap_or(0);
        if let Some(items) = checks.as_array_mut() {
            items.retain(|check| {
                !(check["type"] == "file_replace_exact"
                    && check["expectedSha256"].as_str().is_none())
            });
        }
        let after = checks.as_array().map(Vec::len).unwrap_or(0);
        if after < before {
            state.store.audit(
                "acceptance.demoted",
                json!({
                    "executionId": execution_id,
                    "reason": "ambiguous exact replacement: expected hash unavailable",
                    "removedChecks": before - after,
                }),
            );
            demote_all = after == 0;
        }
    }
    if demote_all {
        acceptance = None;
    }
    if let Some((text, checks)) = &acceptance {
        let mut execution = state.store.get("execution", execution_id)
            .ok_or("실행 기록을 찾을 수 없습니다.")?;
        execution["acceptanceText"] = json!(text);
        execution["acceptanceChecks"] = checks.clone();
        execution["acceptanceSourceMessageId"] = json!(reply_to);
        state.store.put("execution", &execution).ok_or("검증 조건을 저장할 수 없습니다.")?;
    }
    // Owner-message identity must be recorded even when no acceptance
    // checks were required. Typed Chat/Room/SDK run events can then all
    // reference the real committed source, not a guessed session ID.
    bind_execution_source(state,execution_id,reply_to.as_deref())?;
    Ok(TurnSetup{session,strategy_candidates,reply_to,acceptance})
}

/// Bind only the committed owner-message ID, once. CAS protects against an
/// asynchronous resume or a mismatched turn overwriting the original source.
fn bind_execution_source(
    state:&AppState,execution_id:&str,source_id:Option<&str>,
)->Result<(),String>{
    let Some(source_id)=source_id.filter(|s|!s.is_empty()) else{return Ok(());};
    let execution=state.store.get("execution",execution_id)
        .ok_or("실행 기록의 출처를 확인할 수 없습니다.")?;
    let current=execution.get("sourceMessageId").cloned().unwrap_or(Value::Null);
    if current==json!(source_id){return Ok(());}
    if !current.is_null(){
        return Err("실행 출처의 원문 메시지가 변경되었습니다.".into());
    }
    if let Some(saved)=state.store.patch_if("execution",execution_id,
        "sourceMessageId",&Value::Null,&json!({"sourceMessageId":source_id})){
        state.bus.record("execution",&saved);
        return Ok(());
    }
    // A concurrent bind of the SAME source is idempotent; a different source
    // is never adopted, even after a restart.
    if state.store.get("execution",execution_id).is_some_and(|execution|
        execution["sourceMessageId"].as_str()==Some(source_id)){
        return Ok(());
    }
    Err("실행 출처 메시지의 소유권이 변경되었습니다.".into())
}

#[cfg(test)]
mod source_binding_tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn chat_and_group_runs_bind_the_exact_owner_source_once(){
        let dir=std::env::temp_dir().join(format!(
            "dominion-source-binding-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let state=AppState::new(config,store,Vault::open(&dir.join("key")));
        state.store.put("execution",&json!({"id":"chat-run","status":"running",
            "sessionId":"owner-session"})).unwrap();
        bind_execution_source(&state,"chat-run",Some("owner-message-1")).unwrap();
        bind_execution_source(&state,"chat-run",Some("owner-message-1")).unwrap();
        assert!(bind_execution_source(&state,"chat-run",
            Some("unrelated-message")).is_err());
        assert_eq!(state.store.get("execution","chat-run").unwrap()
            ["sourceMessageId"],"owner-message-1");
        let typed=state.bus.replay(0,30).into_iter().filter(|event|
            event["type"]=="run_state"&&event["runId"]=="chat-run"
        ).collect::<Vec<_>>();
        assert_eq!(typed.len(),1);
        assert_eq!(typed[0]["sourceMessageId"],"owner-message-1");
        let _=std::fs::remove_dir_all(dir);
    }
}
