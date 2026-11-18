//! Progressive chat capability disclosure using the existing Rust host.
//! A search receipt belongs to ONE execution; a bridge call is resolved to
//! the real tool before the existing approval, effect and journal dispatcher.
use super::*;
use sha2::{Digest,Sha256};
use std::collections::HashSet;

pub(super) const SEARCH:&str="dominion_tool_search";
pub(super) const CALL:&str="dominion_tool_call";
const MAX_SCHEMA_BYTES:usize=12_288;

pub(super) fn definitions()->Vec<Value>{
    vec![
        json!({"type":"function","function":{
            "name":SEARCH,
            "description":"Search currently authorized tools by task intent. Returns a bounded exact schema and name. A search is read-only; results never grant new permissions.",
            "parameters":{"type":"object","properties":{
                "query":{"type":"string","minLength":1,"maxLength":256},
                "limit":{"type":"integer","minimum":1,"maximum":8}
            },"required":["query"],"additionalProperties":false}
        }}),
        json!({"type":"function","function":{
            "name":CALL,
            "description":"Call an exact tool returned by dominion_tool_search in THIS execution. Its schema, current connection, session scope, owner approval and effect journal are checked again.",
            "parameters":{"type":"object","properties":{
                "name":{"type":"string","minLength":1,"maxLength":128},
                "arguments":{"type":"object"}
            },"required":["name","arguments"],"additionalProperties":false}
        }})
    ]
}

fn permitted(session:&Value,name:&str)->bool{
    if name.is_empty() || matches!(name,SEARCH|CALL){return false;}
    if session["readOnly"]==true &&
        (name.starts_with("mcp__") ||
            !dominion_agent_sdk::runtime::builtin_is_read_only(name)){
        return false;
    }
    if session["delegationWorkspace"].is_object() &&
        (name.starts_with("mcp__") ||
            name.starts_with("github_") || name.starts_with("google_") ||
            matches!(name,"browser_control"|"account_status")){
        return false;
    }
    true
}
fn schema_of(definition:&Value)->Option<&Value>{
    let schema=&definition["function"]["parameters"];
    (schema.is_object()||schema.is_boolean()).then_some(schema)
}
fn schema_sha(schema:&Value)->String {
    format!("{:x}",Sha256::digest(schema.to_string().as_bytes()))
}
async fn authorized_catalog(state:&AppState,session_id:&str)->Vec<Value>{
    let Some(session)=state.store.get("session",session_id) else {return Vec::new()};
    // Re-read owner tool controls even for a previously discovered capability:
    // a room/session switched to tools-off must not keep an old grant alive.
    if session["toolsEnabled"]==false {return Vec::new()}
    if let Some(room_id)=session["roomId"].as_str(){
        if state.store.get("room",room_id)
            .is_some_and(|room|room["toolsEnabled"]==false){
            return Vec::new()
        }
    }
    let mut catalog=tools::definitions_for(state);
    catalog.extend(mcp::definitions(state).await);
    let mut seen=HashSet::new();
    catalog.into_iter().filter(|definition|{
        let name=tool_name(definition);
        permitted(&session,name) && schema_of(definition).is_some_and(|schema|
            schema.to_string().len()<=MAX_SCHEMA_BYTES)
            && seen.insert(name.to_string())
    }).collect()
}
fn rank<'a>(catalog:&'a [Value],query:&str,limit:usize)->Vec<&'a Value>{
    let mut candidates=catalog.iter().filter_map(|definition|{
        let name=tool_name(definition);
        let description=definition["function"]["description"].as_str().unwrap_or("");
        let score=dominion_agent_sdk::tool_relevance(name,description,query);
        (score>0).then_some((score,definition))
    }).collect::<Vec<_>>();
    candidates.sort_by(|a,b|b.0.cmp(&a.0).then_with(||tool_name(a.1).cmp(tool_name(b.1))));
    candidates.into_iter().take(limit.min(8)).map(|(_,definition)|definition).collect()
}
pub(super) fn should_expose(
    catalog_len:usize, selected_len:usize, action_request:bool,
    tools_enabled:bool, constrained_file_creation:bool
)->bool {
    tools_enabled && action_request && !constrained_file_creation
        && catalog_len>18 && selected_len<catalog_len
}

