//! Optional MCP adapter. The SDK and package manager do not depend on MCP.
mod stdio;
pub use stdio::{StdioSession,read_stdio_frame};
use dominion_agent_sdk::extensions::{ExtensionRegistry,ExtensionTool,ToolFuture,ToolRegistration};
use serde_json::{json,Value};
fn valid_segment(name:&str)->bool {
    !name.is_empty() && name.len()<=28 &&
        name.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_')
        && !name.starts_with('_') && !name.ends_with('_') && !name.contains("__")
}
fn qualified(plugin:&str,tool:&str)->String { format!("ext__{plugin}__{tool}") }


/// Launch an explicitly trusted external MCP server, discover its actual
/// tools, and register adapters atomically. No app/server dependency.
/// Each tool shares the same initialized child process. Unregistering
/// drops all handlers and terminates the child.
pub async fn register_mcp_stdio(registry:&mut ExtensionRegistry,plugin:&str,record:&Value,
    workspace:&std::path::Path)->Result<Vec<String>,String>{
    if !valid_segment(plugin)||registry.contains_plugin(plugin){
        return Err("invalid or duplicate extension".into());
    }
    let mut session=crate::StdioSession::open(record,workspace).await?;
    let list=session.list_tools().await?;
    let tools=list["tools"].as_array().ok_or("MCP tools/list missing tools")?;
    let session=std::rc::Rc::new(tokio::sync::Mutex::new(session));
    let mut registrations=Vec::new();
    let mut names=Vec::new();
    for tool in tools{
        let original=tool["name"].as_str().ok_or("MCP tool name missing")?;
        let alias=original.to_ascii_lowercase().replace(['-', '.'], "_");
        if !valid_segment(&alias){return Err(format!("MCP tool name requires explicit adapter: {original}"));}
        names.push(qualified(plugin,&alias));
        registrations.push(ToolRegistration{
            name:alias,
            description:tool["description"].as_str().unwrap_or("").into(),
            parameters:tool["inputSchema"].clone(),
            handler:Box::new(McpHandler{session:session.clone(),name:original.into()}),
        });
    }
    registry.register(plugin,registrations)?;
    Ok(names)
}


struct McpHandler{
    session:std::rc::Rc<tokio::sync::Mutex<crate::StdioSession>>,
    name:String,
}
impl ExtensionTool for McpHandler{
    fn call<'a>(&'a mut self,arguments:&'a Value)->ToolFuture<'a>{
        Box::pin(async move{
            self.session.lock().await.request("tools/call",
                json!({"name":self.name,"arguments":arguments})).await
        })
    }
}

