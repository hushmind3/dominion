//! Opt-in concurrent read-only room jobs.
//! Child model sessions are isolated; the shared room transcript is committed
//! in roster order only AFTER all workers have completed. Tool effects and
//! multi-round peer discussions always use the original serial coordinator.
use super::*;
use crate::engine::{chat, memory};
use std::time::Duration;
use tokio::{sync::mpsc,task::JoinSet};

type WorkerResult = Result<(String,Value),String>;
const MAX_CONCURRENT:usize=4;
const POLL:Duration=Duration::from_millis(250);

fn session_id(room_id:&str,source_id:&str,agent_id:&str)->String{
    format!("room-worker:{}",memory::stable_id(&[room_id,source_id,agent_id]))
}
fn stop_workers(state:&AppState,workers:&mut JoinSet<WorkerResult>,sessions:&[String]){
    for id in sessions {state.cancel_session(id);}
    workers.abort_all();
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    context:&JobContext,room:&Value,input:&Value,room_id:&str,room_session:&str,
    source_id:&str,owner_text:&str,task_id:&str,generation:u64,mode:&str,
    roster:&[String],initial:&[String],budget:usize,
    pool:&[(String,String,Option<String>)],token_budget:u64,
)->Result<Value,String>{
    let state=&context.state;
    // The complete set is verified before launching any worker, so failure
    // to create a session cannot leave half the roster running.
    let mut work=Vec::new();
    let mut sessions=Vec::new();
    for agent_id in initial {
        let agent=state.store.get("agent",agent_id)
            .ok_or_else(||format!("참가 에이전트가 삭제되었습니다: {agent_id}"))?;
        if agent["enabled"]==false {
            return Err(format!("비활성 참가자: {agent_id}"));
        }
        let private_id=session_id(room_id,source_id,agent_id);
        if state.store.get("session",&private_id).is_some(){
            return Err("이전 그룹 병렬 워커가 남아 있어 상태 확인이 필요합니다.".into());
        }
        let route=if pool.is_empty(){None}else{
            Some(&pool[pool_slot(roster,agent_id,pool.len())])
        };
        let provider_id=optional(input,"providerId")
            .or_else(||route.map(|p|p.0.clone()))
            .or_else(||optional(&agent,"providerId"));
        let model=optional(input,"model")
            .or_else(||route.map(|p|p.1.clone()))
            .or_else(||optional(&agent,"model"));
        let effort=route.and_then(|p|p.2.clone())
            .or_else(||optional(input,"reasoningEffort"))
            .or_else(||optional(room,"reasoningEffort"))
            .or_else(||optional(&agent,"reasoningEffort"))
            .or_else(||Some(auto_reasoning_effort(owner_text)));
        let codex=provider_id.as_deref().and_then(|id|state.store.get("provider",id))
            .is_some_and(|record|record["kind"]=="codex");
        let reasoning_effort=effort.clone().filter(|s|codex||s!="none");
        let thinking=room["thinking"].as_bool().or_else(||agent["thinking"].as_bool())
            .or_else(||effort.as_deref().map(|s|s!="none"));
        let session=json!({
            "id":private_id,"kind":"room_worker","roomWorkerFor":room_id,
            "title":"그룹 내부 작업","agentId":agent_id,
            "providerId":provider_id,"model":model,
            "jobTokenOwnerId":context.job_id,
            "jobTokenLimit":if token_budget==0 {Value::Null}
                else{json!(token_budget/initial.len() as u64)},
        });
        sessions.push(private_id.clone());
        work.push((agent_id.clone(),private_id,chat::TurnRequest{
            session_id:Some(session["id"].as_str().unwrap_or("").to_string()),
            request_id:None,input:owner_text.to_string(),
            attachment_ids:Vec::new(),agent_id:Some(agent_id.clone()),
            provider_id,model,tools_enabled:false,tools_execution_enabled:false,
            thinking,reasoning_effort,reasoning_language:Some("ko".into()),
            auto_learning:false,
            system_override:Some(format!(
                "You are an independent read-only worker in a group. \
                 Other participants run concurrently; do not assume their work \
                 or claim tool effects. Provide a concise answer for the owner. \
                 Room topic: {}",room["topic"].as_str().unwrap_or("")
            )),task_id:None,parent_task_id:None,task_revision:None,
            verification_owner:Some("parent".into()),
        }));
    }
    for (_,id,_) in &work{
        let session=state.store.get("session",id);
        if session.is_some(){return Err("그룹 병렬 워커 세션이 충돌했습니다.".into());}
    }
    for (agent_id,id,request) in &work{
        let v=json!({"id":id,"kind":"room_worker","roomWorkerFor":room_id,
            "title":"그룹 내부 작업","agentId":agent_id,
            "providerId":request.provider_id,"model":request.model,
            "jobTokenOwnerId":context.job_id,
            "jobTokenLimit":if token_budget==0 {Value::Null}
                else{json!(token_budget/initial.len() as u64)},
        });
        state.store.put_if_absent("session",&v)
            .ok_or("그룹 워커의 독립 세션을 저장하지 못했습니다.")?;
    }
    state.store.audit("room.parallel_started",json!({
        "roomId":room_id,"sourceMessageId":source_id,
        "workerCount":work.len(),"concurrency":MAX_CONCURRENT,
        "toolsEnabled":false,
    }));

    // Restrict concurrency to four live model calls. Adjacent chunks share no
    // turn history, so provider routing and prompt caching remain per-agent.
    let mut completed=std::collections::HashMap::<String,Value>::new();
    for chunk in work.chunks(MAX_CONCURRENT){
        let mut running=JoinSet::<WorkerResult>::new();
        for (agent_id,private_id,request) in chunk.iter().cloned(){
            context.emit(json!({"type":"speaker","roomId":room_id,"agentId":agent_id,
                "step":completed.len()+1,"stepTotal":budget,"parallel":true}));
            let local=state.clone();
            let bus=state.bus.clone();
            let room_name=room_id.to_string();
            let shared_session=room_session.to_string();
            running.spawn(async move {
                let (tx,mut rx)=mpsc::unbounded_channel::<Value>();
                let event_agent=agent_id.clone();
                let event_session=private_id.clone();
                let collector=tokio::spawn(async move{
                    while let Some(event)=rx.recv().await{
                        bus.emit(json!({"type":"room_event","roomId":room_name,
                            "sessionId":shared_session,"agentId":event_agent,
                            "workerSessionId":event_session,"event":event}));
                    }
                });
                let result=chat::run_turn_background(&local,&request,&tx).await;
                drop(tx);
                let _=collector.await;
                let saved=result?
                    .ok_or("그룹 병렬 워커가 빈 메시지를 반환했습니다.")?;
                Ok((agent_id,saved))
            });
        }
        while !running.is_empty(){
            if stopped(context,room_session,generation){
                stop_workers(state,&mut running,&sessions);
                return Ok(finish(context,room_id,room_session,"cancelled",&[],
                    stop_reason(context,room_session,generation)
                        .unwrap_or("session_cancelled"),None));
            }
            if let Some(new_owner)=newer_owner_message(state,room_session,source_id){
                stop_workers(state,&mut running,&sessions);
                return Ok(finish(context,room_id,room_session,"completed",&[],
                    "new_owner_message",Some(&new_owner)));
            }
            let item=tokio::select!{
                result=running.join_next()=>result,
                _=tokio::time::sleep(POLL)=>continue,
            };
            match item{
                Some(Ok(Ok((agent,saved))))=>{
                    if completed.insert(agent,saved).is_some(){
                        stop_workers(state,&mut running,&sessions);
                        return Err("그룹 병렬 워커가 동일 참가자 결과를 중복 생성했습니다.".into());
                    }
                }
                Some(Ok(Err(err)))=>{
                    stop_workers(state,&mut running,&sessions);
                    return Err(err);
                }
                Some(Err(err))=>{
                    stop_workers(state,&mut running,&sessions);
                    return Err(format!("그룹 병렬 워커 실행 실패: {err}"));
                }
                None=>break,
            }
        }
    }
    if stopped(context,room_session,generation){
        return Ok(finish(context,room_id,room_session,"cancelled",&[],
            "session_cancelled",None));
    }
    if let Some(new_owner)=newer_owner_message(state,room_session,source_id){
        return Ok(finish(context,room_id,room_session,"completed",&[],
            "new_owner_message",Some(&new_owner)));
    }
    // Publish in deterministic roster order, not network completion order.
    // Each final message is the original model output (no invented summary).
    let mut outputs=Vec::new();
    for agent_id in initial{
        let result=completed.remove(agent_id)
            .ok_or("병렬 워커 결과가 누락되었습니다.")?;
        let member=state.store.get("agent",agent_id)
            .ok_or("병렬 작업 완료 전에 참가자가 삭제되었습니다.")?;
        let private_id=session_id(room_id,source_id,agent_id);
        let message_id=format!("room-parallel-{}",memory::stable_id(
            &[room_id,source_id,agent_id]
        ));
        let execution_id=result["metadata"]["executionId"].clone();
        let receipt=json!({
            "id":message_id,"session_id":room_session,"role":"assistant",
            "agent_id":agent_id,"content":result["content"],
            "metadata":{"authorKind":"agent","roomId":room_id,
                "replyToMessageId":source_id,"executionId":execution_id,
                "usage":result["metadata"]["usage"],
                "parallelWorkerSessionId":private_id,
                "parallelReadOnly":true},
        });
        let saved=if let Some(existing)=state.store.message(&message_id){
            if existing["session_id"]!=room_session ||
                existing["agent_id"].as_str()!=Some(agent_id) ||
                existing["metadata"]["replyToMessageId"].as_str()!=Some(source_id){
                return Err("병렬 결과 메시지 범위 충돌".into());
            }
            existing
        }else{
            let saved=state.store.write_message(&receipt)
                .ok_or("병렬 워커 결과를 공동 대화에 저장하지 못했습니다.")?;
            state.bus.message(&saved);
            saved
        };
        let execution=execution_id.as_str()
            .and_then(|id|state.store.get("execution",id))
            .unwrap_or(Value::Null);
        outputs.push(json!({
            "agentId":agent_id,"name":member["name"],
            "content":saved["content"],"messageId":saved["id"],
            "executionId":execution_id,"usage":saved["metadata"]["usage"],
            "modelCalls":execution["modelCalls"],
            "toolCallCount":0,"evidenceCount":0,
            "parallelReadOnly":true,
        }));
        // Shared board is durable and idempotent, even for a read-only peer.
        crate::engine::room_workboard::record(state,room_id,room_session,
            source_id,task_id,agent_id,outputs.len(),&saved)?;
        state.bus.emit(json!({"type":"room_event","roomId":room_id,
            "sessionId":room_session,"agentId":agent_id,
            "event":{"type":"assistant_saved","messageId":saved["id"]}}));
        checkpoint(context,room_session,generation,source_id,mode,roster,initial,
            &VecDeque::new(),&outputs,budget,false,None)?;
    }
    state.store.audit("room.parallel_completed",json!({
        "roomId":room_id,"sourceMessageId":source_id,
        "workers":outputs.len(),"toolEffects":0,
    }));
    Ok(finish(context,room_id,room_session,"completed",&outputs,
        "parallel_readonly",None))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn worker_sessions_are_stable_and_owner_scoped(){
        assert_eq!(session_id("r","owner-1","a"),session_id("r","owner-1","a"));
        assert_ne!(session_id("r","owner-1","a"),session_id("r","owner-2","a"));
        assert_ne!(session_id("r","owner-1","a"),session_id("r","owner-1","b"));
    }
}
