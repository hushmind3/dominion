use crate::{plugin_host as host,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use std::time::Duration;
use super::response::error;
pub async fn status(State(state):State<AppState>)->Json<Value>{Json(host::status(&state).await)}
pub async fn start(State(state):State<AppState>)->Response{
    let _lease=state.plugin_host_operation.lock().await;
    match host::start(&state,"").await{Ok(())=>Json(host::status(&state).await).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
pub async fn stop(State(state):State<AppState>)->Response{
    let _lease=state.plugin_host_operation.lock().await;
    match host::stop(&state).await{Ok(())=>Json(host::status(&state).await).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
pub async fn plugins(State(state):State<AppState>)->Response{
    if host::status(&state).await["running"]!=true{
        let mut inventory=state.store.get("plugin_inventory","default").unwrap_or_else(||json!({"plugins":[]}));
        if let Some(plugins)=inventory["plugins"].as_array_mut(){for plugin in plugins{plugin["state"]=json!("host-stopped");plugin["enabled"]=json!(false);}}
        return Json(json!({"plugins":inventory["plugins"]})).into_response()
    }
    match host::rpc(&state,"plugins.list",json!({}),None,Duration::from_secs(30)).await{Ok(v)=>Json(v).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Install{package_name:String,version:String}
fn valid_name(name:&str)->bool{
    !name.is_empty()&&name.len()<=200&&name.split('/').all(|part|!part.is_empty()&&part!="."&&part!="..")
        &&name.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'@'|b'/'|b'-'|b'_'|b'.'))
}
pub async fn install(State(state):State<AppState>,Json(input):Json<Install>)->Response{
    if !valid_name(&input.package_name)||!input.version.starts_with(|c:char|c.is_ascii_digit())||!input.version.contains('.')||input.version.len()>80
        ||!input.version.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'-'|b'+')){
        return error(StatusCode::BAD_REQUEST,"정확한 플러그인 이름과 버전이 필요합니다.");
    }
    // A persisted receipt owns installation across page changes. Do not retry an
    // unsettled write merely because a browser disconnected.
    let key=format!("plugin-install:{}",input.package_name);
    if !state.begin_session(&key){return error(StatusCode::CONFLICT,"이 플러그인의 설치가 이미 진행 중입니다.")}
    let guard=state.session_guard(&key);
    let id=uuid::Uuid::new_v4().to_string();
    let row=json!({"id":id,"packageName":input.package_name,"version":input.version,"status":"running","startedAt":chrono::Utc::now().timestamp_millis(),"activities":[],"instanceId":state.instance_id});
    let Some(saved)=state.store.put("plugin_install",&row)else{return error(StatusCode::INTERNAL_SERVER_ERROR,"설치 상태를 저장하지 못했습니다.")};
    let task_state=state.clone();let task_id=id.clone();
    tokio::spawn(async move{let _guard=guard;host::install(task_state,task_id,json!({"source":"clawhub","packageName":input.package_name,"version":input.version,"enable":true})).await;});
    (StatusCode::ACCEPTED,Json(saved)).into_response()
}
pub async fn operation(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match state.store.get("plugin_install",&id){Some(mut row)=>{recover_receipt(&state,&mut row);Json(row).into_response()},None=>error(StatusCode::NOT_FOUND,"설치 기록이 없습니다.")}
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action{method:String,params:Value}
pub async fn action(State(state):State<AppState>,Json(input):Json<Action>)->Response{
    if !matches!(input.method.as_str(),"plugins.setEnabled"|"plugins.uninstall"|"plugins.reload"){
        return error(StatusCode::BAD_REQUEST,"지원하지 않는 플러그인 관리 요청입니다.");
    }
    let _lease=state.plugin_host_operation.lock().await;
    if let Err(e)=host::start(&state,"").await{return error(StatusCode::BAD_GATEWAY,&e)}
    match host::rpc(&state,&input.method,input.params,None,Duration::from_secs(120)).await{
        Ok(value)=>{let _=host::rpc(&state,"plugins.list",json!({}),None,Duration::from_secs(30)).await;state.bus.record("plugin_changed",&value);Json(value).into_response()},Err(e)=>error(StatusCode::BAD_GATEWAY,&e)
    }
}
pub async fn inspect(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match host::rpc(&state,"plugins.inspect",json!({"pluginId":id}),None,Duration::from_secs(30)).await{Ok(value)=>Json(value).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration{config:Value}
pub async fn configuration(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Configuration>)->Response{
    if !valid_name(&id)||["__proto__","constructor","prototype"].contains(&id.as_str())||!input.config.is_object(){return error(StatusCode::BAD_REQUEST,"플러그인 설정 형식이 올바르지 않습니다.")}
    let _lease=state.plugin_host_operation.lock().await;
    let snapshot=match host::rpc(&state,"config.get",json!({}),None,Duration::from_secs(20)).await{Ok(v)=>v,Err(e)=>return error(StatusCode::BAD_GATEWAY,&e)};
    let Some(hash)=snapshot["hash"].as_str()else{return error(StatusCode::CONFLICT,"현재 설정 버전을 확인하지 못했습니다.")};
    let mut entries=serde_json::Map::new();entries.insert(id,json!({"config":input.config}));
    let patch=json!({"plugins":{"entries":entries}});
    match host::rpc(&state,"config.patch",json!({"baseHash":hash,"raw":patch.to_string()}),None,Duration::from_secs(60)).await{
        Ok(_)=>Json(json!({"saved":true})).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)
    }
}
#[cfg(test)]mod tests{use super::*;#[test]fn package_identity_cannot_be_a_shell_or_path(){for name in ["../x","x; touch /tmp/x","https://host/x","x/../y","x\n"]{assert!(!valid_name(name));}assert!(valid_name("@openclaw/browser"));}}

pub async fn read_configuration(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if !valid_name(&id){return error(StatusCode::BAD_REQUEST,"Invalid plugin identity")}
    let snapshot=match host::rpc(&state,"config.get",json!({}),None,Duration::from_secs(20)).await{Ok(v)=>v,Err(e)=>return error(StatusCode::BAD_GATEWAY,&e)};
    let schema=host::rpc(&state,"config.schema.lookup",json!({"path":format!("plugins.entries.{id}.config")}),None,Duration::from_secs(20)).await.unwrap_or(Value::Null);
    Json(json!({"config":snapshot["config"]["plugins"]["entries"][&id]["config"].as_object().cloned().unwrap_or_default(),"schema":schema["schema"]})).into_response()
}

pub async fn operations(State(state):State<AppState>)->Json<Value>{
    let mut rows=state.store.all("plugin_install");
    rows.sort_by_key(|row|std::cmp::Reverse(row["startedAt"].as_i64().unwrap_or(0)));
    rows.truncate(100);
    for row in &mut rows{recover_receipt(&state,row);}
    Json(json!(rows))
}
fn recover_receipt(state:&AppState,row:&mut Value){
    if row["status"]=="running"&&row["instanceId"].as_str()!=Some(&state.instance_id){
        row["status"]=json!("failed");row["error"]=json!("앱이 재시작되어 설치 결과 확인이 필요합니다. 플러그인 상태를 확인하세요.");
    }
}
