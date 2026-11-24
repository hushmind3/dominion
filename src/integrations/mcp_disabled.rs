//! Compatibility for historical settings and calls only. No transport or child process.
use crate::state::AppState;
use serde_json::{json,Value};
const DISABLED:&str="Built-in MCP transport is retired; install an independent tool-provider extension or use the legacy-mcp migration build.";
#[derive(Clone)]
pub struct McpTool{pub server_id:String,pub server_name:String,pub slug:String,pub tool:String,pub description:String,pub schema:Value}
impl McpTool{pub fn qualified(&self)->String{format!("mcp__{}__{}",self.slug,self.tool)}}
pub fn invalidate(_: &AppState){}
pub async fn tools(_: &AppState)->Vec<McpTool>{Vec::new()}
pub async fn tools_with_errors(state:&AppState)->(Vec<McpTool>,Vec<Value>){
 (Vec::new(),state.store.all("mcp").into_iter().filter(|r|r["enabled"]!=false)
  .map(|r|json!({"mcpId":r["id"],"server":r["name"],"name":r["name"],"error":DISABLED})).collect())
}
pub async fn definitions(_: &AppState)->Vec<Value>{Vec::new()}
pub fn requires_approval(_: &AppState,_:&str)->bool{true}
pub async fn call(_: &AppState,_:&str,_:&Value)->Result<Value,String>{Err(DISABLED.into())}
pub(crate) fn native_contract_fingerprint(_: &AppState,_:&str,_:&Value,_:&str)->Result<String,String>{Err(DISABLED.into())}
pub(crate) async fn call_pinned(_: &AppState,_:&str,_:&Value,_:&Value,_:&str)->Result<Value,String>{Err(DISABLED.into())}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn disabled_runtime_is_not_a_transport(){assert!(!cfg!(feature="legacy-mcp"));assert!(DISABLED.contains("independent"));}
}
