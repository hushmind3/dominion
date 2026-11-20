//! Progressive capability disclosure for the single native Rust harness.
//! Search is informational. Calls are bound to a completed search receipt in
//! the same execution, then use the existing effect/approval/dispatch gateway.
use crate::{engine::{extensions,tools},state::AppState};
use serde_json::{json,Value};
use std::collections::HashSet;
use sha2::{Digest,Sha256};

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

/// Search receipts bind the exact offered schema, not just a mutable tool name.
fn schema_digest(definition:&Value)->String {
    format!("{:x}",Sha256::digest(definition["function"]["parameters"].to_string().as_bytes()))
}

fn enabled_in_session(
    state:&AppState,session_id:&str,agent_id:Option<&str>,name:&str,
)->bool {
    if name.is_empty() || name==SEARCH || name==CALL {return false;}
    let session=state.store.get("session",session_id).unwrap_or(Value::Null);
    if !tools::profile_tool_visible(&session,name) {return false;}
    if !extensions::tool_allowed(state,session_id,
        agent_id.or_else(||session["agentId"].as_str()),name) {
        return false;
    }
    if session["readOnly"]==true && !tools::observational(name) {return false;}
    if session["delegationWorkspace"].is_object() &&
        (name.starts_with("mcp__") || matches!(name,
            "browser_control"|"github_api"|"github_repositories"|"github_file_read"|
            "github_file_update"|"github_file_replace")) {return false;}
    true
}

