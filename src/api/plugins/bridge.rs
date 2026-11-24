//! Host-pull extension protocol v1. External apps do not get the owner API,
//! vault or an ambient SDK token. Every request is immutable and owner-reviewed.
use super::{inspect,owner,valid_id};
use crate::{state::AppState,api::{sdk_runs,response::error}};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::time::Duration;

pub mod auto_chat;
const KIND:&str="plugin_request";
const MAX_INSTRUCTION:usize=16000;
const MAX_ANSWER:usize=64000;
const WORKSPACE_TOOLS:[&str;6]=["workspace_list","workspace_read","workspace_read_range",
    "workspace_write","workspace_replace","workspace_replace_range"];
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Request {request_id:String,instruction:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Approval {confirmed:bool,request_digest:String,mode:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Delivery {confirmed:bool,answer_digest:String}
fn digest(s:&str)->String{format!("{:x}",Sha256::digest(s.as_bytes()))}
fn record_id(plugin:&str,request:&str)->String{format!("{plugin}:{request}")}
fn invalid(s:&str)->Response{error(StatusCode::BAD_REQUEST,s)}
fn conflict(s:&str)->Response{error(StatusCode::CONFLICT,s)}
fn upstream(s:&str)->Response{error(StatusCode::BAD_GATEWAY,s)}
fn missing()->Response{error(StatusCode::NOT_FOUND,"플러그인 작업 요청을 찾을 수 없습니다.")}
fn parse(value:Value)->Result<Request,String>{
    let r:Request=serde_json::from_value(value).map_err(|_|"Invalid extension request schema".to_string())?;
    if !valid_id(&r.request_id)||r.instruction.trim().is_empty()||r.instruction.len()>MAX_INSTRUCTION
        ||r.instruction.chars().any(|c|c.is_control()&&!matches!(c,'\n'|'\r'|'\t')){
        return Err("Invalid extension request ID or instruction".into())
    }
    Ok(r)
}
fn client()->Result<reqwest::Client,String>{
    reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8)).build().map_err(|e|e.to_string())
}
async fn target(state:&AppState,id:&str)->Result<(Value,u16),String>{
    if !valid_id(id){return Err("Invalid plugin identity".into())}
    let scope=owner(state)?;
    let row=inspect(id,&scope).await?.ok_or("Plugin not installed")?;
    if row["running"]!=true||row["network"]!=true||row["requestedPort"].is_null(){
        return Err("플러그인을 실행하고 서비스 포트를 설정하세요.".into())
    }
    let ports=row["ports"].as_array().ok_or("No service port")?;
    if ports.len()!=1{return Err("SDK 연결은 하나의 로컬 서비스 포트가 필요합니다.".into())}
    let port=ports[0].as_u64().and_then(|n|u16::try_from(n).ok()).filter(|n|*n>0)
        .ok_or("Invalid service port")?;
    Ok((row,port))
}
fn binding_matches(record:&Value,row:&Value)->bool{
    record["containerId"].is_string()&&record["containerId"]==row["containerId"]
        &&record["imageId"].is_string()&&record["imageId"]==row["imageId"]
}
fn lookup(state:&AppState,id:&str,request:&str)->Option<Value>{
    if !valid_id(id)||!valid_id(request){return None}
    state.store.get(KIND,&record_id(id,request)).filter(|r|r["pluginId"]==id&&r["requestId"]==request)
}
fn job_id(record:&Value)->String{format!("plugin-{}",digest(record["id"].as_str().unwrap_or_default()))}
pub(super) fn job_input(record:&Value,mode:&str,provider:&str)->Option<Value>{
    let tools=match mode{"chat"=>vec![],"workspace"=>WORKSPACE_TOOLS.to_vec(),_=>return None};
    Some(json!({
        "prompt":record["instruction"],"clientSurface":"native_agent_panel",
        "pluginRequestId":record["id"],"providerId":provider,"toolsEnabled":mode=="workspace",
        "toolsExecutionEnabled":mode=="workspace","allowedTools":tools,
        "allowFallback":false,"requireEvidence":mode=="workspace",
        "limits":{"maxSteps":12,"maxModelCalls":12,"maxToolCalls":24,"maxEstimatedTokens":32000,
            "maxParallelReads":2,"maxResultChars":3000}
    }))
}
fn linked_job(state:&AppState,record:&Value)->Option<Value>{
    state.store.get("job",&job_id(record)).filter(|j|
        j["kind"]=="prompt"&&j["input"]["pluginRequestId"]==record["id"])
}
fn view(state:&AppState,record:&Value)->Value{
    let run=linked_job(state,record).map(|job|sdk_runs::project(state,&job));
    let answer=run.as_ref().filter(|r|r["status"]=="completed")
        .and_then(|r|r["answer"].as_str()).map(|s|s.chars().take(MAX_ANSWER).collect::<String>());
    json!({"pluginId":record["pluginId"],"requestId":record["requestId"],
        "instruction":record["instruction"],"requestDigest":record["requestDigest"],
        "runId":run.as_ref().map(|r|r["id"].clone()),"status":run.as_ref().map(|r|r["status"].clone()).unwrap_or(json!("pending_owner")),
        "answerDigest":answer.as_ref().map(|s|digest(s)),"answer":answer,
        "delivery":record["delivery"].as_str().unwrap_or("not_sent")})
}
fn bridge_auth(state:&AppState,row:&Value,request:reqwest::RequestBuilder)->Result<reqwest::RequestBuilder,String>{
    if row["image"].as_str().is_some_and(super::recipe::voice_tag){
        let id=row["id"].as_str().ok_or("Invalid voice identity")?;
        let token=super::recipe::bridge_token(state,id).ok_or("Voice bridge credential missing")?;
        Ok(request.bearer_auth(token))
    }else{Ok(request)}
}
async fn fetch_record(state:&AppState,id:&str,row:&Value,port:u16)->Result<Option<Value>,String>{
    let request=bridge_auth(state,row,client()?.get(format!("http://127.0.0.1:{port}/dominion/v1/request")))?;
    let response=request.send().await
        .map_err(|e|e.to_string())?;
    if response.status()==StatusCode::NO_CONTENT{return Ok(None)}
    if !response.status().is_success(){return Err("플러그인이 DOMINION 요청 규격에 응답하지 않았습니다.".into())}
    let value=crate::integrations::network::read_bounded_json(response,64*1024,"plugin request").await?;
    let request=parse(value)?;
    let key=record_id(id,&request.request_id);
    let fingerprint=digest(&request.instruction);
    let candidate=json!({"id":key,"pluginId":id,"requestId":request.request_id,
        "instruction":request.instruction,"requestDigest":fingerprint,"containerId":row["containerId"],
        "imageId":row["imageId"],"delivery":"not_sent"});
    let record=state.store.put_if_absent(KIND,&candidate).or_else(||state.store.get(KIND,&key));
    match record{
        Some(r) if r["requestDigest"]==candidate["requestDigest"]&&binding_matches(&r,row)=>Ok(Some(r)),
        Some(_)=>Err("플러그인 요청 ID가 다른 내용 또는 이미지에 재사용되었습니다.".into()),
        None=>Err("플러그인 요청을 저장하지 못했습니다.".into())
    }
}
pub async fn pull(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("다른 플러그인 변경이 진행 중입니다.")};
    let (row,port)=match target(&state,&id).await{Ok(v)=>v,Err(e)=>return upstream(&e)};
    match fetch_record(&state,&id,&row,port).await{
        Ok(record)=>Json(json!({"request":record.as_ref().map(|r|view(&state,r))})).into_response(),
        Err(e)=>upstream(&e)
    }
}
pub async fn status(State(state):State<AppState>,Path((id,request)):Path<(String,String)>)->Response{
    match lookup(&state,&id,&request){Some(r)=>Json(view(&state,&r)).into_response(),None=>missing()}
}
pub async fn approve(State(state):State<AppState>,Path((id,request)):Path<(String,String)>,Json(input):Json<Approval>)->Response{
    if !input.confirmed||!matches!(input.mode.as_str(),"chat"|"workspace"){return invalid("요청 내용과 작업 권한을 확인하세요.")}
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("다른 플러그인 변경이 진행 중입니다.")};
    let Some(r)=lookup(&state,&id,&request)else{return missing()};
    if r["requestDigest"]!=input.request_digest{return conflict("요청 내용이 변경되었습니다.")}
    if let Some(job)=linked_job(&state,&r){
        if job["input"]["toolsEnabled"]!=(input.mode=="workspace"){return conflict("이미 승인한 작업의 권한은 변경할 수 없습니다.")}
        return Json(view(&state,&r)).into_response()
    }
    let (row,_)=match target(&state,&id).await{Ok(v)=>v,Err(e)=>return upstream(&e)};
    if !binding_matches(&r,&row){return conflict("요청을 제출한 컨테이너가 변경되었습니다.")}
    let provider=match sdk_runs::native_panel_provider(&state,None){Ok(p)=>p,Err(e)=>return invalid(&e)};
    let Some(body)=job_input(&r,&input.mode,&provider)else{return invalid("Invalid scope")};
    // Deterministic host-owned job ID: a lost response cannot enqueue the
    // same paid task twice. The canonical job worker remains execution authority.
    if crate::engine::jobs::enqueue_once(&state,&job_id(&r),"prompt",body).is_none(){
        return conflict("작업 저장에 실패했거나 이전 승인과 설정이 다릅니다.")
    }
    state.store.audit("plugin.request.approved",json!({"pluginId":id,"requestId":request,"mode":input.mode}));
    Json(view(&state,&r)).into_response()
}
async fn send_answer(state:&AppState,mut r:Value,row:&Value,port:u16,expected_digest:&str)->Result<Value,String>{
    let current=view(state,&r);
    if current["status"]!="completed"||current["answerDigest"]!=expected_digest{
        return Err("완료된 답변 원문을 다시 확인하세요.".into())
    }
    if r["delivery"]=="sent"{return Ok(current)}
    if !binding_matches(&r,row){return Err("원래 요청한 컨테이너에만 답변을 전달할 수 있습니다.".into())}
    let request=r["requestId"].as_str().ok_or("Invalid request ID")?.to_owned();
    r["delivery"]=json!("delivery_unknown");
    if state.store.put(KIND,&r).is_none(){return Err("전달 기록을 저장하지 못했습니다.".into())}
    let outbound=bridge_auth(state,row,client()?.post(format!("http://127.0.0.1:{port}/dominion/v1/result")))?;
    let response=outbound.json(&json!({"requestId":request,"answer":current["answer"],"answerDigest":expected_digest}))
        .send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){return Err("외부 프로그램이 답변을 받았는지 확인되지 않았습니다.".into())}
    let ack=crate::integrations::network::read_bounded_json(response,8192,"plugin result acknowledgement").await?;
    if ack["requestId"]!=request||ack["answerDigest"]!=expected_digest{return Err("답변 수신 확인이 일치하지 않습니다.".into())}
    r["delivery"]=json!("sent");
    if state.store.put(KIND,&r).is_none(){return Err("수신 확인을 저장하지 못했습니다.".into())}
    Ok(view(state,&r))
}
pub async fn deliver(State(state):State<AppState>,Path((id,request)):Path<(String,String)>,Json(input):Json<Delivery>)->Response{
    if !input.confirmed{return invalid("외부 프로그램에 답변을 보내는 것을 확인하세요.")}
    let _guard=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("다른 플러그인 변경이 진행 중입니다.")};
    let Some(r)=lookup(&state,&id,&request)else{return missing()};
    let (row,port)=match target(&state,&id).await{Ok(v)=>v,Err(e)=>return upstream(&e)};
    match send_answer(&state,r,&row,port,&input.answer_digest).await{
        Ok(v)=>Json(v).into_response(),Err(e)=>upstream(&e)
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn manifest_cannot_grant_tools_or_select_a_host(){
        let id="01234567-89ab-4def-8123-456789abcdef";
        assert!(parse(json!({"requestId":id,"instruction":"hello"})).is_ok());
        assert!(parse(json!({"requestId":id,"instruction":"hello","allowedTools":["shell_exec"]})).is_err());
        assert!(parse(json!({"requestId":id,"instruction":"hello","url":"http://other"})).is_err());
        let record=json!({"id":"plugin:request","instruction":"hello"});
        let chat=job_input(&record,"chat","local").unwrap();
        assert_eq!(chat["toolsEnabled"],false);
        assert_eq!(chat["allowedTools"],json!([]));
        let work=job_input(&record,"workspace","local").unwrap();
        assert_eq!(work["allowedTools"],json!(WORKSPACE_TOOLS));
        assert!(job_input(&record,"admin","local").is_none());
        assert_eq!(job_id(&record),job_id(&record));
        assert_ne!(job_id(&record),job_id(&json!({"id":"different"})));
    }
    #[test]
    fn replacement_cannot_receive_original_response(){
        let r=json!({"containerId":"old","imageId":"image"});
        assert!(binding_matches(&r,&r));
        assert!(!binding_matches(&r,&json!({"containerId":"new","imageId":"image"})));
    }
}
