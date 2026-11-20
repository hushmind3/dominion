//! Capabilities register their own optional model-visible tools;
//! core execution only consults this interface, never memory implementations.
use crate::engine::tools::ToolContext;
use serde_json::Value;
use super::cognition;
pub fn definitions()->Vec<Value>{
 cognition::tools::definitions()
}
pub async fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Option<Result<Value,String>>{
 cognition::tools::execute(ctx,name,args).await
}
