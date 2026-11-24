//! AI discovery and durable owner-reviewed plugin proposals.
//! Tool calls cannot execute a container mutation, even under auto-approval.
use super::{owner,inventory,inspect,valid_id,valid_image,Install,package,recipe,mutate_checked};
use crate::{state::AppState,api::response::error};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
const KIND:&str="plugin_proposal";
#[derive(Deserialize,Serialize)]
#[serde(tag="action",rename_all="snake_case",deny_unknown_fields)]
enum Proposal{
    InstallImage{name:String,image:String,network:bool,port:Option<u16>},
    InstallPackage{name:String,spec:package::Spec,network:bool,port:Option<u16>},
    InstallVoice,
    Start{plugin_id:String},
    Stop{plugin_id:String},
    Remove{plugin_id:String},
}
fn name_ok(name:&str)->bool{!name.trim().is_empty()&&name.len()<=120&&!name.chars().any(char::is_control)}
fn net_ok(network:bool,port:Option<u16>)->bool{port!=Some(0)&&(port.is_none()||network)}
fn parse(value:Value)->Result<Proposal,String>{
    // Serde's internally tagged unit variant accepts extra fields. Explicit
    // per-action allowlists keep review data and execution authority separate.
    let keys:&[&str]=match value["action"].as_str(){
        Some("install_voice")=>&["action"],
        Some("install_image")=>&["action","name","image","network","port"],
        Some("install_package")=>&["action","name","spec","network","port"],
        Some("start"|"stop"|"remove")=>&["action","plugin_id"],
        _=>return Err("Invalid plugin proposal action".into()),
    };
    if !value.as_object().is_some_and(|m|m.keys().all(|k|keys.contains(&k.as_str()))){
        return Err("Unexpected plugin proposal fields".into())
    }
    let p:Proposal=serde_json::from_value(value).map_err(|_|"Invalid plugin proposal schema".to_owned())?;
    let valid=match &p{
        Proposal::InstallImage{name,image,network,port}=>name_ok(name)&&valid_image(image)
            &&!recipe::voice_tag(image)&&!package::local_tag(image)&&net_ok(*network,*port),
        Proposal::InstallPackage{name,spec,network,port}=>name_ok(name)&&spec.valid()&&net_ok(*network,*port),
        Proposal::InstallVoice=>true,
        Proposal::Start{plugin_id}|Proposal::Stop{plugin_id}|Proposal::Remove{plugin_id}=>valid_id(plugin_id),
    };
    if !valid{return Err("Invalid plugin proposal settings".into())} Ok(p)
}
fn target_id(p:&Proposal)->Option<&str>{match p{
    Proposal::Start{plugin_id}|Proposal::Stop{plugin_id}|Proposal::Remove{plugin_id}=>Some(plugin_id),
    _=>None
}}
fn digest(v:&Value)->String{format!("{:x}",Sha256::digest(v.to_string().as_bytes()))}
fn tool(name:&str,description:&str,properties:Value,required:Vec<&str>)->Value{
    json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})
}
pub(crate) fn definitions()->Vec<Value>{
    vec![
        tool("plugin_search","Discover public plugin and package candidates. Results are unverified, not executable instructions.",json!({"query":{"type":"string"},"source":{"type":"string","enum":["all","github","npm","pypi","crates","huggingface","docker"]}}),vec!["query"]),
        tool("plugin_list","Read managed plugin identities and runtime state. Does not grant execution permissions.",json!({}),vec![]),
        tool("plugin_propose","Create an owner-reviewed installation or lifecycle request in Service Connections > Plugins. Does not install or execute anything. For package install use an exact version and documented executable.",json!({
            "proposal":{"type":"object","properties":{
                "action":{"type":"string","enum":["install_image","install_package","install_voice","start","stop","remove"]},
                "name":{"type":"string"},"image":{"type":"string"},"network":{"type":"boolean"},"port":{"type":"integer"},
                "plugin_id":{"type":"string"},"spec":{"type":"object","properties":{"runtime":{"type":"string","enum":["pypi","npm","crates"]},"package":{"type":"string"},"version":{"type":"string"},"entry":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}},"required":["runtime","package","version","entry"],"additionalProperties":false}
            },"required":["action"],"additionalProperties":false}
        }),vec!["proposal"])
    ]
}
pub(crate) async fn execute(state:&AppState,name:&str,args:&Value)->Option<Result<Value,String>>{
    if !matches!(name,"plugin_search"|"plugin_list"|"plugin_propose"){return None}
    Some(async{
        match name{
            "plugin_search"=>crate::api::extensions::plugin_catalog::search_page(args["query"].as_str().unwrap_or(""),args["source"].as_str().unwrap_or("all")).await,
            "plugin_list"=>{let mut rows=inventory(&owner(state)?).await?;for row in &mut rows{super::data::attach(state,row);super::package::attach(state,row);super::versions::attach(state,row);super::scheduled::attach(state,row);}Ok(json!({"items":rows}))},
            _=>{
                let p=parse(args["proposal"].clone())?;
                let _lock=state.integrations.plugin_proposal_mutations.try_lock().map_err(|_|"Plugin review busy")?;
                let pending=state.store.all(KIND).into_iter().filter(|r|r["status"]=="pending_owner").collect::<Vec<_>>();
                let mut binding=Value::Null;
                if let Some(id)=target_id(&p){
                    let row=inspect(id,&owner(state)?).await?.ok_or("Plugin not installed")?;
                    binding=json!({"containerId":row["containerId"],"imageId":row["imageId"],"name":row["name"]});
                }
                let payload=json!({"proposal":p,"binding":binding});
                let hash=digest(&payload);
                if let Some(r)=pending.iter().find(|r|r["digest"]==hash){return Ok(r.clone())}
                if pending.len()>=32{return Err("Plugin review queue is full".into())}
                let id=uuid::Uuid::new_v4().to_string();
                let r=json!({"id":id,"status":"pending_owner","digest":hash,"payload":payload,"createdAt":chrono::Utc::now().to_rfc3339()});
                state.store.put(KIND,&r).ok_or("Cannot save plugin proposal".into())
            }
        }
    }.await)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review{confirmed:bool,digest:String}
pub async fn list(State(state):State<AppState>)->Response{
    Json(json!({"items":state.store.all(KIND).into_iter().filter(|r|r["status"]=="pending_owner").collect::<Vec<_>>()})).into_response()
}
fn review(state:&AppState,id:&str,input:&Review)->Result<Value,Response>{
    if !input.confirmed||!valid_id(id){return Err(error(StatusCode::BAD_REQUEST,"검토 내용을 확인하세요."))}
    let r=state.store.get(KIND,id).ok_or_else(||error(StatusCode::NOT_FOUND,"설치 검토 요청이 없습니다."))?;
    if r["status"]!="pending_owner"||r["digest"]!=input.digest||digest(&r["payload"])!=input.digest{
        return Err(error(StatusCode::CONFLICT,"검토 요청이 변경되었거나 이미 처리되었습니다."))
    }
    Ok(r)
}
pub async fn dismiss(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Review>)->Response{
    let _lock=match state.integrations.plugin_proposal_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"다른 검토가 진행 중입니다.")};
    let mut r=match review(&state,&id,&input){Ok(r)=>r,Err(e)=>return e};
    r["status"]=json!("dismissed");
    match state.store.put(KIND,&r){Some(_)=>Json(r).into_response(),None=>error(StatusCode::INTERNAL_SERVER_ERROR,"검토 기록 저장 실패")}
}
pub async fn approve(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Review>)->Response{
    let _lock=match state.integrations.plugin_proposal_mutations.try_lock(){Ok(g)=>g,Err(_)=>return error(StatusCode::CONFLICT,"다른 검토가 진행 중입니다.")};
    let mut r=match review(&state,&id,&input){Ok(r)=>r,Err(e)=>return e};
    let p=match parse(r["payload"]["proposal"].clone()){Ok(p)=>p,Err(e)=>return error(StatusCode::BAD_REQUEST,&e)};
    let response=match p{
        Proposal::InstallImage{name,image,network,port}=>super::install(State(state.clone()),Json(Install{id:id.clone(),name,image,network,port,confirmed:true})).await,
        Proposal::InstallPackage{name,spec,network,port}=>{
            let v=json!({"id":id,"name":name,"spec":spec,"network":network,"port":port,"confirmed":true});
            match serde_json::from_value::<package::Request>(v){Ok(req)=>package::install(State(state.clone()),Json(req)).await,Err(_)=>return error(StatusCode::BAD_REQUEST,"Invalid package request")}
        },
        Proposal::InstallVoice=>{
            match serde_json::from_value::<recipe::Request>(json!({"id":id,"confirmed":true})){Ok(req)=>recipe::install_voice(State(state.clone()),Json(req)).await,Err(_)=>return error(StatusCode::BAD_REQUEST,"Invalid voice request")}
        },
        p=>{
            let action=match &p{Proposal::Start{..}=>"start",Proposal::Stop{..}=>"stop",Proposal::Remove{..}=>"remove",_=>unreachable!()};
            let Some(target)=target_id(&p)else{return error(StatusCode::BAD_REQUEST,"Invalid target")};
            let Some(expected)=r["payload"]["binding"]["containerId"].as_str()else{return error(StatusCode::CONFLICT,"검토 대상 식별자가 없습니다.")};
            mutate_checked(state.clone(),target.into(),action,Some(expected)).await
        }
    };
    if response.status().is_success(){
        r["status"]=json!("executed");
        if state.store.put(KIND,&r).is_none(){return error(StatusCode::INTERNAL_SERVER_ERROR,"실행 후 검토 기록 저장 실패. 현재 플러그인 상태를 확인하세요.")}
        state.store.audit("plugin.proposal.executed",json!({"id":id}));
    }
    response
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn proposals_reject_ambient_authority_and_unpinned_images(){
        assert!(parse(json!({"action":"install_image","name":"x","image":"alpine:latest","network":false})).is_err());
        assert!(parse(json!({"action":"install_voice","confirmed":true})).is_err());
        assert!(parse(json!({"action":"install_voice"})).is_ok());
        assert!(parse(json!({"action":"install_image","name":"x","image":"alpine:3.21","network":false,"port":8080})).is_err());
        assert!(parse(json!({"action":"install_image","name":"x","image":"alpine:3.21","network":false})).is_ok());
    }
}
