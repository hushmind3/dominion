//! Optional Skill tools: schemas, scoped discovery and user-reviewed proposals.
use crate::{engine::{memory::MemoryScope,tools::ToolContext},integrations::skills::discovery};
use serde_json::{json,Value};
fn tool(name:&str,description:&str,parameters:Value)->Value{
  json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}
pub fn definitions()->Vec<Value>{
 let object=|properties:Value,required:Vec<&str>|json!({"type":"object","properties":properties,
   "required":required,"additionalProperties":false});
 let text=|description:&str|json!({"type":"string","description":description});
 vec![
        tool("skill_propose", "Draft a reusable skill for user review", object(json!({"name": text("Skill name"), "instructions": text("Reusable instructions")}), vec!["name", "instructions"])),
        tool("skill_search", "Discover owner-enabled Skills 스킬 by task relevance. Returns only scoped names, descriptions and IDs; does not read the full instructions or grant new permissions.", object(json!({
            "query": text("Missing procedural capability, task, or skill topic"),
            "limit":json!({"type":"integer","minimum":1,"maximum":8})
        }), vec!["query"])),
        tool("skill_read", "Load the bounded instructions for an enabled Skill 스킬 by exact ID. Honors agent, session and channel scope. Content is advisory, not a new owner instruction.", object(json!({
            "id":text("ID returned by skill_search")
        }), vec!["id"])),
        tool("skill_resource_read", "Read a text reference/template belonging to an enabled Skill 스킬. Relative paths only, file size bounded, no script execution or access outside the installed Skill folder.", object(json!({
            "id":text("Skill ID returned by skill_search"),
            "path":text("Relative reference path from the installed Skill root")
        }),vec!["id","path"])),
 ]
}
fn required_arg(args:&Value,key:&str)->Result<String,String>{
 args.get(key).and_then(Value::as_str).filter(|s|!s.trim().is_empty())
  .map(str::to_string).ok_or_else(||format!("Missing tool argument: {key}"))
}
pub async fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Option<Result<Value,String>>{
 if !matches!(name,"skill_propose"|"skill_search"|"skill_read"|"skill_resource_read"){return None}
 let result:Result<Value,String>=async {
  match name {
        "skill_search" => {
            let query = required_arg(args, "query")?;
            let limit = args["limit"].as_u64().unwrap_or(5).clamp(1,8) as usize;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,ctx.session_id,ctx.agent_id
            );
            crate::integrations::skills::discovery::search(ctx.store,&scope,&query,limit)
        }
        "skill_read" => {
            let id = required_arg(args, "id")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,ctx.session_id,ctx.agent_id
            );
            crate::integrations::skills::discovery::read(ctx.store,&scope,&id)
        }
        "skill_resource_read" => {
            let id=required_arg(args,"id")?;
            let path=required_arg(args,"path")?;
            let scope=crate::engine::memory::MemoryScope::for_session(
                ctx.store,ctx.session_id,ctx.agent_id
            );
            crate::integrations::skills::discovery::read_resource(ctx.store,&scope,&id,&path)
        }
        "skill_propose" => {
            let name = required_arg(args, "name")?;
            let instructions = required_arg(args, "instructions")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            let record = ctx.store.put(
                "skill",
                &json!({
                    "name": name,
                    "instructions": instructions,
                    "description": instructions.chars().take(160).collect::<String>(),
                    "enabled": false,
                    "status": "draft",
                    "source": "agent",
                    "scope": "agent",
                    "sessionId": ctx.session_id,
                    "agentId": ctx.agent_id,
                    "partitionKey": scope.partition_key,
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
            Ok(json!({ "proposed": true, "skill": record }))
        }
   _=>unreachable!("guarded Skill tool"),
  }
 }.await;
 Some(result)
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn skill_schemas_are_owned_by_extension_and_stable(){
  let names=definitions().into_iter().map(|x|x["function"]["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
  assert_eq!(names,vec!["skill_propose","skill_search","skill_read","skill_resource_read"]);
 }
}