pub(super) async fn search(
    state:&AppState,session_id:&str,args:&Value
)->Result<Value,String>{
    let query=args["query"].as_str().map(str::trim)
        .filter(|q|!q.is_empty() && q.chars().count()<=256)
        .ok_or("Capability query must be 1-256 characters")?;
    let limit=match args.get("limit"){
        None|Some(Value::Null)=>5usize,
        Some(value)=>value.as_u64().filter(|n|(1..=8).contains(n))
            .ok_or("Capability limit must be 1-8")? as usize
    };
    let catalog=authorized_catalog(state,session_id).await;
    let matches=rank(&catalog,query,limit).into_iter().filter_map(|definition|{
        let schema=schema_of(definition)?;
        let name=tool_name(definition);
        Some(json!({
            "name":name,
            "description":definition["function"]["description"].as_str().unwrap_or("")
                .chars().take(180).collect::<String>(),
            "parameters":schema,
            "schemaSha256":schema_sha(schema),
            "requiresApproval": if name.starts_with("mcp__"){
                mcp::requires_approval(state,name)
            }else{tools::requires_approval(name)}
        }))
    }).collect::<Vec<_>>();
    Ok(json!({
        "query":query,"matches":matches,"availableCount":catalog.len(),
        "invocation":"Call dominion_tool_call with a name and arguments from these matches. Every call still needs ordinary approval and passes the existing effect journal."
    }))
}

/// Admission for every provider-authored tool call. The model cannot
/// synthesize a previously hidden native name to bypass progressive search.
/// Bridge calls are re-bound to their underlying real tool while preserving
/// the original provider call ID and message/receipt chain.
pub(super) async fn bind_model_call(
    state:&AppState,session_id:&str,execution_id:&str,
    offered:&HashSet<String>,call:&mut ToolCall
)->Result<(),String>{
    if !offered.contains(&call.name){
        return Err(format!("The tool '{}' was not offered in this model round. Discover it using dominion_tool_search.",call.name));
    }
    if call.name==CALL {
        let payload=serde_json::from_str::<Value>(&call.arguments)
            .map_err(|error|format!("Invalid discovery arguments: {error}"))?;
        let (real_name,real_arguments)=resolve(state,session_id,execution_id,&payload).await?;
        call.name=real_name;
        call.arguments=real_arguments.to_string();
    }
    Ok(())
}

