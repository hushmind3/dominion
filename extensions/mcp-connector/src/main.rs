//! Independent MCP extension. Implements the same generic SDK as any other tool provider.
mod mcp;
mod state;
// Original transport fixtures exercise the same implementation in isolation.
#[cfg(test)] mod config{pub use crate::state::Config;}
#[cfg(test)] mod db{pub use crate::state::Store;}
#[cfg(test)] mod vault{pub use crate::state::Vault;}
use axum::{extract::{State,DefaultBodyLimit},http::{HeaderMap,StatusCode},routing::{get,post},Router,Json};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::HashSet,io::Write,path::{Path,PathBuf},sync::Arc};
use state::{AppState,Config,Store,Vault};
type Failure=(StatusCode,Json<Value>);
fn fail(status:StatusCode,message:&str)->Failure{(status,Json(json!({"error":message})))}
fn invalid(message:&str)->Failure{fail(StatusCode::BAD_REQUEST,message)}
fn server_error(_:impl std::fmt::Display)->Failure{fail(StatusCode::INTERNAL_SERVER_ERROR,"Connector storage or transport failed")}
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(v.to_string().as_bytes()))}
fn atomic(path:&Path,value:&Value)->std::io::Result<()>{
 let tmp=path.with_extension(format!("{}.tmp",uuid::Uuid::new_v4()));
 let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
 #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
 let mut file=options.open(&tmp)?;
 let result=(||{file.write_all(value.to_string().as_bytes())?;file.sync_all()?;std::fs::rename(&tmp,path)?;std::fs::File::open(path.parent().unwrap())?.sync_all()})();
 if result.is_err(){let _=std::fs::remove_file(&tmp);}result
}
fn read_json(path:&Path)->Result<Value,Failure>{
 let metadata=std::fs::metadata(path).map_err(server_error)?;
 if metadata.len()>1024*1024{return Err(invalid("State file too large"))}
 serde_json::from_slice(&std::fs::read(path).map_err(server_error)?).map_err(server_error)
}
const MASK:&str="••••";
fn redacted(config:&Value)->Value{
 let mut values=config["values"].clone();
 if let Some(servers)=values["servers"].as_array_mut(){for s in servers{
  if s["apiKey"].is_string(){s["apiKey"]=json!(MASK)}
  if let Some(env)=s["env"].as_object_mut(){for(_,v)in env{*v=json!(MASK)}}
 }}
 json!({"values":values,"description":"MCP servers: {\"servers\":[{\"id\":\"unique-id\",\"name\":\"Server\",\"transport\":\"http\",\"url\":\"https://example.com/mcp\",\"apiKey\":\"optional\",\"enabled\":true}]}. For stdio use command and args; programs run inside this isolated extension, not on your PC. Keep masked secrets unchanged to retain them. Save, then register extension tools in DOMINION."})
}
fn validate(mut values:Value,previous:&Value)->Result<Value,Failure>{
 if !values.is_object()||values.as_object().unwrap().keys().any(|k|k!="servers"){return Err(invalid("Expected a servers array"))}
 let servers=values["servers"].as_array_mut().ok_or_else(||invalid("Expected servers array"))?;
 if servers.len()>8{return Err(invalid("At most eight servers per installation"))}
 let mut ids=HashSet::new();let mut names=HashSet::new();
 for s in servers{
  let map=s.as_object().ok_or_else(||invalid("Invalid server record"))?;
  if map.keys().any(|k|!matches!(k.as_str(),"id"|"name"|"enabled"|"transport"|"url"|"command"|"args"|"env"|"apiKey"|"protocolMode")){return Err(invalid("Unknown server field"))}
  let id=s["id"].as_str().filter(|v|!v.is_empty()&&v.len()<=120).ok_or_else(||invalid("Server ID required"))?.to_owned();
  let name=s["name"].as_str().filter(|v|!v.trim().is_empty()&&v.len()<=100&&!v.chars().any(char::is_control)).ok_or_else(||invalid("Server name required"))?.to_owned();
  if !ids.insert(id.clone())||!names.insert(name.to_lowercase()){return Err(invalid("Duplicate server identity"))}
  if s["enabled"].is_null(){s["enabled"]=json!(true)}else if !s["enabled"].is_boolean(){return Err(invalid("Invalid enabled value"))}
  if s["protocolMode"].as_str().is_some_and(|x|!matches!(x,"auto"|"modern"|"legacy")){return Err(invalid("Invalid protocol mode"))}
  match s["transport"].as_str(){
   Some("http")=>{
    let u=reqwest::Url::parse(s["url"].as_str().unwrap_or("")).map_err(|_|invalid("Invalid HTTP URL"))?;
    if !matches!(u.scheme(),"http"|"https")||u.host_str().is_none()||!u.username().is_empty()||u.password().is_some(){return Err(invalid("Invalid HTTP endpoint"))}
   }
   Some("stdio")=>{
    if !s["command"].as_str().is_some_and(|v|!v.is_empty()&&v.len()<=256&&!v.contains('\0')){return Err(invalid("Invalid executable"))}
    if !s["args"].is_null()&&!s["args"].as_array().is_some_and(|a|a.len()<=64&&a.iter().all(|v|v.as_str().is_some_and(|s|s.len()<=4096&&!s.contains('\0')))){return Err(invalid("Invalid executable arguments"))}
   }
   _=>return Err(invalid("Unsupported transport"))
  }
  let old=previous["values"]["servers"].as_array().into_iter().flatten().find(|v|v["id"]==id);
  if s["apiKey"]==MASK{s["apiKey"]=old.map(|v|v["apiKey"].clone()).unwrap_or(Value::Null)}
  if !s["apiKey"].is_null()&&!s["apiKey"].as_str().is_some_and(|v|v.len()<=8192&&!v.chars().any(char::is_control)){return Err(invalid("Invalid credential"))}
  if !s["env"].is_null(){
   let env=s["env"].as_object_mut().ok_or_else(||invalid("Invalid environment"))?;
   if env.len()>64{return Err(invalid("Too many environment variables"))}
   for(k,v)in env{
    if k=="DOMINION_BRIDGE_TOKEN"||k.is_empty()||k.len()>120||!k.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'){return Err(invalid("Invalid environment key"))}
    if *v==MASK{*v=old.and_then(|o|o["env"].get(k)).cloned().ok_or_else(||invalid("No stored value for masked environment variable"))?}
    if !v.as_str().is_some_and(|s|s.len()<=8192&&!s.contains('\0')){return Err(invalid("Invalid environment value"))}
   }
  }
 }
 if values.to_string().len()>65536{return Err(invalid("Configuration too large"))}Ok(values)
}
fn runtime(root:&Path,config:&Value)->Result<AppState,Failure>{
 let workspace=root.join("workspace");std::fs::create_dir_all(&workspace).map_err(server_error)?;
 let state=AppState::new(Config{port:0,data_dir:root.to_owned(),web_dir:root.to_owned(),workspace},Store::default(),Vault::default());
 for entry in config["values"]["servers"].as_array().ok_or_else(||invalid("Invalid saved configuration"))?{
  let mut row=entry.clone();
  if let Some(key)=row.as_object_mut().and_then(|o|o.remove("apiKey")).and_then(|v|v.as_str().map(str::to_owned)){
   state.save_secret(&format!("mcp:{}",row["id"].as_str().unwrap_or_default()),&key);
  }
  state.store.put("mcp",&row).ok_or_else(||invalid("Invalid server record"))?;
 }
 Ok(state)
}
struct Connector{root:PathBuf,config:Value,state:AppState}
#[derive(Clone)]
struct Service{token:Arc<String>,inner:Arc<tokio::sync::Mutex<Connector>>}
fn authorize(service:&Service,headers:&HeaderMap)->Result<(),Failure>{
 let got=headers.get("authorization").and_then(|v|v.to_str().ok()).and_then(|v|v.strip_prefix("Bearer ")).unwrap_or("");
 let expected=service.token.as_bytes();let actual=got.as_bytes();
 if actual.len()!=expected.len()||actual.iter().zip(expected).fold(0u8,|a,(b,c)|a|(b^c))!=0{return Err(fail(StatusCode::UNAUTHORIZED,"Authentication required"))}Ok(())
}
fn declaration(t:&mcp::McpTool,revision:&Value)->Value{
 let stem=t.tool.chars().filter(|c|c.is_ascii_alphanumeric()||*c=='_').take(7).collect::<String>();
 let key=hash(&json!([t.server_id,t.tool]));
 let desc=format!("{}/{}: {}",t.server_name,t.tool,t.description).chars().map(|c|if c.is_control(){' '}else{c}).collect::<String>();
 let mut clipped=String::new();for c in desc.chars(){if clipped.len()+c.len_utf8()>1800{break}clipped.push(c)}
 json!({"name":format!("{}_{}",stem,&key[..16]),"description":format!("{} [config {}]",clipped,revision.as_str().unwrap_or("")),"schema":t.schema})
}
async fn catalog(c:&Connector)->Result<Vec<(mcp::McpTool,Value)>,Failure>{
 let (tools,errors)=mcp::tools_with_errors(&c.state).await;
 if !errors.is_empty(){return Err(fail(StatusCode::BAD_GATEWAY,"One or more configured MCP servers failed discovery; check extension settings"))}
 let mut seen=HashSet::new();
 let out=tools.into_iter().map(|t|{let d=declaration(&t,&c.config["revision"]);(t,d)}).collect::<Vec<_>>();
 if out.len()>64||out.iter().any(|(_,d)|!seen.insert(d["name"].as_str().unwrap_or_default())||d["schema"]["type"]!="object"||d["schema"].to_string().len()>12288){return Err(invalid("Catalog exceeds SDK limits or contains invalid declarations"))}Ok(out)
}
async fn capabilities(State(s):State<Service>,h:HeaderMap)->Result<Json<Value>,Failure>{authorize(&s,&h)?;Ok(Json(json!({"protocol":"dominion.plugin","version":2,"transport":"host-pull","tools":true,"configuration":true})))}
async fn configuration(State(s):State<Service>,h:HeaderMap)->Result<Json<Value>,Failure>{authorize(&s,&h)?;Ok(Json(redacted(&s.inner.lock().await.config)))}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change{confirmed:bool,values:Value}
async fn configure(State(s):State<Service>,h:HeaderMap,Json(change):Json<Change>)->Result<Json<Value>,Failure>{
 authorize(&s,&h)?;if !change.confirmed{return Err(invalid("Configuration confirmation required"))}
 let mut c=s.inner.lock().await;
 let values=validate(change.values,&c.config)?;
 let next=json!({"values":values,"revision":uuid::Uuid::new_v4().to_string()});
 let next_state=runtime(&c.root,&next)?;
 atomic(&c.root.join("configuration.json"),&next).map_err(server_error)?;
 mcp::invalidate(&c.state);c.config=next;c.state=next_state;
 Ok(Json(redacted(&c.config)))
}
async fn tools(State(s):State<Service>,h:HeaderMap)->Result<Json<Value>,Failure>{
 authorize(&s,&h)?;let c=s.inner.lock().await;let rows=catalog(&c).await?;
 Ok(Json(json!({"tools":rows.into_iter().map(|(_,d)|d).collect::<Vec<_>>()})))
}
#[derive(Deserialize,serde::Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Call{call_id:String,name:String,arguments:Value,declaration:Value}
async fn invoke(State(s):State<Service>,h:HeaderMap,Json(call):Json<Call>)->Result<Json<Value>,Failure>{
 authorize(&s,&h)?;
 if uuid::Uuid::parse_str(&call.call_id).ok().map(|v|v.to_string()).as_deref()!=Some(call.call_id.as_str())||!call.arguments.is_object()||call.arguments.to_string().len()>65536{return Err(invalid("Invalid call identity or arguments"))}
 let c=s.inner.lock().await;let digest=hash(&serde_json::to_value(&call).map_err(server_error)?);
 let path=c.root.join("calls").join(format!("{}.json",call.call_id));
 if path.exists(){
  let r=read_json(&path)?;
  if r["digest"]!=digest{return Err(fail(StatusCode::CONFLICT,"Call ID reused"))}
  if r["status"]=="finished"{return Ok(Json(r["response"].clone()))}
  return Err(fail(StatusCode::CONFLICT,"Prior call outcome uncertain; no automatic replay"))
 }
 let (tool,_) =catalog(&c).await?.into_iter().find(|(_,d)|d["name"]==call.name&&*d==call.declaration).ok_or_else(||fail(StatusCode::CONFLICT,"Live declaration changed"))?;
 if std::fs::read_dir(c.root.join("calls")).map_err(server_error)?.take(10001).count()>=10000{return Err(fail(StatusCode::INSUFFICIENT_STORAGE,"Call journal full; review retained records"))}
 atomic(&path,&json!({"digest":digest,"status":"started"})).map_err(server_error)?;
 let result=mcp::call_pinned(&c.state,&tool.qualified(),&call.arguments,&tool.schema,&tool.description).await;
 let response=match result{
  Ok(v) if v["isError"]!=true=>json!({"callId":call.call_id,"success":true,"result":v}),
  Ok(_)=>json!({"callId":call.call_id,"success":false,"result":{"error":"MCP returned an error"}}),
  Err(_)=>json!({"callId":call.call_id,"success":false,"result":{"error":"MCP call failed or outcome uncertain"}})
 };
 if response.to_string().len()>900000{return Err(fail(StatusCode::BAD_GATEWAY,"Result exceeds SDK limit; call outcome retained for review"))}
 atomic(&path,&json!({"digest":digest,"status":"finished","response":response})).map_err(server_error)?;
 Ok(Json(response))
}
fn app(service:Service)->Router{
 Router::new().route("/",get(||async{"DOMINION MCP Connector — configure and register tools from the extension manager."}))
 .route("/dominion/v2/capabilities",get(capabilities)).route("/dominion/v2/configuration",get(configuration).post(configure))
 .route("/dominion/v2/tools",get(tools)).route("/dominion/v2/tools/call",post(invoke))
 .layer(DefaultBodyLimit::max(1024*1024)).with_state(service)
}
#[tokio::main]
async fn main(){
 let token=std::env::var("DOMINION_BRIDGE_TOKEN").expect("Install through the extension manager");
 assert!(token.len()>=16,"Invalid extension credential");
 let root=PathBuf::from(std::env::var("DOMINION_DATA").unwrap_or_else(|_|"/data".into()));
 std::fs::create_dir_all(root.join("calls")).expect("Create extension data directory");
 let path=root.join("configuration.json");
 let config=if path.exists(){read_json(&path).expect("Read extension configuration")}else{json!({"values":{"servers":[]},"revision":uuid::Uuid::new_v4().to_string()})};
 validate(config["values"].clone(),&config).expect("Validate extension configuration");
 let state=runtime(&root,&config).expect("Start extension runtime");
 let service=Service{token:Arc::new(token),inner:Arc::new(tokio::sync::Mutex::new(Connector{root,config,state}))};
 let listener=tokio::net::TcpListener::bind("0.0.0.0:8080").await.expect("Bind extension");
 axum::serve(listener,app(service)).await.expect("Run extension");
}
#[cfg(test)]
mod sdk_tests{
 use super::*;
 #[test] fn redaction_and_validation(){
  let values=json!({"servers":[{"id":"a","name":"A","transport":"http","url":"https://example.com/mcp","apiKey":"secret","env":{"KEY":"other"}}]});
  let cfg=json!({"values":validate(values,&Value::Null).unwrap(),"revision":"rev"});
  let visible=redacted(&cfg);assert_eq!(visible["values"]["servers"][0]["apiKey"],MASK);
  assert_eq!(visible["values"]["servers"][0]["env"]["KEY"],MASK);
  assert_eq!(validate(visible["values"].clone(),&cfg).unwrap(),cfg["values"]);
 }
 #[tokio::test] async fn independent_sdk_executes_real_stdio_and_refuses_duplicate_effect(){
  let root=std::env::temp_dir().join(format!("connector-sdk-{}",uuid::Uuid::new_v4()));
  std::fs::create_dir_all(root.join("calls")).unwrap();
  let script=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/mcp-echo-modern-server.mjs");
  let cfg=json!({"values":{"servers":[{"id":"echo","name":"Echo","transport":"stdio","command":"node","args":[script],"protocolMode":"modern","enabled":true}]},"revision":"fixture"});
  let state=runtime(&root,&cfg).unwrap();
  let service=Service{token:Arc::new("test-token-for-sdk".into()),inner:Arc::new(tokio::sync::Mutex::new(Connector{root:root.clone(),config:cfg,state}))};
  let mut headers=HeaderMap::new();headers.insert("authorization","Bearer test-token-for-sdk".parse().unwrap());
  assert!(tools(State(service.clone()),HeaderMap::new()).await.is_err());
  let Json(list)=tools(State(service.clone()),headers.clone()).await.unwrap();
  let decl=list["tools"][0].clone();
  let request=json!({"callId":uuid::Uuid::new_v4().to_string(),"name":decl["name"],"arguments":{"text":"independent"},"declaration":decl});
  let Json(first)=invoke(State(service.clone()),headers.clone(),Json(serde_json::from_value(request.clone()).unwrap())).await.unwrap();
  assert_eq!(first["success"],true);assert!(first["result"].to_string().contains("MCP_MODERN: independent"));
  let Json(duplicate)=invoke(State(service.clone()),headers.clone(),Json(serde_json::from_value(request.clone()).unwrap())).await.unwrap();
  assert_eq!(duplicate,first);
  let mut changed=request;changed["arguments"]["text"]=json!("changed");
  assert!(invoke(State(service.clone()),headers,Json(serde_json::from_value(changed).unwrap())).await.is_err());
  drop(service);let _=std::fs::remove_dir_all(root);
 }
}
