//! Capabilities register their own optional model-visible tools;
//! core execution only consults this interface, never memory implementations.
use crate::engine::tools::ToolContext;
use serde_json::Value;
use super::cognition;
pub fn definitions()->Vec<Value>{
 let mut declared=cognition::tools::definitions();
 declared.extend(super::platform::code_search::definitions());
 declared
}
pub async fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Option<Result<Value,String>>{
 if let Some(result)=cognition::tools::execute(ctx,name,args).await{return Some(result)}
 super::platform::code_search::execute(ctx,name,args)
}
