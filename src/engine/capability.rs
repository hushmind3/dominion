//! Progressive capability disclosure for the single native Rust harness.
//! Search is informational. Calls are bound to a completed search receipt in
//! the same execution, then use the existing effect/approval/dispatch gateway.
use crate::{engine::{mcp,tools},state::AppState};
use serde_json::{json,Value};
use std::collections::HashSet;

pub const SEARCH: &str = "dominion_tool_search";
pub const CALL: &str = "dominion_tool_call";

pub fn definitions() -> Vec<Value> {
    vec![
        json!({"type":"function","function":{
            "name":SEARCH,
            "description":"Discover available owner-enabled tools by intent. Search if a needed capability is missing; results include actual schemas for dominion_tool_call. Discovery never executes a tool.",
            "parameters":{"type":"object","properties":{
                "query":{"type":"string","minLength":1,"maxLength":256,"description":"Required capability or service"},
                "limit":{"type":"integer","minimum":1,"maximum":8}
            },"required":["query"],"additionalProperties":false}
        }}),
        json!({"type":"function","function":{
            "name":CALL,
            "description":"Execute a tool found by dominion_tool_search in this execution. Pass its exact name and the retrieved arguments schema. Original approvals, effects, cancellation and receipts apply.",
            "parameters":{"type":"object","properties":{
                "name":{"type":"string","minLength":1,"maxLength":128},
                "arguments":{"type":"object","description":"Arguments from the discovered tool schema"}
            },"required":["name","arguments"],"additionalProperties":false}
        }})
    ]
}

fn tool_name(definition:&Value)->&str {
    definition.pointer("/function/name").and_then(Value::as_str).unwrap_or("")
}

fn enabled_in_session(state:&AppState,session_id:&str,name:&str)->bool {
    if name.is_empty() || name==SEARCH || name==CALL {return false;}
    let session=state.store.get("session",session_id).unwrap_or(Value::Null);
    if !tools::profile_tool_visible(&session,name) {return false;}
    if session["readOnly"]==true && !tools::observational(name) {return false;}
    if session["delegationWorkspace"].is_object() &&
        (name.starts_with("mcp__") || matches!(name,
            "browser_control"|"github_api"|"github_repositories"|"github_file_read"|
            "github_file_update"|"github_file_replace")) {return false;}
    true
}

async fn catalog(state:&AppState,session_id:&str)->Vec<Value> {
    let mut all=tools::definitions();
    all.extend(mcp::definitions(state).await);
    let mut unique=HashSet::new();
    all.into_iter().filter(|definition|{
        let name=tool_name(definition);
        enabled_in_session(state,session_id,name) && unique.insert(name.to_string())
    }).collect()
}

fn terms(query:&str)->HashSet<String> {
    let mut words=query.to_lowercase().split(|c:char|!c.is_alphanumeric())
        .filter(|word|word.chars().count()>=2)
        .map(str::to_string).collect::<HashSet<_>>();
    let lower=query.to_lowercase();
    for (key,aliases) in [
        ("코드",&["code","file","workspace","github"][..]),
        ("파일",&["file","workspace","read","write"]),
        ("검색",&["search","web","fetch"]),
        ("조사",&["search","web","fetch"]),
        ("브라우저",&["browser","control"]),
        ("위임",&["delegate","task"]),
        ("깃허브",&["github","repository"]),
        ("영상",&["video","render"]),
        ("구글",&["google","drive"]),
        ("메일",&["mail","email"]),
        ("문서",&["document","file"]),
        ("테스트",&["test","shell"]),
    ] {
        if lower.contains(key) {
            words.extend(aliases.iter().map(|alias|alias.to_string()));
        }
    }
    words
}

