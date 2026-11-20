//! User-controlled, session-scoped memory tools as optional capabilities.
use crate::engine::tools::ToolContext;
use serde_json::{json,Value};
fn tool(name:&str,description:&str,parameters:Value)->Value{
 json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}
pub fn definitions()->Vec<Value>{
 let object=|properties:Value,required:Vec<&str>|json!({"type":"object","properties":properties,
  "required":required,"additionalProperties":false});
 let text=|description:&str|json!({"type":"string","description":description});
 vec![
        tool("memory_store", "Save a grounded memory for this agent. Quote the user's exact words; invented facts are rejected. Shared memories are managed by the user.", object(json!({"text": text("Fact to remember, maximum 1600 characters"), "evidenceQuote": text("Exact quote from the current user message supporting the fact"), "sourceMessageId": text("Optional earlier source message ID"), "type": json!({"type":"string","enum":["fact","preference","decision","procedure","lesson"]}), "key": text("Optional stable fact key; a new value replaces the previous value for this key"), "scope": json!({"type":"string","enum":["agent"]})}), vec!["text", "evidenceQuote"])),
        tool("memory_search", "Find saved knowledge relevant to a topic", object(json!({"query": text("Search query")}), vec!["query"])),
 ]
}
fn required_arg(args:&Value,key:&str)->Result<String,String>{
 args.get(key).and_then(Value::as_str).map(str::to_string)
  .ok_or_else(||format!("Missing argument: {key}"))
}
pub async fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Option<Result<Value,String>>{
 if !matches!(name,"memory_store"|"memory_search"){return None}
 let result:Result<Value,String>=(||{
  match name {
        "memory_store" => {
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            crate::engine::memory::store_tool(ctx.store, &scope, args)
        }
        "memory_search" => {
            let query = required_arg(args, "query")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            Ok(json!({
                "results": crate::engine::memory::retrieve(ctx.store, &scope, &query, 10)
            }))
        }
   _=>unreachable!("guarded memory tool"),
  }
 })();
 Some(result)
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn optional_memory_tools_stay_in_feature_registry(){
  assert_eq!(definitions().len(),2);
  assert_eq!(definitions()[0]["function"]["name"],"memory_store");
 }
}
