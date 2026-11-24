//! Language-neutral capability provider. Installed programs expose their own
//! tools via the SDK; no program or protocol-specific implementation lives here.
use super::*;
use dominion_sdk::harness::ToolSpec;
use std::collections::BTreeSet;
const GRANT:&str="plugin_tool_grant";
const MAX_TOOLS:usize=64;
const MAX_PROVIDERS:usize=8;
#[derive(Clone,serde::Serialize,Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
struct Declaration{name:String,description:String,schema:Value}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog{tools:Vec<Declaration>}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Change{enabled:bool,confirmed:bool,digest:Option<String>}
fn local_ok(s:&str)->bool{!s.is_empty()&&s.len()<=24&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')}
fn parse(v:Value)->Result<Vec<Declaration>,String>{
 let mut c:Catalog=serde_json::from_value(v).map_err(|_|"Invalid extension tool catalog")?;
 let mut seen=BTreeSet::new();
 if c.tools.is_empty()||c.tools.len()>MAX_TOOLS||c.tools.iter().any(|t|
   !local_ok(&t.name)||!seen.insert(t.name.clone())||t.description.trim().is_empty()||
   t.description.len()>2048||t.description.chars().any(|x|x.is_control())||
   !t.schema.is_object()||t.schema["type"]!="object"||t.schema.to_string().len()>12288){
   return Err("Invalid, duplicate or oversized extension tool declaration".into())
 }
 c.tools.sort_by(|a,b|a.name.cmp(&b.name));Ok(c.tools)
}
fn qualified(id:&str,local:&str)->String{format!("ext__{}__{local}",id.replace('-',""))}
fn identity(name:&str)->Option<(String,&str)>{
 let (id,local)=name.strip_prefix("ext__")?.split_once("__")?;
 if id.len()!=32||!local_ok(local)||!id.bytes().all(|b|b.is_ascii_hexdigit()){return None}
 Some((uuid::Uuid::parse_str(id).ok()?.to_string(),local))
}
pub(crate) fn owns(name:&str)->bool{name.starts_with("ext__")}
fn spec(id:&str,t:&Declaration)->ToolSpec{ToolSpec{name:qualified(id,&t.name),description:t.description.clone(),schema:t.schema.clone(),read_only:false}}
fn declarations(g:&Value)->Result<Vec<Declaration>,String>{parse(json!({"tools":g["tools"]}))}
fn binding(g:&Value)->String{digest(&json!({"id":g["id"],"containerId":g["containerId"],"imageId":g["imageId"],"tools":g["tools"]}).to_string())}
fn authorization(g:&Value)->String{digest(&json!({"binding":binding(g),"grantId":g["grantId"]}).to_string())}
fn grant(state:&AppState,id:&str)->Result<Value,String>{
 state.store.get(GRANT,id).filter(|g|g["enabled"]==true&&g["grantId"].is_string()).ok_or("Extension tools are not enabled".into())
}
fn admitted(state:&AppState,name:&str)->Result<(Value,Declaration),String>{
 let (id,local)=identity(name).ok_or("Invalid extension tool name")?;
 let g=grant(state,&id)?;
 let t=declarations(&g)?.into_iter().find(|t|t.name==local).ok_or("Tool was not approved")?;
 Ok((g,t))
}
pub(crate) fn fingerprint(state:&AppState,s:&ToolSpec)->Result<String,String>{
 let (g,t)=admitted(state,&s.name)?;
 if s.description!=t.description||s.schema!=t.schema||s.read_only{return Err("Extension tool declaration changed".into())}
 Ok(authorization(&g))
}
pub(crate) fn approval_binding(state:&AppState,name:&str)->Result<String,String>{let(g,_)=admitted(state,name)?;Ok(authorization(&g))}
fn endpoint(port:u16,path:&str)->String{format!("http://127.0.0.1:{port}/dominion/v2/tools{path}")}
async fn live_catalog(state:&AppState,row:&Value,port:u16)->Result<Vec<Declaration>,String>{
 let response=client()?.get(endpoint(port,"")).bearer_auth(v2::token(state,row)?).send().await.map_err(|e|e.to_string())?;
 if !response.status().is_success(){return Err(format!("Extension catalog HTTP {}",response.status()))}
 parse(crate::integrations::network::read_bounded_json(response,1024*1024,"extension catalog").await?)
}
async fn preview(state:&AppState,id:&str)->Result<Value,String>{
 let (row,port)=target(state,id).await?;
 let tools=live_catalog(state,&row,port).await?;
 let mut g=json!({"id":id,"containerId":row["containerId"],"imageId":row["imageId"],"tools":tools});
 g["digest"]=json!(binding(&g));Ok(g)
}
pub async fn status(State(state):State<AppState>,Path(id):Path<String>)->Response{
 if !valid_id(&id){return invalid("Invalid extension ID")}
 Json(state.store.get(GRANT,&id).unwrap_or_else(||json!({"enabled":false,"tools":[]}))).into_response()
}
pub async fn discover(State(state):State<AppState>,Path(id):Path<String>)->Response{
 match preview(&state,&id).await{Ok(g)=>Json(g).into_response(),Err(e)=>upstream(&e)}
}
pub async fn change(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Change>)->Response{
 if !valid_id(&id)||!input.confirmed{return invalid("Confirm the extension tools")}
 let _lock=match state.integrations.plugin_mutations.try_lock(){Ok(g)=>g,Err(_)=>return conflict("Extension change in progress")};
 if !input.enabled{
  let mut g=state.store.get(GRANT,&id).unwrap_or_else(||json!({"id":id,"tools":[]}));
  g["enabled"]=json!(false);
  return match state.store.put(GRANT,&g){Some(_)=>Json(g).into_response(),None=>upstream("Could not revoke extension tools")}
 }
 if state.maintenance_mode(){return conflict("Maintenance mode")}
 if state.store.get(GRANT,&id).is_some_and(|g|g["enabled"]==true){return conflict("Revoke the previous catalog first")}
 if state.store.all(GRANT).iter().filter(|g|g["enabled"]==true).count()>=MAX_PROVIDERS{return conflict("Extension tool provider limit reached")}
 let mut g=match preview(&state,&id).await{Ok(g)=>g,Err(e)=>return upstream(&e)};
 if input.digest.as_deref()!=g["digest"].as_str(){return conflict("The reviewed extension catalog changed")}
 g["enabled"]=json!(true);g["grantId"]=json!(uuid::Uuid::new_v4().to_string());
 match state.store.put(GRANT,&g){Some(_)=>Json(g).into_response(),None=>upstream("Could not save extension tools")}
}
pub(crate) async fn list(state:&AppState)->Vec<ToolSpec>{
 let mut result=Vec::new();
 for g in state.store.all(GRANT).into_iter().filter(|g|g["enabled"]==true).take(MAX_PROVIDERS){
  let Some(id)=g["id"].as_str().filter(|id|valid_id(id))else{continue};
  // Only an installed, currently running, identical package is discoverable.
  let Ok((row,_))=target(state,id).await else{continue};
  if !binding_matches(&g,&row){continue}
  if let Ok(tools)=declarations(&g){result.extend(tools.iter().map(|t|spec(id,t)))}
 }
 result
}
pub(crate) async fn call(state:&AppState,name:&str,args:&Value,expected:Option<&str>)->Result<Value,String>{
 if !args.is_object()||args.to_string().len()>65536{return Err("Invalid extension tool arguments".into())}
 let expected=expected.ok_or("Extension tool requires a pinned authorization")?;
 // Lifecycle changes and catalog revocation are serialized with dispatch.
 let _lock=state.integrations.plugin_mutations.try_lock().map_err(|_|"Extension lifecycle is busy")?;
 let (g,t)=admitted(state,name)?;
 if authorization(&g)!=expected{return Err("Extension authorization changed after approval".into())}
 let id=g["id"].as_str().ok_or("Missing extension ID")?;
 let (row,port)=target(state,id).await?;
 if !binding_matches(&g,&row){return Err("Extension installation changed; reapprove tools".into())}
 let fresh=live_catalog(state,&row,port).await?;
 if fresh!=declarations(&g)?{return Err("Extension catalog changed; reapprove tools".into())}
 let call_id=uuid::Uuid::new_v4().to_string();
 let response=client()?.post(endpoint(port,"/call")).bearer_auth(v2::token(state,&row)?)
  .json(&json!({"callId":call_id,"name":t.name,"arguments":args,"declaration":t}))
  .send().await.map_err(|e|format!("Extension call outcome uncertain: {e}"))?;
 if !response.status().is_success(){return Err(format!("Extension call HTTP {}; outcome may be uncertain",response.status()))}
 let v=crate::integrations::network::read_bounded_json(response,1024*1024,"extension result").await?;
 if v["callId"]!=call_id||!v["success"].is_boolean(){return Err("Extension result identity invalid; outcome uncertain".into())}
 if v["success"]!=true{return Err("Extension returned failure; inspect its execution record".into())}
 v.get("result").cloned().ok_or("Missing extension result".into())
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn declaration_and_namespace(){
  let d=json!({"tools":[{"name":"read","description":"Read a record","schema":{"type":"object"}}]});
  let a=parse(d.clone()).unwrap();assert_eq!(a.len(),1);
  let id="01234567-89ab-4def-8123-456789abcdef";let name=qualified(id,&a[0].name);
  assert_eq!(identity(&name),Some((id.to_string(),"read")));assert!(name.len()<=64);
  assert!(parse(json!({"tools":[d["tools"][0],d["tools"][0]]})).is_err());
  let mut x=d;x["tools"][0]["approval"]=json!(false);assert!(parse(x).is_err());
 }
 #[test] fn binding_changes_with_image_or_grant(){
  let mut g=json!({"id":"x","containerId":"a","imageId":"b","tools":[],"grantId":"1"});
  let old=authorization(&g);g["grantId"]=json!("2");assert_ne!(authorization(&g),old);
  let old=authorization(&g);g["imageId"]=json!("c");assert_ne!(authorization(&g),old);
 }
}
