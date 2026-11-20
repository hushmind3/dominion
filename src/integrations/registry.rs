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
 result
}

/// The engine sees only typed external-tool descriptors, never MCP transports.
pub async fn native_external_tools(state:&AppState)->Vec<ToolSpec>{
 mcp::tools(state).await.into_iter().map(|item|ToolSpec{
  name:item.qualified(),description:item.description,schema:item.schema,read_only:false
 }).collect()
}

/// Pin the registered transport/effect contract before dispatching a job tool.
pub fn contract_fingerprint(
 state:&AppState,name:&str,schema:&Value,description:&str
)->Result<String,String>{mcp::native_contract_fingerprint(state,name,schema,description)}

pub fn external_requires_approval(state:&AppState,name:&str)->bool{
 mcp::requires_approval(state,name)
}

pub async fn call_pinned_external(
 state:&AppState,name:&str,args:&Value,schema:&Value,description:&str
)->Result<Value,String>{mcp::call_pinned(state,name,args,schema,description).await}

pub fn filter_available(state:&AppState,definitions:Vec<Value>)->Vec<Value>{
 accounts::tools::filter_available(state,definitions)
}

/// Channel delivery is an adapter task, not a core engine job type.
pub async fn dispatch_job(
 ctx:&crate::engine::jobs::JobContext,job:&Value,kind:&str
)->Option<Result<Value,String>>{
 match kind {
  "discord" => Some(crate::integrations::channels::discord::run(ctx,job).await),
  "external_agent" => Some(crate::integrations::agent_bridge::run(ctx,job).await),
  _=>None,
 }
}

pub fn requires_approval(name:&str)->bool{
 browser::requires_approval(name)||accounts::tools::requires_approval(name)
}

/// Returns None only for tools not owned by any optional integration.
/// Caller must run the host's normal permission and effect-admission gates.
pub async fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Option<Result<Value,String>>{
 if let Some(output)=accounts::tools::execute(ctx.state,name,args).await{return Some(output)}
 if let Some(output)=network::execute(name,args).await{return Some(output)}
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
  assert_eq!(names.len(),15);
  assert!(names.contains("browser_control"));
  assert!(names.contains("google_workspace_read"));
  assert!(names.contains("skill_search"));
  assert!(requires_approval("browser_control"));
  assert!(requires_approval("github_file_commit"));
  assert!(!requires_approval("skill_search"));
 }
}