fn score(definition:&Value,query:&str,words:&HashSet<String>)->i32{
    let name=tool_name(definition).to_lowercase();
    let description=definition.pointer("/function/description")
        .and_then(Value::as_str).unwrap_or("").to_lowercase();
    let mut score=0;
    if query.to_lowercase().contains(&name) {score+=32;}
    for term in words {
        if name.split(|c:char|!c.is_alphanumeric()).any(|part|part==term){score+=12;}
        else if name.contains(term){score+=7;}
        else if description.contains(term){score+=2;}
    }
    score
}

fn ranked<'a>(catalog:&'a [Value],query:&str)->Vec<(i32,&'a Value)> {
    let words=terms(query);
    let mut matches=catalog.iter().map(|definition|(
        score(definition,query,&words),definition
    )).filter(|(score,_)|*score>0).collect::<Vec<_>>();
    matches.sort_by(|a,b|b.0.cmp(&a.0).then_with(||
        tool_name(a.1).cmp(tool_name(b.1))));
    matches
}

pub async fn search(state:&AppState,session_id:&str,args:&Value)->Result<Value,String> {
    let query=args["query"].as_str().map(str::trim)
        .filter(|query|!query.is_empty()&&query.chars().count()<=256)
        .ok_or("도구 검색어는 1~256자여야 합니다.")?;
    let limit=match args.get("limit"){
        None|Some(Value::Null)=>5,
        Some(value)=>value.as_u64().filter(|n|(1..=8).contains(n))
            .ok_or("검색 결과 제한은 1~8이어야 합니다.")? as usize,
    };
    let all=catalog(state,session_id).await;
    let matches=ranked(&all,query).into_iter().take(limit).map(|(_,tool)|json!({
        "name":tool_name(tool),
        "description":tool.pointer("/function/description").and_then(Value::as_str)
            .unwrap_or("").chars().take(180).collect::<String>(),
        "parameters":tool.pointer("/function/parameters"),
        "requiresApproval":if tool_name(tool).starts_with("mcp__"){
            mcp::requires_approval(state,tool_name(tool))
        }else{tools::requires_approval(tool_name(tool))}
    })).collect::<Vec<_>>();
    Ok(json!({
        "query":query,"matches":matches,"availableCount":all.len(),
        "invocation":"Use dominion_tool_call with an exact discovered name and its arguments. All normal tool permissions and approvals remain in force."
    }))
}

