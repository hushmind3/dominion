//! Optional tool providers. The executor knows only native tool calls and
//! effect receipts; MCP server enumeration and transport live behind this gate.
use super::{enabled,tool_allowed,Extension};
use crate::{engine::{mcp,tools},state::AppState};
use serde_json::Value;

pub fn native(
    state:&AppState,session_id:&str,agent_id:Option<&str>
)->Vec<Value>{
    tools::definitions().into_iter().filter(|tool|
        tool.pointer("/function/name").and_then(Value::as_str)
            .is_some_and(|name|tool_allowed(state,session_id,agent_id,name))
    ).collect()
}

pub async fn external(
    state:&AppState,session_id:&str,agent_id:Option<&str>
)->Vec<Value>{
    if !enabled(state,session_id,agent_id,Extension::Mcp){
        return Vec::new();
    }
    mcp::definitions(state).await
}

pub async fn catalog(
    state:&AppState,session_id:&str,agent_id:Option<&str>
)->Vec<Value>{
    let mut selected=native(state,session_id,agent_id);
    selected.extend(external(state,session_id,agent_id).await);
    selected
}

pub async fn invoke_mcp(
    state:&AppState,session_id:&str,agent_id:Option<&str>,
    name:&str,arguments:&Value,
)->Result<Value,String>{
    if !name.starts_with("mcp__") {return Err("MCP 실행 경로가 아닙니다.".into());}
    if !enabled(state,session_id,agent_id,Extension::Mcp){
        return Err("이 세션에서 MCP 부가기능이 비활성화되어 있습니다.".into());
    }
    mcp::call(state,name,arguments).await
}
pub fn mcp_requires_approval(state:&AppState,name:&str)->bool{
    mcp::requires_approval(state,name)
}
