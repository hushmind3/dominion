//! Versioned host-pull application SDK. No plugin-specific names, host URL,
//! provider credentials, arbitrary tool calls, or executable install scripts.
//! A durable owner grant binds one container/image to a pinned tool catalog.
use super::*;
use std::collections::BTreeSet;
const GRANT:&str="plugin_connection_v2";
const RECORD:&str="plugin_run_v2";
const MAX_CONNECTIONS:usize=4;
const DAILY_LIMIT:u64=100;

fn secret_key(id:&str,image:&str)->String{format!("plugin:bridge:v2:{id}:{image}")}
pub(in crate::api::plugins) fn ensure_token(state:&AppState,id:&str,image:&str)->Result<String,String>{
    let key=secret_key(id,image);
    if let Some(t)=state.read_secret(&key).filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit())){return Ok(t)}
    let t=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
    state.save_secret(&key,&t);
    if state.read_secret(&key).as_deref()!=Some(t.as_str()){return Err("Plugin credential storage failed".into())}
    Ok(t)
}
pub(super) fn token(state:&AppState,row:&Value)->Result<String,String>{
    let id=row["id"].as_str().ok_or("Missing plugin identity")?;
    let image=row["imageId"].as_str().ok_or("Missing plugin image identity")?;
    state.read_secret(&secret_key(id,image)).filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))
        .ok_or("이 플러그인은 공통 SDK 인증정보가 없습니다. 새 버전으로 재설치하세요.".into())
}
fn endpoint(port:u16,path:&str)->String{format!("http://127.0.0.1:{port}/dominion/v2/{path}")}
async fn handshake(state:&AppState,row:&Value,port:u16)->Result<(),String>{
    let res=client()?.get(endpoint(port,"capabilities")).bearer_auth(token(state,row)?).send().await.map_err(|e|e.to_string())?;
    if !res.status().is_success(){return Err("공통 SDK v2 연결을 지원하지 않거나 인증에 실패했습니다.".into())}
    let v=crate::integrations::network::read_bounded_json(res,8192,"plugin capabilities").await?;
    if v["protocol"]!="dominion.plugin"||v["version"]!=2||v["transport"]!="host-pull"||v["events"]!=true{
        return Err("Unsupported plugin SDK protocol".into())
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Change{enabled:bool,confirmed:bool,mode:Option<String>}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Work{request_id:String,instruction:String,kind:String}
fn parse_work(v:Value)->Result<Work,String>{
    let w:Work=serde_json::from_value(v).map_err(|_|"Invalid v2 request schema")?;
    if !valid_id(&w.request_id)||w.instruction.trim().is_empty()||w.instruction.len()>16000
        ||w.instruction.chars().any(|c|c.is_control()&&!matches!(c,'\n'|'\r'|'\t'))
        ||!matches!(w.kind.as_str(),"chat"|"task"){return Err("Invalid v2 request".into())}
    Ok(w)
}
fn view_grant(g:Option<&Value>)->Value{
    match g{None=>json!({"enabled":false}),Some(g)=>json!({"enabled":g["enabled"],"mode":g["mode"],
        "tools":g["tools"],"activeRequest":g["activeRequest"],"usedToday":g["usedToday"],"dailyLimit":DAILY_LIMIT,
        "lastError":g["lastError"],"lastStatus":g["lastStatus"],"lastRunId":g["lastRunId"]})}
}
fn save(state:&AppState,g:&Value)->Result<(),String>{state.store.put(GRANT,g).map(|_|()).ok_or("Plugin connection persistence failed".into())}
pub async fn status(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if !valid_id(&id){return invalid("Invalid plugin ID")}
    Json(view_grant(state.store.get(GRANT,&id).as_ref())).into_response()
}
async fn catalog(state:&AppState,mode:&str)->Result<Vec<String>,String>{
    let mut names=BTreeSet::new();
    match mode{
        "chat"=>{},
        "web"=>{names.extend(["web_search","http_fetch","browser_control"].into_iter().map(str::to_owned));},
        "workspace"=>{names.extend(WORKSPACE_TOOLS.into_iter().map(str::to_owned));},
        "agent"=>{
            for v in crate::engine::tools::definitions_for(state){
                if let Some(n)=v["function"]["name"].as_str(){names.insert(n.to_owned());}
            }
            for v in crate::integrations::registry::native_external_tools(state).await{names.insert(v.name);}
        }
        _=>return Err("Unknown connection scope".into())
    }
    if names.len()>256{return Err("Too many configured tools; reduce the tool catalog".into())}
    Ok(names.into_iter().collect())
}
pub async fn change(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Change>)->Response{
    if !valid_id(&id)||!input.confirmed{return invalid("연결 권한을 확인하세요.")}
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("다른 플러그인 변경이 진행 중입니다. 다시 시도하세요.")};
    let old=state.store.get(GRANT,&id);
    if !input.enabled{
        if let Some(mut g)=old{
            g["enabled"]=json!(false);
            if let Err(e)=save(&state,&g){return upstream(&e)}
            if let Some(run)=g["lastRunId"].as_str(){
                if let Some(job)=state.store.get("job",run).filter(|j|j["input"]["pluginBridgeGrant"]==g["grantId"]){
                    if matches!(job["status"].as_str(),Some("queued"|"running")){
                        let _=sdk_runs::cancel(State(state.clone()),Path(run.to_owned())).await;
                    }
                }
            }
        }
        return Json(view_grant(state.store.get(GRANT,&id).as_ref())).into_response()
    }
    if state.maintenance_mode(){return conflict("유지보수 중입니다.")}
    if state.integrations.plugin_chat_grants.lock().unwrap().contains_key(&id){return conflict("이전 자동 대화 연결을 먼저 해제하세요.")}
    if old.as_ref().is_some_and(|g|g["enabled"]==true){return conflict("권한 변경 전 기존 연결을 먼저 해제하세요.")}
    if state.store.all(GRANT).iter().filter(|g|g["enabled"]==true).count()>=MAX_CONNECTIONS{return conflict("동시 SDK 연결 한도에 도달했습니다.")}
    let mode=input.mode.as_deref().unwrap_or("chat");
    let (row,port)=match target(&state,&id).await{Ok(v)=>v,Err(e)=>return upstream(&e)};
    if let Err(e)=handshake(&state,&row,port).await{return upstream(&e)}
    let tools=match catalog(&state,mode).await{Ok(v)=>v,Err(e)=>return invalid(&e)};
    let g=json!({"id":id,"enabled":true,"grantId":uuid::Uuid::new_v4().to_string(),"containerId":row["containerId"],
        "imageId":row["imageId"],"mode":mode,"tools":tools,"day":chrono::Utc::now().format("%Y-%m-%d").to_string(),
        "usedToday":0,"activeRequest":null});
    if let Err(e)=save(&state,&g){return upstream(&e)}
    state.store.audit("plugin.v2.connected",json!({"pluginId":id,"mode":mode,"toolCount":tools.len()}));
    Json(view_grant(Some(&g))).into_response()
}
pub(crate) fn job_authorized(state:&AppState,job:&Value)->bool{
    if job["input"]["pluginBridgeV2"]!=true{return true}
    let Some(id)=job["input"]["pluginBridgeId"].as_str()else{return false};
    state.store.get(GRANT,id).is_some_and(|g|g["enabled"]==true&&g["grantId"]==job["input"]["pluginBridgeGrant"])
}
fn record_id_v2(id:&str,request:&str)->String{format!("{id}:{request}")}
fn run_id(r:&Value)->String{format!("plugin-v2-{}",digest(r["id"].as_str().unwrap_or_default()))}
fn input(r:&Value,g:&Value,provider:&str)->Value{
    let tools=if r["kind"]=="task"{g["tools"].as_array().cloned().unwrap_or_default()}else{vec![]};
    let enabled=!tools.is_empty();
    json!({"prompt":r["instruction"],"providerId":provider,"clientSurface":"native_agent_panel","pluginRequestId":r["id"],
        "pluginBridgeV2":true,"pluginBridgeId":g["id"],"pluginBridgeGrant":g["grantId"],
        "pluginPrivateConversation":true,"toolsEnabled":enabled,"toolsExecutionEnabled":enabled,"allowedTools":tools,
        "allowFallback":false,"requireEvidence":false,
        "ownerInstructions":"Complete requested actions using the granted tools, not just advice. Never claim a tool ran or an external operation succeeded without its receipt. Ask the owner when authentication or missing information is required. Answer normal conversation without unnecessary tools.",
        "limits":{"maxSteps":if enabled{24}else{4},"maxModelCalls":if enabled{24}else{4},"maxToolCalls":if enabled{40}else{0},
            "maxEstimatedTokens":if enabled{64000}else{8000},"maxParallelReads":2,"maxResultChars":4000}})
}
async fn fetch_work(state:&AppState,row:&Value,port:u16)->Result<Option<Work>,String>{
    let res=client()?.get(endpoint(port,"request")).bearer_auth(token(state,row)?).send().await.map_err(|e|e.to_string())?;
    if res.status()==StatusCode::NO_CONTENT{return Ok(None)}
    if !res.status().is_success(){return Err(format!("Plugin request HTTP {}",res.status()))}
    let raw=crate::integrations::network::read_bounded_json(res,65536,"plugin v2 request").await?;
    parse_work(raw).map(Some)
}
fn event(state:&AppState,job:&Value,r:&Value)->Value{
    let run=sdk_runs::project(state,job);
    let status=run["status"].as_str().unwrap_or("needs_review");
    let terminal=!matches!(status,"queued"|"running"|"awaiting_approval");
    let receipts=run["checkpoint"]["toolReceipts"].as_array().map(|a|a.iter().rev().take(12).cloned().collect::<Vec<_>>()).unwrap_or_default();
    let answer=if status=="completed"{run["answer"].as_str().map(|s|s.chars().take(12000).collect::<String>())}else{None};
    let mut e=json!({"requestId":r["requestId"],"runId":job["id"],"status":status,"terminal":terminal,"answer":answer,
        "error":if terminal&&status!="completed"{Some("작업 결과를 PC 도미니온에서 확인하세요.")}else{None},
        "approvalRequired":status=="awaiting_approval","tools":receipts});
    e["eventId"]=json!(digest(&e.to_string()));e
}
async fn deliver_event(state:&AppState,row:&Value,port:u16,e:&Value)->Result<(),String>{
    let response=client()?.post(endpoint(port,"event")).bearer_auth(token(state,row)?).json(e).send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){return Err(format!("Plugin event HTTP {}",response.status()))}
    let ack=crate::integrations::network::read_bounded_json(response,8192,"plugin v2 acknowledgement").await?;
    if ack["eventId"]!=e["eventId"]||ack["requestId"]!=e["requestId"]{return Err("Plugin event acknowledgement mismatch".into())}
    Ok(())
}
async fn tick(state:&AppState,g:&mut Value)->Result<(),String>{
    let id=g["id"].as_str().ok_or("Missing plugin ID")?.to_owned();
    let (row,port)=target(state,&id).await?;
    if !binding_matches(g,&row){
        g["enabled"]=json!(false);save(state,g)?;
        return Err("설치 이미지 또는 컨테이너가 바뀌었습니다. 새 연결 권한을 확인하세요.".into())
    }
    let today=chrono::Utc::now().format("%Y-%m-%d").to_string();
    if g["day"]!=today{g["day"]=json!(today);g["usedToday"]=json!(0);}
    let mut r=if let Some(key)=g["activeRequest"].as_str(){
        state.store.get(RECORD,key).ok_or("Active request record missing")?
    }else{
        if g["usedToday"].as_u64().unwrap_or(0)>=DAILY_LIMIT{return Err("오늘의 작업 한도에 도달했습니다.".into())}
        let Some(w)=fetch_work(state,&row,port).await?else{return Ok(())};
        let key=record_id_v2(&id,&w.request_id);
        let fingerprint=digest(&format!("{}\n{}",w.kind,w.instruction));
        let candidate=json!({"id":key,"pluginId":id,"requestId":w.request_id,"instruction":w.instruction,"kind":w.kind,
            "fingerprint":fingerprint,"grantId":g["grantId"],"containerId":row["containerId"],"imageId":row["imageId"]});
        let r=state.store.put_if_absent(RECORD,&candidate).or_else(||state.store.get(RECORD,&key)).ok_or("Request persistence failed")?;
        if r["fingerprint"]!=candidate["fingerprint"]||r["grantId"]!=g["grantId"]||!binding_matches(&r,&row){return Err("Request ID was reused with different content or authority".into())}
        if r["settled"]==true{return Ok(())}
        g["activeRequest"]=r["id"].clone();g["usedToday"]=json!(g["usedToday"].as_u64().unwrap_or(0)+1);
        g["lastRunId"]=json!(run_id(&r));save(state,g)?;r
    };
    if r["grantId"]!=g["grantId"]||!binding_matches(&r,&row){return Err("Request authority changed".into())}
    let run=run_id(&r);
    let job=if let Some(j)=state.store.get("job",&run){j}else{
        let provider=sdk_runs::native_panel_provider(state,None)?;
        crate::engine::jobs::enqueue_once(state,&run,"prompt",input(&r,g,&provider)).ok_or("Could not queue plugin job")?
    };
    if job["input"]["pluginBridgeGrant"]!=g["grantId"]||job["input"]["pluginRequestId"]!=r["id"]{return Err("Job identity mismatch".into())}
    let e=event(state,&job,&r);
    g["lastStatus"]=e["status"].clone();g["lastRunId"]=json!(run);
    if r["lastEvent"]!=e["eventId"]{
        deliver_event(state,&row,port,&e).await?;
        r["lastEvent"]=e["eventId"].clone();
        if e["terminal"]==true{r["settled"]=json!(true)}
        state.store.put(RECORD,&r).ok_or("Event acknowledgement persistence failed")?;
    }
    if e["terminal"]==true{g["activeRequest"]=Value::Null}
    Ok(())
}
pub(crate) fn start(state:AppState){
    tokio::spawn(async move{
        let mut timer=tokio::time::interval(Duration::from_secs(3));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop{
            timer.tick().await;
            if state.maintenance_mode(){continue}
            let ids=state.store.all(GRANT).into_iter().filter(|g|g["enabled"]==true).filter_map(|g|g["id"].as_str().map(str::to_owned)).take(MAX_CONNECTIONS).collect::<Vec<_>>();
            for id in ids{
                let Ok(_guard)=state.integrations.plugin_mutations.try_lock()else{continue};
                let Some(mut g)=state.store.get(GRANT,&id).filter(|g|g["enabled"]==true)else{continue};
                g["lastError"]=match tick(&state,&mut g).await{Ok(())=>Value::Null,Err(e)=>json!(e)};
                let _=save(&state,&g);
            }
        }
    });
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn scope_is_host_owned_and_request_cannot_inject_authority(){
        let id="01234567-89ab-4def-8123-456789abcdef";
        assert!(parse_work(json!({"requestId":id,"instruction":"open a website","kind":"task"})).is_ok());
        assert!(parse_work(json!({"requestId":id,"instruction":"x","kind":"task","allowedTools":["shell_exec"]})).is_err());
        let g=json!({"id":"p","grantId":"g","tools":["browser_control","web_search"]});
        let mut r=json!({"id":"r","instruction":"x","kind":"task"});
        let b=input(&r,&g,"provider");assert_eq!(b["allowedTools"],g["tools"]);assert_eq!(b["toolsEnabled"],true);
        r["kind"]=json!("chat");let b=input(&r,&g,"provider");assert_eq!(b["allowedTools"],json!([]));assert_eq!(b["toolsEnabled"],false);
        assert_eq!(b["pluginPrivateConversation"],true);
    }
}