pub(super) async fn resolve(
    state:&AppState,session_id:&str,execution_id:&str,args:&Value
)->Result<(String,Value),String>{
    let name=args["name"].as_str().filter(|name|
        !name.is_empty() && name.len()<=128 && !matches!(*name,SEARCH|CALL))
        .ok_or("Invalid discovered tool name")?;
    let arguments=args["arguments"].as_object()
        .ok_or("Discovered tool arguments must be an object")?;
    let execution=state.store.get("execution",execution_id)
        .ok_or("Missing execution record for discovered call")?;
    if execution["sessionId"].as_str()!=Some(session_id) ||
        execution["status"]!="running" {
        return Err("Discovered tool requires an active execution in the same session".into());
    }
    let operations=state.store.execution_operations(execution_id)?;
    let receipt=operations.iter().find_map(|operation|{
        if operation["status"]!="completed" || operation["tool"]!=SEARCH ||
            operation["origin"]!="model" ||
            operation["sessionId"].as_str()!=Some(session_id) {
            return None;
        }
        operation["result"]["matches"].as_array()?.iter().find(|result|
            result["name"].as_str()==Some(name)).cloned()
    }).ok_or("Call dominion_tool_search in this execution before invoking this tool")?;

    // Re-authorize against LIVE account/MCP/session state and reject changed
    // schemas rather than replaying stale capability evidence.
    let catalog=authorized_catalog(state,session_id).await;
    let definition=catalog.iter().find(|definition|tool_name(definition)==name)
        .ok_or("Discovered tool no longer authorized or connected")?;
    let schema=schema_of(definition).ok_or("Missing real tool schema")?;
    if receipt["schemaSha256"].as_str()!=Some(schema_sha(schema).as_str()){
        return Err("Discovered tool schema changed; search again".into());
    }
    let arguments=Value::Object(arguments.clone());
    dominion_agent_sdk::validate_arguments(schema,&arguments)
        .map_err(|error|format!("Tool arguments failed schema admission: {error}"))?;
    Ok((name.to_string(),arguments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!("sub3-chat-discovery-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.join("workspace")};
        std::fs::create_dir_all(&config.workspace).unwrap();
        let app=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        (app,root)
    }
    #[test]
    fn no_tools_for_casual_rooms_disabled_actions_or_single_file_fast_path(){
        assert!(!should_expose(40,6,false,true,false));
        assert!(!should_expose(40,6,true,false,false));
        assert!(!should_expose(40,6,true,true,true));
        assert!(!should_expose(15,6,true,true,false));
        assert!(!should_expose(40,40,true,true,false));
        assert!(should_expose(40,6,true,true,false));
    }
    #[tokio::test]
    async fn bridge_requires_same_run_receipt_and_exact_argument_schema(){
        let (state,root)=state();
        state.store.put("session",&json!({"id":"owner"})).unwrap();
        state.store.put("execution",&json!({"id":"run-a","sessionId":"owner","status":"running"})).unwrap();
        state.store.put("execution",&json!({"id":"run-b","sessionId":"owner","status":"running"})).unwrap();
        let results=search(&state,"owner",&json!({"query":"workspace read","limit":8})).await.unwrap();
        let found=results["matches"].as_array().unwrap().iter().any(|r|r["name"]=="workspace_read");
        assert!(found,"{results}");
        let good=json!({"name":"workspace_read","arguments":{"path":"README.md"}});
        assert!(resolve(&state,"owner","run-a",&good).await.is_err());
        state.store.put("execution_operation",&json!({
            "id":"found-a","runId":"run-a","sessionId":"owner",
            "status":"completed","origin":"model","tool":SEARCH,"result":results
        })).unwrap();
        assert!(resolve(&state,"owner","run-a",&good).await.is_ok());
        assert!(resolve(&state,"owner","run-b",&good).await.is_err());
        assert!(resolve(&state,"other","run-a",&good).await.is_err());
        assert!(resolve(&state,"owner","run-a",&json!({"name":"workspace_read","arguments":{}})).await.is_err());
        assert!(resolve(&state,"owner","run-a",&json!({
            "name":"workspace_write","arguments":{"path":"README.md","content":"unsafe"}
        })).await.is_err());
        state.store.patch_if("execution","run-a","status",&json!("running"),
            &json!({"status":"completed"})).unwrap();
        assert!(resolve(&state,"owner","run-a",&good).await.is_err());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn disabling_room_tools_revokes_a_previously_discovered_capability(){
        let (state,root)=state();
        state.store.put("room",&json!({"id":"room-one","toolsEnabled":true})).unwrap();
        state.store.put("session",&json!({"id":"owner","roomId":"room-one"})).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"owner","status":"running"})).unwrap();
        let matches=search(&state,"owner",&json!({"query":"workspace read","limit":8})).await.unwrap();
        assert!(matches["matches"].as_array().unwrap().iter().any(|r|r["name"]=="workspace_read"));
        state.store.put("execution_operation",&json!({
            "id":"granted","runId":"run","sessionId":"owner","status":"completed",
            "origin":"model","tool":SEARCH,"result":matches
        })).unwrap();
        let args=json!({"name":"workspace_read","arguments":{"path":"README.md"}});
        assert!(resolve(&state,"owner","run",&args).await.is_ok());
        state.store.put("room",&json!({"id":"room-one","toolsEnabled":false})).unwrap();
        assert!(search(&state,"owner",&json!({"query":"workspace read"}))
            .await.unwrap()["matches"].as_array().unwrap().is_empty());
        assert!(resolve(&state,"owner","run",&args).await.is_err());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn schema_digest_cannot_be_forged_to_skip_live_tool_admission(){
        let (state,root)=state();
        state.store.put("session",&json!({"id":"owner"})).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"owner","status":"running"})).unwrap();
        let mut matches=search(&state,"owner",&json!({"query":"workspace read","limit":8})).await.unwrap();
        for entry in matches["matches"].as_array_mut().unwrap(){
            entry["schemaSha256"]=json!("untrusted-old-schema");
        }
        state.store.put("execution_operation",&json!({
            "id":"forged","runId":"run","sessionId":"owner","status":"completed",
            "origin":"model","tool":SEARCH,"result":matches
        })).unwrap();
        assert!(resolve(&state,"owner","run",&json!({
            "name":"workspace_read","arguments":{"path":"README.md"}
        })).await.is_err());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn unseen_provider_tool_name_must_not_bypass_search_receipt(){
        let (state,root)=state();
        state.store.put("session",&json!({"id":"owner"})).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"owner","status":"running"})).unwrap();
        let offered=HashSet::from([SEARCH.to_string(),CALL.to_string()]);
        let mut injected=ToolCall{id:"call-injected".into(),name:"workspace_write".into(),
            arguments:json!({"path":"file.txt","content":"oops"}).to_string()};
        assert!(bind_model_call(&state,"owner","run",&offered,&mut injected).await.is_err());
        assert_eq!(injected.name,"workspace_write");
        let mut bridge=ToolCall{id:"call-bridge".into(),name:CALL.into(),
            arguments:json!({"name":"workspace_read","arguments":{"path":"file.txt"}}).to_string()};
        assert!(bind_model_call(&state,"owner","run",&offered,&mut bridge).await.is_err());
        let matches=search(&state,"owner",&json!({"query":"workspace read","limit":8})).await.unwrap();
        state.store.put("execution_operation",&json!({
            "id":"searched","runId":"run","sessionId":"owner","origin":"model",
            "status":"completed","tool":SEARCH,"result":matches
        })).unwrap();
        assert!(bind_model_call(&state,"owner","run",&offered,&mut bridge).await.is_ok());
        assert_eq!(bridge.name,"workspace_read");
        assert_eq!(bridge.id,"call-bridge");
        assert_eq!(serde_json::from_str::<Value>(&bridge.arguments).unwrap()["path"],"file.txt");
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn read_only_revocation_blocks_write_even_after_previous_search(){
        let (state,root)=state();
        state.store.put("session",&json!({"id":"owner"})).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"owner","status":"running"})).unwrap();
        let matches=search(&state,"owner",&json!({"query":"workspace write","limit":8})).await.unwrap();
        state.store.put("execution_operation",&json!({
            "id":"before","runId":"run","sessionId":"owner",
            "status":"completed","origin":"model","tool":SEARCH,"result":matches
        })).unwrap();
        let mut session=state.store.get("session","owner").unwrap();
        session["readOnly"]=json!(true);
        state.store.put("session",&session).unwrap();
        let results=search(&state,"owner",&json!({"query":"workspace write","limit":8})).await.unwrap();
        assert!(results["matches"].as_array().unwrap().iter().all(|r|
            r["name"]!="workspace_write"));
        assert!(resolve(&state,"owner","run",&json!({
            "name":"workspace_write","arguments":{"path":"a","content":"b"}
        })).await.is_err());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
}
