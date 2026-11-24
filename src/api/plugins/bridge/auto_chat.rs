//! Short-lived, explicitly granted conversation only. Grant is memory-only,
//! tied to one exact container/image, never includes shared memories or tools.
use super::*;
use axum::extract::State;
use std::time::{Instant,Duration};
const TURN_LIMIT:u64=20;
const SESSION_LIMIT:u64=1800;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change{enabled:bool,confirmed:bool}

fn grant_view(grant:Option<&Value>)->Value{
    let now=chrono::Utc::now().timestamp();
    let active=grant.is_some_and(|g|g["expiresAt"].as_i64().is_some_and(|end|end>now));
    json!({"enabled":active,"expiresAt":grant.map(|g|g["expiresAt"].clone()),
        "remaining":grant.map(|g|g["remaining"].clone()).unwrap_or(json!(0)),
        "lastError":grant.and_then(|g|g["lastError"].as_str()),
        "activeRequest":grant.map(|g|g["activeRequest"].clone())})
}
fn read_grant(state:&AppState,id:&str)->Option<Value>{
    state.integrations.plugin_chat_grants.lock().unwrap().get(id).cloned()
}
fn write_grant(state:&AppState,id:&str,grant:Value){
    state.integrations.plugin_chat_grants.lock().unwrap().insert(id.to_owned(),grant);
}
pub async fn status(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if !valid_id(&id){return invalid("Invalid plugin ID")}
    Json(grant_view(read_grant(&state,&id).as_ref())).into_response()
}
pub async fn change(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Change>)->Response{
    if !valid_id(&id){return invalid("Invalid plugin ID")}
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("플러그인 처리 중입니다. 잠시 후 다시 시도하세요.")};
    if !input.enabled{
        state.integrations.plugin_chat_grants.lock().unwrap().remove(&id);
        return Json(grant_view(None)).into_response()
    }
    if !input.confirmed{return invalid("자동 대화와 외부 답변 전달을 승인하세요.")}
    let (row,_)=match target(&state,&id).await{Ok(v)=>v,Err(e)=>return upstream(&e)};
    let at_limit={let grants=state.integrations.plugin_chat_grants.lock().unwrap();grants.len()>=4&&!grants.contains_key(&id)};
    if at_limit{
        return conflict("자동 대화 연결 한도에 도달했습니다.")
    }
    let grant=json!({"grantId":uuid::Uuid::new_v4().to_string(),"containerId":row["containerId"],"imageId":row["imageId"],
        "expiresAt":chrono::Utc::now().timestamp()+SESSION_LIMIT as i64,"remaining":TURN_LIMIT,"activeRequest":null});
    write_grant(&state,&id,grant.clone());
    state.store.audit("plugin.auto_chat.granted",json!({"pluginId":id,"turnLimit":TURN_LIMIT,"seconds":SESSION_LIMIT}));
    Json(grant_view(Some(&grant))).into_response()
}
fn auto_input(r:&Value,grant:&Value,provider:&str)->Option<Value>{
    let mut body=job_input(r,"chat",provider)?;
    body["pluginPrivateConversation"]=json!(true);
    body["pluginAutoGrantId"]=grant["grantId"].clone();
    body["limits"]["maxModelCalls"]=json!(3);
    body["limits"]["maxSteps"]=json!(3);
    body["limits"]["maxEstimatedTokens"]=json!(8000);
    Some(body)
}
async fn tick(state:&AppState,id:&str,grant:&mut Value)->Result<(),String>{
    let (row,port)=target(state,id).await?;
    if !binding_matches(grant,&row){return Err("실행 이미지가 바뀌었습니다. 자동 대화를 다시 승인하세요.".into())}
    let now=chrono::Utc::now().timestamp();
    if grant["expiresAt"].as_i64().unwrap_or(0)<=now{return Ok(())}
    let record=if let Some(request)=grant["activeRequest"].as_str(){
        lookup(state,id,request)
    }else{
        if grant["remaining"].as_u64().unwrap_or(0)==0{return Ok(())}
        fetch_record(state,id,&row,port).await?
    };
    let Some(r)=record else{return Ok(())};
    if grant["expiresAt"].as_i64().unwrap_or(0)<=chrono::Utc::now().timestamp(){return Ok(())}
    if !binding_matches(&r,&row){return Err("요청 출처가 변경되었습니다.".into())}
    if let Some(job)=linked_job(state,&r){
        // Never send an answer approved manually or under an earlier grant.
        if job["input"]["pluginAutoGrantId"]!=grant["grantId"]{
            return Err("이 요청은 수동 승인 작업입니다. 직접 결과를 확인하세요.".into())
        }
        let v=view(state,&r);
        if v["status"]=="completed"{
            if r["delivery"]=="delivery_unknown"{return Err("이전 답변 전달이 불확실합니다. 수동 확인이 필요합니다.".into())}
            let answer=v["answerDigest"].as_str().ok_or("No completed answer")?;
            send_answer(state,r.clone(),&row,port,answer).await?;
            grant["activeRequest"]=Value::Null;
        }else if !matches!(v["status"].as_str(),Some("queued"|"running")){
            return Err(format!("자동 대화 작업 상태: {}. 작업 화면에서 확인하세요.",v["status"]))
        }
        return Ok(())
    }
    let remaining=grant["remaining"].as_u64().unwrap_or(0);
    if remaining==0{return Ok(())}
    let provider=sdk_runs::native_panel_provider(state,None)?;
    let body=auto_input(&r,grant,&provider).ok_or("Invalid conversation scope")?;
    let job=crate::engine::jobs::enqueue_once(state,&job_id(&r),"prompt",body)
        .ok_or("자동 대화 작업을 저장하지 못했습니다.")?;
    if job["input"]["pluginAutoGrantId"]!=grant["grantId"]{return Err("이미 다른 승인으로 실행된 요청입니다.".into())}
    grant["activeRequest"]=r["requestId"].clone();
    grant["remaining"]=json!(remaining-1);
    Ok(())
}
/// No new server/port. Grants vanish on app restart and can be revoked by owner.
/// A failing/ambiguous response disables automatic execution, never retries a
/// potentially delivered answer silently.
pub(crate) fn start(state:AppState){
    tokio::spawn(async move{
        let mut timer=tokio::time::interval(Duration::from_secs(3));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop{
            timer.tick().await;
            if state.maintenance_mode(){continue}
            let now=chrono::Utc::now().timestamp();
            let ids={
                let mut grants=state.integrations.plugin_chat_grants.lock().unwrap();
                grants.retain(|_,g|g["expiresAt"].as_i64().is_some_and(|end|end>now));
                grants.iter().filter(|(_,g)|g["lastError"].is_null()
                    &&(g["remaining"].as_u64().unwrap_or(0)>0||g["activeRequest"].is_string()))
                    .map(|(id,_)|id.clone()).collect::<Vec<_>>()
            };
            for id in ids{
                let Ok(_guard)=state.integrations.plugin_mutations.try_lock()else{continue};
                let Some(mut grant)=read_grant(&state,&id)else{continue};
                let started=Instant::now();
                if let Err(e)=tick(&state,&id,&mut grant).await{
                    grant["lastError"]=json!(e);
                    state.store.audit("plugin.auto_chat.paused",json!({"pluginId":id,"elapsedMs":started.elapsed().as_millis()}));
                }
                write_grant(&state,&id,grant);
            }
        }
    });
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn auto_chat_has_no_memory_tools_or_fallback_and_bounded_budget(){
        let body=auto_input(&json!({"id":"x","instruction":"hello"}),&json!({"grantId":"g"}),"p").unwrap();
        assert_eq!(body["toolsEnabled"],false);
        assert_eq!(body["allowedTools"],json!([]));
        assert_eq!(body["pluginPrivateConversation"],true);
        assert_eq!(body["allowFallback"],false);
        assert_eq!(body["limits"]["maxModelCalls"],3);
    }
}
