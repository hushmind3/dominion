//! Executable memory/skills add-ons. Only this module may translate a
//! native tool request into long-term storage or reusable skill records.
//! The built-in tool dispatcher retains the same approval/journal entrypoint.
use super::{enabled,Extension};
use crate::{engine::{memory,tools::ToolContext},state::AppState};
use serde_json::{json,Value};

fn required(args:&Value,key:&str)->Result<String,String>{
    args.get(key).and_then(Value::as_str)
        .map(str::to_string).ok_or_else(||format!("Missing argument: {key}"))
}
fn scope(ctx:&ToolContext<'_>)->memory::MemoryScope {
    memory::MemoryScope::for_session(ctx.store,ctx.session_id,ctx.agent_id)
}

pub fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Result<Value,String>{
    let extension=match name {
        "memory_store"|"memory_search"=>Extension::Memory,
        "skill_view"|"skill_propose"=>Extension::Skills,
        _=>return Err("등록되지 않은 지식 부가기능 도구입니다.".into()),
    };
    // A provider may reuse a stale tool schema after an owner changed the
    // extension policy. Check at execution, not just before advertising.
    if !enabled(ctx.state,ctx.session_id,ctx.agent_id,extension){
        return Err("이 세션에서 해당 지식 부가기능이 비활성화되어 있습니다.".into());
    }
    match name {
        "memory_store"=>memory::store_tool(ctx.store,&scope(ctx),args),
        "memory_search"=>Ok(json!({
            "results":memory::retrieve(ctx.store,&scope(ctx),&required(args,"query")?,10)
        })),
        "skill_view"=>{
            let id=required(args,"id")?;
            let scope=scope(ctx);
            let record=ctx.store.all("skill").into_iter().find(|record|{
                (record["id"].as_str()==Some(id.as_str())
                    ||record["name"].as_str()==Some(id.as_str()))
                    &&record["enabled"]!=false&&record["status"]!="draft"
                    &&memory::visible(record,&scope)
            }).ok_or("활성화된 스킬을 찾을 수 없거나 접근 권한이 없습니다.")?;
            let instructions=record["instructions"].as_str()
                .or_else(||record["description"].as_str()).unwrap_or("");
            let lines:Vec<&str>=instructions.lines().collect();
            let start=args["startLine"].as_u64().unwrap_or(1).max(1) as usize;
            let count=args["maxLines"].as_u64().unwrap_or(100).clamp(1,160) as usize;
            if start>lines.len().max(1){
                return Err("스킬의 읽기 시작 줄이 전체 줄 수를 초과했습니다.".into());
            }
            let end=start.saturating_sub(1).saturating_add(count).min(lines.len());
            let excerpt=lines[start-1..end].join("\n");
            Ok(json!({
                "id":record["id"],"name":record["name"],"content":excerpt,
                "startLine":start,"endLine":end,"totalLines":lines.len(),
                "hasMore":end<lines.len(),
                "revision":memory::stable_id(&[
                    record["id"].as_str().unwrap_or(""),
                    instructions,record["updatedAt"].as_str().unwrap_or("")
                ])
            }))
        },
        "skill_propose"=>{
            let name=required(args,"name")?;
            let instructions=required(args,"instructions")?;
            let scope=scope(ctx);
            let record=ctx.store.put("skill",&json!({
                "name":name,"instructions":instructions,
                "description":instructions.chars().take(160).collect::<String>(),
                "enabled":false,"status":"draft","source":"agent",
                "scope":"agent","sessionId":ctx.session_id,"agentId":ctx.agent_id,
                "partitionKey":scope.partition_key,
                "createdAt":chrono::Utc::now().to_rfc3339()
            }));
            Ok(json!({"proposed":true,"skill":record}))
        },
        _=>unreachable!(),
    }
}

/// Kept as an independent function to allow admission tests without an
/// actual MCP server, memory extraction model or browser service.
pub fn tool_available(state:&AppState,session_id:&str,agent_id:Option<&str>,name:&str)->bool{
    super::Extension::from_tool(name)
        .is_some_and(|ext|enabled(state,session_id,agent_id,ext))
}