async fn catalog(state:&AppState,session_id:&str,agent_id:Option<&str>)->Vec<Value> {
    let Some(session)=state.store.get("session",session_id) else {return Vec::new()};
    if session["toolsEnabled"]==false {return Vec::new();}
    if let Some(room_id)=session["roomId"].as_str(){
        if state.store.get("room",room_id)
            .is_some_and(|room|room["toolsEnabled"]==false){return Vec::new();}
    }
    let all=extensions::tools::catalog(
        state,session_id,agent_id.or_else(||session["agentId"].as_str())).await;
    let mut unique=HashSet::new();
    all.into_iter().filter(|definition|{
        let name=tool_name(definition);
        enabled_in_session(state,session_id,agent_id,name) && unique.insert(name.to_string())
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

/// 2026-style deferred-tool ranking: IDF favors discriminative capabilities
/// instead of rewarding the same generic "search" description on every MCP.
fn tokens(text:&str)->Vec<String>{
    text.to_lowercase().split(|ch:char|!ch.is_alphanumeric())
        .filter(|word|word.chars().count()>=2)
        .map(str::to_string).collect()
}

fn ranked<'a>(catalog:&'a [Value],query:&str)->Vec<(f64,&'a Value)> {
    struct Document<'a> {
        definition:&'a Value,
        name:String,
        name_terms:Vec<String>,
        description_terms:Vec<String>,
        parameter_terms:Vec<String>,
        length:f64,
    }
    let mut query_terms=terms(query).into_iter().collect::<Vec<_>>();
    query_terms.sort_unstable();
    if query_terms.is_empty(){return Vec::new();}
    let documents=catalog.iter().map(|definition|{
        let name=tool_name(definition).to_lowercase();
        let name_terms=tokens(&name);
        let description_terms=tokens(definition.pointer("/function/description")
            .and_then(Value::as_str).unwrap_or(""));
        // Parameter *names* help distinguish otherwise similar tools without
        // indexing or copying large parameter descriptions into every search.
        let parameter_terms=definition.pointer("/function/parameters/properties")
            .and_then(Value::as_object)
            .map(|properties|properties.keys().flat_map(|key|tokens(key)).collect::<Vec<_>>())
            .unwrap_or_default();
        let length=(name_terms.len()+description_terms.len()+parameter_terms.len()).max(1) as f64;
        Document{definition,name,name_terms,description_terms,parameter_terms,length}
    }).collect::<Vec<_>>();
    if documents.is_empty(){return Vec::new();}
    let average_length=documents.iter().map(|doc|doc.length).sum::<f64>()
        /documents.len() as f64;
    let total=documents.len() as f64;
    // Compute corpus rarity once per query, not once for every candidate.
    // This keeps discovery linear in catalog size for a bounded query.
    let rarity=query_terms.iter().map(|term|{
        let df=documents.iter().filter(|candidate|
            candidate.name_terms.iter().any(|word|word==term)
            ||candidate.description_terms.iter().any(|word|word==term)
            ||candidate.parameter_terms.iter().any(|word|word==term)
        ).count() as f64;
        (1.0+(total-df+0.5)/(df+0.5)).ln()
    }).collect::<Vec<_>>();
    let mut ranked=Vec::new();
    let normalized_query=query.trim().to_lowercase();
    for doc in &documents {
        let mut score=if normalized_query==doc.name {100.0} else {0.0};
        if doc.name.len()>=5 && normalized_query.contains(&doc.name) {score+=20.0;}
        let length_norm=1.2*(0.25+0.75*doc.length/average_length);
        for (term,idf) in query_terms.iter().zip(rarity.iter()) {
            let name_hits=doc.name_terms.iter().filter(|word|*word==term).count() as f64;
            let description_hits=doc.description_terms.iter().filter(|word|*word==term).count() as f64;
            let parameter_hits=doc.parameter_terms.iter().filter(|word|*word==term).count() as f64;
            let mut frequency=8.0*name_hits+2.0*description_hits+parameter_hits;
            // Preserve useful partial identifier matches, but do not let
            // generic substrings dominate exact rare-term matches.
            if frequency==0.0 && term.chars().count()>=3 && doc.name.contains(term) {
                frequency=1.0;
            }
            if frequency>0.0 {
                score+=idf*2.2*frequency/(frequency+length_norm);
            }
        }
        if score>0.0 {ranked.push((score,doc.definition));}
    }
    ranked.sort_by(|a,b|b.0.total_cmp(&a.0).then_with(||
        tool_name(a.1).cmp(tool_name(b.1))));
    ranked
}

pub async fn search(state:&AppState,session_id:&str,args:&Value)->Result<Value,String> {
    search_for_agent(state,session_id,None,args).await
}

/// Search only capabilities admitted to the actual turn participant. Shared
/// room sessions have no single session.agentId, so the caller must propagate
/// its agent identity instead of silently falling back to room-wide grants.
pub async fn search_for_agent(
    state:&AppState,session_id:&str,agent_id:Option<&str>,args:&Value,
)->Result<Value,String> {
    let query=args["query"].as_str().map(str::trim)
        .filter(|query|!query.is_empty()&&query.chars().count()<=256)
        .ok_or("도구 검색어는 1~256자여야 합니다.")?;
    let limit=match args.get("limit"){
        None|Some(Value::Null)=>5,
        Some(value)=>value.as_u64().filter(|n|(1..=8).contains(n))
            .ok_or("검색 결과 제한은 1~8이어야 합니다.")? as usize,
    };
    let all=catalog(state,session_id,agent_id).await;
    let matches=ranked(&all,query).into_iter().take(limit).map(|(_,tool)|json!({
        "name":tool_name(tool),
        "description":tool.pointer("/function/description").and_then(Value::as_str)
            .unwrap_or("").chars().take(180).collect::<String>(),
        "parameters":tool.pointer("/function/parameters"),
        "schemaSha256":schema_digest(tool),
        "requiresApproval":if tool_name(tool).starts_with("mcp__"){
            extensions::tools::mcp_requires_approval(state,tool_name(tool))
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
    resolve_for_agent(state,session_id,None,execution_id,args).await
}

/// A tool search receipt is scoped to its originating agent as well as the
/// execution and schema. One room participant cannot borrow another agent's
/// discovery receipt or reveal tools denied by that agent's policy.
pub async fn resolve_for_agent(
    state:&AppState,session_id:&str,agent_id:Option<&str>,
    execution_id:&str,args:&Value,
)->Result<(String,Value),String>{
    let name=args["name"].as_str().filter(|name|!name.is_empty()&&name.len()<=128)
        .ok_or("검색한 도구의 정확한 이름이 필요합니다.")?;
    let arguments=args["arguments"].as_object()
        .ok_or("도구 인수는 JSON 객체여야 합니다.")?;
    let execution=state.store.get("execution",execution_id)
        .ok_or("도구 호출에 연결된 실행 기록이 없습니다.")?;
    if execution["sessionId"].as_str()!=Some(session_id) || execution["status"]!="running" {
        return Err("진행 중인 동일 세션 실행에서만 검색한 도구를 호출할 수 있습니다.".into());
    }
    let session=state.store.get("session",session_id).unwrap_or(Value::Null);
    let effective_agent_id=agent_id.or_else(||session["agentId"].as_str());
    let all=catalog(state,session_id,agent_id).await;
    let definition=all.iter().find(|definition|tool_name(definition)==name)
        .ok_or("도구가 더 이상 연결돼 있지 않거나 현재 세션에서 허용되지 않습니다.")?;
    let current_schema=schema_digest(definition);
    // A prior search receipt is evidence only for the exact schema offered.
    // A newly refreshed search can authorize a newer schema in the same run.
    let discovered=state.store.execution_operations(execution_id)?.iter().any(|operation|{
        operation["status"]=="completed" && operation["tool"]==SEARCH
            && operation["origin"]=="model"
            && operation["sessionId"].as_str()==Some(session_id)
            && operation["agentId"].as_str()==effective_agent_id
            && operation["result"]["matches"].as_array().is_some_and(|matches|
                matches.iter().any(|item|
                    item["name"].as_str()==Some(name)
                    && item["schemaSha256"].as_str()==Some(current_schema.as_str())))
    });
    if !discovered {
        return Err("이 도구의 현재 스키마로 다시 검색해야 합니다.".into());
    }
    let arguments=Value::Object(arguments.clone());
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
        state.store.put("execution",&json!({"id":"run-a","sessionId":"owner","status":"running"}));
        state.store.put("execution",&json!({"id":"run-b","sessionId":"owner","status":"running"}));
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
        state.store.put("execution",&json!({"id":"verify-run","sessionId":"verifier","status":"running"})).unwrap();
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
    #[test]
    fn discovery_prefers_rare_capability_over_generic_mcp_descriptions(){
        let mut definitions=(0..40).map(|index|json!({"type":"function","function":{
            "name":format!("mcp__common__search_{index}"),
            "description":"Search files and records in common integrations"
        }})).collect::<Vec<_>>();
        definitions.push(json!({"type":"function","function":{
            "name":"mcp__git__review_pull_request",
            "description":"Review a pull request and inspect its changed files"
        }}));
        let matches=ranked(&definitions,"review pull request");
        assert_eq!(tool_name(matches[0].1),"mcp__git__review_pull_request");
    }

    #[test]
    fn exact_tool_identifier_outweighs_verbose_generic_matches(){
        let definitions=vec![
            json!({"type":"function","function":{
                "name":"mcp__other__search","description":"github_file_update github_file_update tool"
            }}),
            json!({"type":"function","function":{
                "name":"github_file_update","description":"Update one file"
            }})
        ];
        let matches=ranked(&definitions,"github_file_update");
        assert_eq!(tool_name(matches[0].1),"github_file_update");
    }

    #[test]
    fn discovery_rank_is_stable_when_catalog_order_changes(){
        let mut definitions=vec![
            json!({"type":"function","function":{"name":"github_file_read","description":"read repository file"}}),
            json!({"type":"function","function":{"name":"github_file_update","description":"update repository file"}}),
            json!({"type":"function","function":{"name":"web_search","description":"search online"}}),
        ];
        let before=ranked(&definitions,"github file")
            .iter().map(|(_,d)|tool_name(d).to_string()).collect::<Vec<_>>();
        definitions.reverse();
        let after=ranked(&definitions,"github file")
            .iter().map(|(_,d)|tool_name(d).to_string()).collect::<Vec<_>>();
        assert_eq!(before,after);
        assert!(ranked(&definitions,"qzzyzx-no-such-tool").is_empty());
    }

    #[tokio::test]
    async fn stale_schema_receipt_and_finished_execution_cannot_dispatch(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"owner"})).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"owner","status":"running"})).unwrap();
        let good=search(&state,"owner",&json!({"query":"workspace read","limit":8}))
            .await.unwrap();
        let payload=json!({"name":"workspace_read","arguments":{"path":"README.md"}});
        let mut stale=good.clone();
        for result in stale["matches"].as_array_mut().unwrap(){
            if result["name"]=="workspace_read"{
                result["schemaSha256"]=json!("stale-schema");
            }
        }
        state.store.put("execution_operation",&json!({"id":"old-search","runId":"run",
            "sessionId":"owner","origin":"model","status":"completed","tool":SEARCH,
            "result":stale})).unwrap();
        assert!(resolve(&state,"owner","run",&payload).await.is_err());
        state.store.put("execution_operation",&json!({"id":"new-search","runId":"run",
            "sessionId":"owner","origin":"model","status":"completed","tool":SEARCH,
            "result":good})).unwrap();
        assert!(resolve(&state,"owner","run",&payload).await.is_ok());
        let mut run=state.store.get("execution","run").unwrap();
        run["status"]=json!("completed");
        state.store.put("execution",&run).unwrap();
        assert!(resolve(&state,"owner","run",&payload).await.is_err());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn shared_room_discovery_and_receipts_preserve_agent_permissions(){
        let (state,dir)=state();
        state.store.put("agent",&json!({"id":"restricted","extensions":{
            "memory":false,"skills":false
        }})).unwrap();
        state.store.put("agent",&json!({"id":"permitted"})).unwrap();
        state.store.put("room",&json!({"id":"shared-room"})).unwrap();
        state.store.put("session",&json!({"id":"shared","roomId":"shared-room"})).unwrap();
        state.store.put("execution",&json!({"id":"shared-run",
            "sessionId":"shared","status":"running"})).unwrap();
        let query=json!({"query":"memory_search","limit":8});
        let restricted=search_for_agent(&state,"shared",Some("restricted"),&query)
            .await.unwrap();
        assert!(restricted["matches"].as_array().unwrap().iter()
            .all(|item|item["name"]!="memory_search"));
        let permitted=search_for_agent(&state,"shared",Some("permitted"),&query)
            .await.unwrap();
        assert!(permitted["matches"].as_array().unwrap().iter()
            .any(|item|item["name"]=="memory_search"));
        state.store.put("execution_operation",&json!({
            "id":"permitted-search","runId":"shared-run","sessionId":"shared",
            "agentId":"permitted","origin":"model","status":"completed",
            "tool":SEARCH,"result":permitted
        })).unwrap();
        let call=json!({"name":"memory_search","arguments":{"query":"notes"}});
        assert!(resolve_for_agent(&state,"shared",Some("restricted"),
            "shared-run",&call).await.is_err());
        assert!(resolve_for_agent(&state,"shared",Some("permitted"),
            "shared-run",&call).await.is_ok());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn owner_room_tool_revocation_immediately_invalidates_search_receipts(){
        let (state,dir)=state();
        state.store.put("room",&json!({"id":"r","toolsEnabled":true})).unwrap();
        state.store.put("session",&json!({"id":"s","roomId":"r"})).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"s","status":"running"})).unwrap();
        let results=search(&state,"s",&json!({"query":"workspace read","limit":8}))
            .await.unwrap();
        assert!(results["matches"].as_array().unwrap().iter()
            .any(|tool|tool["name"]=="workspace_read"));
        state.store.put("execution_operation",&json!({"id":"search","runId":"run",
            "sessionId":"s","origin":"model","status":"completed","tool":SEARCH,
            "result":results})).unwrap();
        let payload=json!({"name":"workspace_read","arguments":{"path":"README.md"}});
        assert!(resolve(&state,"s","run",&payload).await.is_ok());
        let mut room=state.store.get("room","r").unwrap();
        room["toolsEnabled"]=json!(false);
        state.store.put("room",&room).unwrap();
        assert!(resolve(&state,"s","run",&payload).await.is_err());
        assert_eq!(search(&state,"s",&json!({"query":"workspace read"}))
            .await.unwrap()["matches"].as_array().unwrap().len(),0);
        let _=std::fs::remove_dir_all(dir);
    }

}
