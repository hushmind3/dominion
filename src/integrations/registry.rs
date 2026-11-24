//! Composable optional tools boundary for the host engine.
//! The core planner sees descriptions and invokes one generic tool adapter;
//! connectors remain independently owned, but host approval, effect IDs,
//! cancellation and durable receipts are never bypassed by this module.
use crate::{engine::tools::ToolContext,state::AppState};
use crate::integrations::{accounts,browser,skills,network,mcp};
use dominion_sdk::harness::ToolSpec;
use serde_json::Value;

/// Extend here when installing a new *host-integrated* tool family.
/// MCP tools stay in the separate `mcp` registry and are not duplicated.
pub fn definitions()->Vec<Value>{
 let mut result=skills::tools::definitions();
 result.push(browser::tool_definition());
 result.extend(accounts::tools::definitions());
 result.extend(network::definitions());
 result.extend(crate::api::plugins::agent::definitions());
 result
}

// A transport adapter supplies discovery, approval metadata, a pinned
// contract and execution. The host never routes by protocol in its loops.
use dominion_sdk::harness::BoxFuture;
trait ExternalAdapter: Sync {
 fn owns(&self,name:&str)->bool;
 fn list<'a>(&'a self,state:&'a AppState)->BoxFuture<'a,Vec<ToolSpec>>;
 fn fingerprint(&self,state:&AppState,spec:&ToolSpec)->Result<String,String>;
 fn requires_approval(&self,state:&AppState,name:&str)->bool;
 fn call<'a>(&'a self,state:&'a AppState,name:&'a str,args:&'a Value)->BoxFuture<'a,Result<Value,String>>;
 fn call_pinned<'a>(&'a self,state:&'a AppState,spec:&'a ToolSpec,args:&'a Value)->BoxFuture<'a,Result<Value,String>>;
}
struct LegacyMcpAdapter;
impl ExternalAdapter for LegacyMcpAdapter {
 fn owns(&self,name:&str)->bool{name.starts_with("mcp__")}
 fn list<'a>(&'a self,state:&'a AppState)->BoxFuture<'a,Vec<ToolSpec>>{Box::pin(async move{
  mcp::tools(state).await.into_iter().map(|item|ToolSpec{
   name:item.qualified(),description:item.description,schema:item.schema,read_only:false
  }).collect()
 })}
 fn fingerprint(&self,state:&AppState,spec:&ToolSpec)->Result<String,String>{
  mcp::native_contract_fingerprint(state,&spec.name,&spec.schema,&spec.description)
 }
 fn requires_approval(&self,state:&AppState,name:&str)->bool{mcp::requires_approval(state,name)}
 fn call<'a>(&'a self,state:&'a AppState,name:&'a str,args:&'a Value)->BoxFuture<'a,Result<Value,String>>{
  Box::pin(mcp::call(state,name,args))
 }
 fn call_pinned<'a>(&'a self,state:&'a AppState,spec:&'a ToolSpec,args:&'a Value)->BoxFuture<'a,Result<Value,String>>{
  Box::pin(mcp::call_pinned(state,&spec.name,args,&spec.schema,&spec.description))
 }
}
static LEGACY_MCP:LegacyMcpAdapter=LegacyMcpAdapter;
use crate::api::plugins::bridge::tools as installed;
struct InstalledAdapter;
static INSTALLED:InstalledAdapter=InstalledAdapter;
impl ExternalAdapter for InstalledAdapter {
 fn owns(&self,name:&str)->bool{installed::owns(name)}
 fn list<'a>(&'a self,state:&'a AppState)->BoxFuture<'a,Vec<ToolSpec>>{Box::pin(installed::list(state))}
 fn fingerprint(&self,state:&AppState,spec:&ToolSpec)->Result<String,String>{installed::fingerprint(state,spec)}
 fn requires_approval(&self,_:&AppState,_:&str)->bool{true}
 fn call<'a>(&'a self,state:&'a AppState,name:&'a str,args:&'a Value)->BoxFuture<'a,Result<Value,String>>{Box::pin(async move{let expected=installed::approval_binding(state,name)?;installed::call(state,name,args,Some(&expected)).await})}
 fn call_pinned<'a>(&'a self,state:&'a AppState,spec:&'a ToolSpec,args:&'a Value)->BoxFuture<'a,Result<Value,String>>{Box::pin(async move{let expected=installed::fingerprint(state,spec)?;installed::call(state,&spec.name,args,Some(&expected)).await})}
}
fn adapters()->[&'static dyn ExternalAdapter;2]{[&INSTALLED,&LEGACY_MCP]}
fn adapter(name:&str)->Option<&'static dyn ExternalAdapter>{adapters().into_iter().find(|a|a.owns(name))}
pub fn is_external(name:&str)->bool{adapter(name).is_some()}
pub async fn native_external_tools(state:&AppState)->Vec<ToolSpec>{
 let mut out=Vec::new();
 for a in adapters(){out.extend(a.list(state).await);}
 out
}
pub async fn external_definitions(state:&AppState)->Vec<Value>{
 native_external_tools(state).await.into_iter().map(|spec|serde_json::json!({
  "type":"function","function":{"name":spec.name,"description":spec.description,"parameters":spec.schema}
 })).collect()
}
pub fn external_requires_approval(state:&AppState,name:&str)->bool{
 adapter(name).is_none_or(|a|a.requires_approval(state,name))
}
pub fn tool_requires_approval(state:&AppState,name:&str)->bool{
 adapter(name).map(|a|a.requires_approval(state,name)).unwrap_or_else(||crate::engine::tools::requires_approval(name))
}
pub fn approval_binding(state:&AppState,name:&str)->Result<Option<String>,String>{
 if installed::owns(name){installed::approval_binding(state,name).map(Some)}else{Ok(None)}
}
pub async fn call_external(state:&AppState,name:&str,args:&Value,expected:Option<&str>)->Result<Value,String>{
 if installed::owns(name){return installed::call(state,name,args,expected).await}
 adapter(name).ok_or("External tool adapter unavailable")?.call(state,name,args).await
}
pub async fn call_pinned_external(state:&AppState,spec:&ToolSpec,args:&Value,expected:&str)->Result<Value,String>{
 let a=adapter(&spec.name).ok_or("External tool adapter unavailable")?;
 if a.fingerprint(state,spec)?!=expected{return Err("External tool authorization changed after approval".into())}
 if installed::owns(&spec.name){return installed::call(state,&spec.name,args,Some(expected)).await}
 a.call_pinned(state,spec,args).await
}
pub fn external_contracts(state:&AppState,specs:&[ToolSpec])->Result<Value,String>{
 let mut out=serde_json::Map::new();
 for spec in specs{
  if let Some(a)=adapter(&spec.name){out.insert(spec.name.clone(),serde_json::json!(a.fingerprint(state,spec)?));}
 }
 Ok(Value::Object(out))
}
// Keep historical jobs readable without re-authorizing any changed tool.
pub fn saved_contracts(job:&Value)->&Value{
 job.get("nativeExternalContracts").or_else(||job.get("nativeMcpContracts")).unwrap_or(&Value::Null)
}
pub fn verify_external_contract(state:&AppState,job:&Value,spec:&ToolSpec)->Result<String,String>{
 let a=adapter(&spec.name).ok_or("External adapter unavailable")?;
 let pinned=saved_contracts(job)[&spec.name].as_str().ok_or("External tool contract absent from durable native job")?;
 if pinned!=a.fingerprint(state,spec)?{return Err("External tool identity changed; review required".into())}
 Ok(pinned.to_owned())
}
// Request origin is evaluated by the host's authorization adapter, not by
// the execution loop. A revoked grant cannot dispatch a new external effect.
pub fn job_authorized(state:&AppState,job:&Value)->bool{
 crate::api::plugins::bridge::v2::job_authorized(state,job)
}
pub fn requires_interactive_review(job:&Value)->bool{job["input"]["pluginBridgeV2"]==true}
pub fn job_tool_requires_approval(state:&AppState,job:&Value,name:&str)->bool{
 if requires_interactive_review(job){!matches!(name,"web_search"|"http_fetch")}else{tool_requires_approval(state,name)}
}

pub fn filter_available(state:&AppState,definitions:Vec<Value>)->Vec<Value>{
 accounts::tools::filter_available(state,definitions)
}

/// Channel delivery is an adapter task, not a core engine job type.
pub async fn dispatch_job(
 ctx:&crate::engine::jobs::JobContext,job:&Value,kind:&str
)->Option<Result<Value,String>>{
 match kind {
  "discord" => Some(crate::integrations::channels::discord::run(ctx,job).await),
  "external_agent" if job["input"]["bridgeRuntime"]=="remote"=>
      Some(crate::host::execution::remote_worker::run_team_read(ctx,job).await),
  "external_agent" => Some(crate::integrations::agent_bridge::run(ctx,job).await),
  _=>None,
 }
}

pub fn requires_approval(name:&str)->bool{
 browser::requires_approval(name)||accounts::tools::requires_approval(name)||name=="plugin_propose"
}

/// Returns None only for tools not owned by any optional integration.
/// Caller must run the host's normal permission and effect-admission gates.
pub async fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Option<Result<Value,String>>{
 if let Some(output)=accounts::tools::execute(ctx.state,name,args).await{return Some(output)}
 if let Some(output)=network::execute(name,args).await{return Some(output)}
 if let Some(output)=crate::api::plugins::agent::execute(ctx.state,name,args).await{return Some(output)}
 if let Some(output)=skills::tools::execute(ctx,name,args).await{return Some(output)}
 if name=="browser_control"{return Some(browser::execute_tool(ctx,args).await)}
 None
}

#[cfg(test)]
mod tests{
 use super::*;
 use std::collections::HashSet;
 #[test]
 fn all_integration_declarations_have_one_name_and_preserve_core_distinction(){
  let tools=definitions();
  let mut names=HashSet::new();
  for definition in &tools{
   let name=definition["function"]["name"].as_str().unwrap();
   assert!(names.insert(name), "duplicate optional tool {name}");
   assert!(!matches!(name,"shell_exec"|"workspace_write"|"delegate_task"));
  }
  assert_eq!(names.len(),18);
  assert!(requires_approval("plugin_propose"));
  assert!(names.contains("browser_control"));
  assert!(names.contains("google_workspace_read"));
  assert!(names.contains("skill_search"));
  assert!(requires_approval("browser_control"));
  assert!(requires_approval("github_file_commit"));
  assert!(!requires_approval("skill_search"));
 }
}
