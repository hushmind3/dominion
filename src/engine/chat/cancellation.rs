//! Cancellation and timeout supervisor for model and tool turns.
//! This is the only place that drains in-flight operations and seals
//! unknown external effects before the caller can attempt recovery.
use crate::state::AppState;
use serde_json::{json,Value};
use std::time::Duration;

const TOOL_CANCEL_DRAIN_LIMIT: Duration=Duration::from_secs(60);

/// Preserve uncertainty rather than mark an aborted external effect as
/// successfully rolled back. This function is used only when the running
/// dispatch could not supply its own durable completion receipt.
fn mark_unsettled_tools(state:&AppState,execution_id:&str,reason:&str)->Result<usize,String>{
    let mut count=0;
    let operations=state.store.execution_operations(execution_id)?;
    for operation in operations{
        if operation["status"]!="running" {continue;}
        let Some(id)=operation["id"].as_str()else{continue;};
        if let Some(saved)=state.store.patch_if(
            "execution_operation",id,"status",&json!("running"),
            &json!({
                "status":"needs_review","outcomeUnknown":true,
                "effectState":"effect_unknown",
                "phase":"interrupted","reason":reason,
                "finishedAt":chrono::Utc::now().to_rfc3339(),
            })
        ){
            state.bus.record("execution_operation",&saved);
            count+=1;
        }
    }
    if count>0{
        state.store.audit("tool.unsettled_after_cancel",json!({
            "executionId":execution_id,"count":count,"reason":reason,
        }));
    }
    Ok(count)
}

fn has_live_tools(state:&AppState,execution_id:&str)->bool{
    // A transient SQLite read failure must NOT be interpreted as evidence
    // that no tool is live. Fail closed and let the normal drain path run.
    state.store.execution_operations(execution_id).map(|operations|
        operations.iter().any(|op|op["status"]=="running")).unwrap_or(true)
}

/// A top-level select! otherwise drops the entire model/tool future on
/// cancellation, defeating the inner scheduler's drain-on-cancel guarantee.
pub(super) async fn supervise_tool_cancellation<F>(
    state:&AppState,session_id:&str,execution_id:&str,
    generation:u64,turn_limit:Duration,body:F
)->Result<Option<Value>,String>
where F:std::future::Future<Output=Result<Option<Value>,String>>{
    let mut body=std::pin::pin!(body);
    let limit=tokio::time::sleep(turn_limit);
    let mut limit=std::pin::pin!(limit);
    tokio::select!{
        // A completed tool future and a cancellation signal can become
        // ready in the same scheduler tick. Once cancellation is committed,
        // the saved tool receipt may be complete, but the turn must not
        // return a successful answer from the cancelled epoch.
        result=&mut body=>{
            if state.is_cancelled(session_id)
                || state.cancellation_generation(session_id)!=generation {
                Err("실행이 중단되었습니다.".into())
            } else { result }
        },
        _=&mut limit=>{
            match mark_unsettled_tools(state,execution_id,
                "턴 시간 제한 도달: 외부 도구 실행 결과를 확인하지 못했습니다."){
                Ok(_)=>Err("설정한 실행 시간 제한을 초과했습니다.".into()),
                Err(error)=>Err(format!(
                    "실행 시간 제한 이후 도구 결과 저장소를 확인하지 못했습니다: {error}. 수동 확인이 필요합니다.")),
            }
        }
        _=wait_cancelled(state,session_id,generation)=>{
            if !has_live_tools(state,execution_id) {
                return Err("실행이 중단되었습니다.".into());
            }
            state.store.audit("tool.cancellation_draining",json!({
                "executionId":execution_id,"sessionId":session_id,
                "maxWaitSeconds":TOOL_CANCEL_DRAIN_LIMIT.as_secs(),
            }));
            // The normal tool dispatcher keeps executing and records its
            // result while the cancelled epoch stops subsequent calls.
            match tokio::time::timeout(TOOL_CANCEL_DRAIN_LIMIT,&mut body).await{
                Ok(_)=>{
                    state.store.audit("tool.cancellation_drained",json!({
                        "executionId":execution_id,"sessionId":session_id,
                    }));
                    Err("실행이 중단되었습니다.".into())
                }
                Err(_)=>{
                    match mark_unsettled_tools(state,execution_id,
                        "취소 후 도구 결과 회수 제한을 초과했습니다."){
                        Ok(unknown)=>Err(format!(
                            "실행이 중단되었습니다. {unknown}개 도구 결과가 불확실하여 검토가 필요합니다.")),
                        Err(error)=>Err(format!(
                            "실행이 중단되었습니다. 결과 기록 저장소에 접근하지 못했습니다: {error}. 수동 확인이 필요합니다.")),
                    }
                }
            }
        }
    }
}

async fn wait_cancelled(state: &AppState, session_id: &str, generation: u64) {
    // Same-process cancellation is event-driven; only the slower backstop
    // checks the durable epoch for a cancellation written by another process.
    // Register the waiter before inspecting state to avoid missing a wakeup.
    let mut backstop=tokio::time::interval(Duration::from_secs(1));
    backstop.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let notified=state.cancel_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if state.is_cancelled(session_id)
            || state.cancellation_generation(session_id)!=generation {
            return;
        }
        tokio::select! {
            _=&mut notified=>{},
            _=backstop.tick()=>{},
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};

    #[tokio::test]
    async fn durable_cross_process_cancellation_works_without_local_notification() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-cross-cancel-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{
            port:0, workspace:dir.clone(),data_dir:dir.clone(),web_dir:dir.join("web")
        };
        let store=Store::open(&config.db_path()).unwrap();
        let state=AppState::new(config.clone(),store.clone(),
            Vault::open(&config.key_path()));
        // Independent notifier: simulates a second engine process writing the
        // shared durable session epoch with no in-memory wakeup in the waiter.
        let outside=AppState::new(config,store,Vault::open(&state.config.key_path()));
        let epoch=state.cancellation_generation("external-session");
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            outside.cancel_session("external-session");
        });
        tokio::time::timeout(Duration::from_secs(3),
            wait_cancelled(&state,"external-session",epoch))
            .await.expect("persistent epoch fallback must observe cancellation");
        assert!(state.is_cancelled("external-session"));
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn local_cancel_notifies_without_waiting_for_the_persistence_backstop() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-local-cancel-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{
            port:0,workspace:dir.clone(),data_dir:dir.clone(),web_dir:dir.join("web")
        };
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let epoch=state.cancellation_generation("local-session");
        let other=state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            other.cancel_session("local-session");
        });
        tokio::time::timeout(Duration::from_millis(900),
            wait_cancelled(&state,"local-session",epoch))
            .await.expect("local notifier must wake promptly");
        let _=std::fs::remove_dir_all(dir);
    }
}