/// Resolving a bridge call never executes anything. The dispatcher is the
/// sole place that starts effects, asks permission and records the real tool.
pub async fn resolve(
    state:&AppState,session_id:&str,execution_id:&str,args:&Value,
)->Result<(String,Value),String>{
    let name=args["name"].as_str().filter(|name|!name.is_empty()&&name.len()<=128)
        .ok_or("검색한 도구의 정확한 이름이 필요합니다.")?;
    let arguments=args["arguments"].as_object()
        .ok_or("도구 인수는 JSON 객체여야 합니다.")?;
    let execution=state.store.get("execution",execution_id)
        .ok_or("도구 호출에 연결된 실행 기록이 없습니다.")?;
    if execution["sessionId"].as_str()!=Some(session_id){
        return Err("다른 세션의 실행 기록으로 도구를 호출할 수 없습니다.".into());
    }
    let discovered=state.store.execution_operations(execution_id)?.iter().any(|operation|{
        operation["status"]=="completed" && operation["tool"]==SEARCH
            && operation["origin"]=="model"
            && operation["sessionId"].as_str()==Some(session_id)
            && operation["result"]["matches"].as_array().is_some_and(|matches|
                matches.iter().any(|item|item["name"].as_str()==Some(name)))
    });
    if !discovered {
        return Err("이 실행에서 검색한 적 없는 도구입니다. dominion_tool_search로 먼저 찾으세요.".into());
    }
    let all=catalog(state,session_id).await;
    let definition=all.iter().find(|definition|tool_name(definition)==name)
        .ok_or("도구가 더 이상 연결돼 있지 않거나 현재 세션에서 허용되지 않습니다.")?;
    let arguments=Value::Object(arguments.clone());
    let session=state.store.get("session",session_id).unwrap_or(Value::Null);
    if !tools::profile_tool_allowed(&session,name,&arguments) {
        return Err("검색된 도구라도 현재 위임 역할에 허용되지 않는 명령입니다.".into());
    }
    dominion_core::tool_schema::admit(
        &dominion_core::domain::ToolCall{
            id:"discovered-call".into(),name:name.to_string(),arguments:arguments.clone()
        },std::slice::from_ref(definition)
    ).map_err(|error|format!("검색한 도구의 인수 검증 실패: {}",error.reason))?;
    Ok((name.to_string(),arguments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-capability-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        (AppState::new(config,store,vault),dir)
    }
    #[tokio::test]
    async fn discovered_tool_needs_same_execution_receipt_and_real_schema() {
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"owner"}));
        state.store.put("execution",&json!({"id":"run-a","sessionId":"owner"}));
        state.store.put("execution",&json!({"id":"run-b","sessionId":"owner"}));
        let results=search(&state,"owner",&json!({"query":"workspace read"})).await.unwrap();
        assert!(results["matches"].as_array().unwrap().iter().any(|r|r["name"]=="workspace_read"));
        state.store.put("execution_operation",&json!({"id":"search-a","runId":"run-a",
            "sessionId":"owner","origin":"model","status":"completed","tool":SEARCH,
            "result":results}));
        let payload=json!({"name":"workspace_read","arguments":{"path":"README.md"}});
        assert!(resolve(&state,"owner","run-a",&payload).await.is_ok());
        assert!(resolve(&state,"owner","run-b",&payload).await.is_err());
        assert!(resolve(&state,"wrong","run-a",&payload).await.is_err());
        assert!(resolve(&state,"owner","run-a",&json!({"name":"workspace_read","arguments":{}})).await.is_err());
        assert!(resolve(&state,"owner","run-a",&json!({"name":"workspace_write","arguments":{"path":"p","content":"x"}})).await.is_err());
        let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn verification_profile_discovery_cannot_bypass_test_only_command_gate() {
        let (state,dir)=state();
        state.store.put("session",&json!({
            "id":"verifier","delegated":true,"delegationProfile":"verification",
            "delegationWorkspace":{"path":"subagent"}
        })).unwrap();
        state.store.put("execution",&json!({"id":"verify-run","sessionId":"verifier"})).unwrap();
        let results=search(&state,"verifier",
            &json!({"query":"terminal_exec","limit":1})).await.unwrap();
        assert_eq!(results["matches"][0]["name"],"terminal_exec");
        state.store.put("execution_operation",&json!({
            "id":"verify-search","runId":"verify-run","sessionId":"verifier",
            "origin":"model","status":"completed","tool":SEARCH,"result":results
        })).unwrap();
        let run=json!({"name":"terminal_exec",
            "arguments":{"command":"cargo","args":["run","--bin","script"]}});
        assert!(resolve(&state,"verifier","verify-run",&run).await.is_err());
        let test=json!({"name":"terminal_exec",
            "arguments":{"command":"cargo","args":["test","--locked"]}});
        assert!(resolve(&state,"verifier","verify-run",&test).await.is_ok());
        let write=json!({"name":"workspace_write",
            "arguments":{"path":"src/lib.rs","content":"malicious"}});
        assert!(resolve(&state,"verifier","verify-run",&write).await.is_err());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn read_only_session_never_discovers_mutating_tools(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"readonly","readOnly":true}));
        let results=search(&state,"readonly",&json!({"query":"workspace write read"})).await.unwrap();
        assert!(results["matches"].as_array().unwrap().iter().all(|r|r["name"]!="workspace_write"));
        assert!(results["matches"].as_array().unwrap().iter().all(|r|r["name"]!="shell_exec"));
        let _=std::fs::remove_dir_all(dir);
    }
}
