//! Progressive tool schema disclosure inside the existing chat/SDK/room
//! execution loop. Search is a read-only native observation, not a new agent
//! runner, approval authority, or persistent permission grant.
use crate::{engine::{addons,tools},state::AppState};
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};

pub(super) const SEARCH:&str="dominion_tool_search";
const THRESHOLD:usize=18;
const INITIAL:usize=12;
const MAX_PROMOTED:usize=20;

pub(super) fn definition()->Value{
    json!({"type":"function","function":{
        "name":SEARCH,
        "description":"Discover additional owner-enabled native, extension or MCP tools by intent. Search when a required tool is missing. The next model step receives the real tool schemas. Search never grants permissions or executes the discovered tool.",
        "parameters":{"type":"object","properties":{
            "query":{"type":"string","minLength":1,"maxLength":256,
                "description":"Needed capability, tool or service"},
            "limit":{"type":"integer","minimum":1,"maximum":8}
        },"required":["query"],"additionalProperties":false}
    }})
}

fn name(def:&Value)->&str{
    def.pointer("/function/name").and_then(Value::as_str).unwrap_or("")
}

fn words(query:&str)->HashSet<String>{
    let lower=query.to_lowercase();
    let mut words=lower.split(|ch:char|!ch.is_alphanumeric())
        .filter(|token|token.chars().count()>=2)
        .map(str::to_string).collect::<HashSet<_>>();
    for (needle,aliases) in [
        ("파일",&["workspace","file","read","write"][..]),
        ("코드",&["code","workspace","file"][..]),
        ("검색",&["search","fetch","web"][..]),
        ("조사",&["search","fetch","web"][..]),
        ("깃허브",&["github","repository"][..]),
        ("구글",&["google","drive","docs"][..]),
        ("메일",&["email","mail"][..]),
        ("문서",&["docs","document","file"][..]),
        ("브라우저",&["browser","control"][..]),
        ("시트",&["sheets","google"][..]),
        ("배포",&["deploy","publish"][..]),
        ("게시",&["publish","post"][..]),
        ("테스트",&["test","shell","terminal"][..]),
        ("위임",&["delegate","task"][..]),
        ("기억",&["memory","search"][..]),
        ("원문",&["history","evidence","read"][..]),
        ("이전",&["history","evidence"][..]),
        ("과거",&["history","evidence"][..]),
    ] {
        if lower.contains(needle){words.extend(aliases.iter().map(|item|item.to_string()));}
    }
    // The filename prefix "docs/" must not rank Google Docs tools.
    if (lower.contains("docs/") || lower.contains("docs\\"))
        && !lower.contains("google") && !lower.contains("구글"){
        words.remove("docs");
    }
    words
}

fn rank(def:&Value,query:&str,words:&HashSet<String>)->i32{
    let label=name(def).to_lowercase();
    let requested=query.to_lowercase();
    // A local docs/ path or generic "document" intent is not a
    // Google Docs/Drive authorization signal. Search still finds these
    // tools when the user names the cloud service explicitly.
    let google_intent=["google","구글","docs.google.com","drive.google.com",
        "sheets.google.com"].iter().any(|term|requested.contains(term));
    if label.starts_with("google_") && !google_intent{return -100;}
    let browser_intent=["browser","브라우저","웹","사이트","http://","https://",
        "screenshot","스크린샷","visit url","open url","페이지 이동"]
        .iter().any(|term|requested.contains(term));
    if label=="browser_control" && !browser_intent{return -100;}
    let summary=def.pointer("/function/description")
        .and_then(Value::as_str).unwrap_or("").to_lowercase();
    if label.is_empty(){return 0;}
    let mut score=if dominion_agent_sdk::plugin_protocol::explicitly_requested(
        name(def),def["function"]["description"].as_str().unwrap_or(""),query){80}else{0};
    if requested.contains(&label){score+=40;}
    for word in words{
        if label.split(|ch:char|!ch.is_alphanumeric()).any(|part|part==word.as_str()){score+=12;}
        else if label.contains(word.as_str()){score+=7;}
        else if summary.contains(word.as_str()){score+=2;}
    }
    score
}

/// Stable initial schemas, including read/edit/test essentials. No provider
/// call is spent on discovery unless the model actually needs another tool.
/// Existing tool permissions remain in the native dispatcher.
pub(super) fn project(definitions:Vec<Value>,query:&str)->Vec<Value>{
    if definitions.len()<=INITIAL{return definitions;}
    let needed=words(query);
    // Retain eight high-value primitives and let four slots respond to the
    // actual task. A fixed 12-tool core would hide an explicitly requested
    // connector on the first model step and waste a discovery round.
    let essentials=[
        "workspace_list","workspace_read","workspace_read_range","code_search",
        "workspace_write","workspace_apply_patch","shell_exec","web_search"
    ];
    let mut candidates=definitions.into_iter()
        .filter(|definition|!name(definition).is_empty())
        .collect::<Vec<_>>();
    // An unused slot is not permission to advertise an unrelated MCP
    // endpoint. Only stable native essentials or a positively ranked match
    // belong in the first provider request; all others remain searchable.
    let mut scoring=candidates.iter().enumerate().filter_map(|(index,definition)|{
        let base=if essentials.contains(&name(definition)){100}else{0};
        let score=base+rank(definition,query,&needed);
        (score>0).then_some((index,score))
    }).collect::<Vec<_>>();
    scoring.sort_by(|a,b|b.1.cmp(&a.1)
        .then_with(||name(&candidates[a.0]).cmp(name(&candidates[b.0]))));
    let selected=scoring.into_iter().take(INITIAL)
        .map(|(index,_)|index).collect::<HashSet<_>>();
    let mut projected=Vec::with_capacity(INITIAL+1);
    for (index,definition) in candidates.drain(..).enumerate(){
        if selected.contains(&index){projected.push(definition);}
    }
    // Discovery is attached by the turn coordinator only when its complete
    // execution policy has been frozen; projection itself grants nothing.
    projected
}

pub(super) fn should_enable(catalog_len:usize,initial_len:usize,
    simple_creation:bool,explicit_core:bool)->bool{
    catalog_len>THRESHOLD && initial_len>0 && !simple_creation && !explicit_core
}

/// Authoritative *current* tool registry for search. Rechecked whenever the
/// owner changes session capability settings or disconnects an MCP account.
pub(super) async fn catalog(state:&AppState,session_id:&str)->Vec<Value>{
    let Some(session)=state.store.get("session",session_id) else{return Vec::new()};
    let mut definitions=tools::definitions();
    if session["sdkUiPolicy"].is_null() || session["sdkUiPolicy"]["mcpEnabled"]==true{
        definitions.extend(addons::tool_definitions(state).await);
    }
    if session["sdkUiPolicy"].is_null()||session["sdkUiPolicy"]["extensionsEnabled"]==true{
        definitions.extend(crate::extension_manager::definitions(state));
    }
    tools::retain_connected_account_tools(state,&mut definitions);
    if session["parallelReadOnly"]==true || session["readOnly"]==true {
        definitions.retain(|definition|tools::parallel_read_only_tool(name(definition)));
    }
    if session["sdkUiPolicy"].is_object(){
        definitions.retain(|definition|
            tools::ui_tool_allowed(&session["sdkUiPolicy"],name(definition)));
    }
    let mut seen=HashSet::new();
    definitions.into_iter().filter(|definition|{
        let label=name(definition);
        !label.is_empty() && label!=SEARCH && seen.insert(label.to_string())
    }).collect()
}

fn search_catalog(catalog:&[Value],args:&Value)->Result<Value,String>{
    let query=args.get("query").and_then(Value::as_str)
        .map(str::trim).filter(|query|!query.is_empty()
            && query.chars().count()<=256)
        .ok_or("도구 검색어는 1~256자여야 합니다.")?;
    let limit=match args.get("limit"){
        None|Some(Value::Null)=>5usize,
        Some(value)=>value.as_u64().filter(|value|(1..=8).contains(value))
            .ok_or("도구 검색 결과 제한은 1~8이어야 합니다.")? as usize,
    };
    let tokens=words(query);
    let mut ordered=catalog.iter().map(|item|(rank(item,query,&tokens),item))
        .filter(|(score,_)|*score>0).collect::<Vec<_>>();
    ordered.sort_by(|a,b|b.0.cmp(&a.0)
        .then_with(||name(a.1).cmp(name(b.1))));
    let matches=ordered.into_iter().take(limit)
        .map(|(_,tool)|json!({
            "name":name(tool),
            "description":tool.pointer("/function/description")
                .and_then(Value::as_str).unwrap_or("")
                .chars().take(180).collect::<String>(),
            // The provider receives this exact schema on the following
            // round after promotion. Echoing it inside the search observation
            // duplicates potentially huge MCP input contracts in chat history.
            // Only identity/short summary belong to the discovery receipt.
            "schemaDeferred":true,
        })).collect::<Vec<_>>();
    Ok(json!({"query":query,"matches":matches,
        "availableCount":catalog.len(),
        "nextStep":"Call the discovered tool by its exact name in the next model step. Real tool schemas, normal approval, execution receipts and cancellation remain authoritative."}))
}

/// The same execution must explicitly advertise progressive discovery. A
/// guessed tool name, old execution ID, or a session with tools disabled
/// cannot turn a search into a new permission grant.
pub(super) async fn search(
    state:&AppState,session_id:&str,execution_id:&str,args:&Value
)->Result<Value,String>{
    let execution=state.store.get("execution",execution_id)
        .ok_or("도구 탐색 실행 기록이 없습니다.")?;
    if execution["sessionId"].as_str()!=Some(session_id)
        || execution["status"]!="running"
        || execution["progressiveToolDiscovery"]!=true{
        return Err("이 실행에서는 동적 도구 탐색이 활성화되지 않았습니다.".into());
    }
    let current=catalog(state,session_id).await;
    search_catalog(&current,args)
}

/// Post-search promotion changes *only* provider-visible schemas for later
/// rounds. No store permission or executable registration is changed.
pub(super) fn promote(
    visible:&mut Vec<Value>,catalog:&[Value],response:&Value,
)->usize{
    let Some(matches)=response["matches"].as_array() else{return 0};
    let lookup=catalog.iter().map(|def|(name(def),def))
        .collect::<HashMap<_,_>>();
    let mut already=visible.iter().map(|def|name(def).to_string())
        .collect::<HashSet<_>>();
    let mut added=0;
    for match_item in matches.iter().take(8){
        if visible.len()>=MAX_PROMOTED{break;}
        let Some(found)=match_item["name"].as_str().and_then(|key|lookup.get(key))
            else{continue;};
        if already.insert(name(found).to_string()){
            visible.push((**found).clone());
            added+=1;
        }
    }
    added
}

#[cfg(test)]
mod tests{
    use super::*;
    fn sample()->Vec<Value>{
        let mut out=(0..80).map(|index|json!({"type":"function","function":{
            "name":format!("mcp__unrelated_{index:03}"),
            "description":"An unrelated endpoint",
            "parameters":{"type":"object","properties":{"payload":{"type":"string"}}}
        }})).collect::<Vec<_>>();
        for label in ["workspace_read","workspace_write","workspace_list",
            "workspace_replace","workspace_apply_patch","shell_exec","web_search",
            "mcp__archive_publish","mcp__mail_search"]{
            out.push(json!({"type":"function","function":{
                "name":label,"description":format!("Execute {label}"),
                "parameters":{"type":"object"}
            }}));
        }
        out
    }
    #[test]
    fn projection_is_bounded_stable_and_keeps_real_native_tool_schemas(){
        let original=sample();
        let a=project(original.clone(),"코드 수정 테스트");
        let b=project(original.clone(),"코드 수정 테스트");
        assert_eq!(a,b);
        assert!(a.len()<=INITIAL);
        assert!(!a.iter().any(|item|name(item)==SEARCH));
        assert!(a.iter().any(|item|name(item)=="workspace_write"));
        assert!(!a.iter().any(|item|name(item)=="mcp__archive_publish"));
        let mail=project(original.clone(),"Search email messages");
        assert!(mail.iter().any(|item|name(item)=="mcp__mail_search"),
            "directly requested connected capabilities must not require a paid rediscovery step");
        assert!(serde_json::to_vec(&a).unwrap().len()*2
            <serde_json::to_vec(&original).unwrap().len());
    }
    #[test]
    fn unrelated_mcp_capabilities_never_fill_unused_initial_schema_slots(){
        let unrelated=sample();
        let visible=project(unrelated.clone(),"Fix src/lib.rs and run the Rust tests");
        assert!(visible.iter().all(|item|
            !name(item).starts_with("mcp__unrelated_")
                && name(item)!="mcp__archive_publish"));
        let wanted=project(unrelated,"Publish to mcp__archive_publish");
        assert!(wanted.iter().any(|item|name(item)=="mcp__archive_publish"),
            "an explicitly named owner-enabled endpoint remains available");
    }

    #[test]
    fn local_docs_paths_do_not_rank_cloud_schemas_in_native_discovery(){
        let mut catalog=sample();
        for name in ["google_docs_read","google_drive_search",
            "google_sheets_read","browser_control"]{
            catalog.push(json!({"type":"function","function":{
                "name":name,
                "description":"Read docs and documents using cloud tools",
                "parameters":{"type":"object"}
            }}));
        }
        let local=project(catalog.clone(),
            "Create docs/a.txt, docs/b.txt and SUMMARY.md in the workspace");
        for tool in ["google_docs_read","google_drive_search",
            "google_sheets_read","browser_control"]{
            assert!(!local.iter().any(|row|name(row)==tool),
                "local docs directory must not advertise {tool}");
        }
        let cloud=project(catalog.clone(),"Open Google Docs and read it");
        assert!(cloud.iter().any(|row|name(row)=="google_docs_read"));
        let search=search_catalog(&catalog,&json!({
            "query":"google docs read","limit":5
        })).unwrap();
        assert!(search["matches"].as_array().unwrap().iter()
            .any(|row|row["name"]=="google_docs_read"));
    }

    #[test]
    fn discovery_receipt_never_echoes_full_mcp_schema_but_promotes_exact_original(){
        let huge_description="large payload specification ".repeat(1200);
        let actual=json!({"type":"function","function":{
            "name":"mcp__invoice_upload",
            "description":"Upload an invoice after approval",
            "parameters":{"type":"object","properties":{
                "invoice":{"type":"string","description":huge_description},
                "secretContext":{"type":"string","description":"private schema detail"}
            },"required":["invoice"]}
        }});
        let catalog=vec![actual.clone()];
        let result=search_catalog(&catalog,&json!({
            "query":"mcp__invoice_upload","limit":1
        })).unwrap();
        let hit=&result["matches"][0];
        assert_eq!(hit["name"],"mcp__invoice_upload");
        assert_eq!(hit["schemaDeferred"],true);
        assert!(hit.get("parameters").is_none(),
            "discovery must not duplicate MCP argument schemas into tool results");
        assert!(!result.to_string().contains("secretContext"));
        assert!(result.to_string().len()<1000,
            "tool search is a compact capability card, not a schema transfer");
        let mut visible=Vec::new();
        assert_eq!(promote(&mut visible,&catalog,&result),1);
        assert_eq!(visible,vec![actual],
            "the provider must receive the exact original schema after promotion");
    }

    #[test]
    fn search_promotes_only_catalogued_schemas_and_limits_growth(){
        let all=sample();
        let mut visible=project(all.clone(),"코드 수정 테스트");
        let result=search_catalog(&all,&json!({
            "query":"archive publish","limit":3
        })).unwrap();
        assert!(result["matches"].as_array().unwrap().iter().any(|item|
            item["name"]=="mcp__archive_publish"));
        let before=visible.len();
        assert!(promote(&mut visible,&all,&result)>0);
        assert!(visible.len()>before && visible.len()<=MAX_PROMOTED);
        assert!(visible.iter().any(|item|name(item)=="mcp__archive_publish"));
        assert_eq!(promote(&mut visible,&all,&result),0);
        let forged=json!({"matches":[{"name":"mcp__not_installed"}]});
        assert_eq!(promote(&mut visible,&all,&forged),0);
        assert!(search_catalog(&all,&json!({"query":"","limit":9})).is_err());
        assert!(search_catalog(&all,&json!({"query":"x","limit":0})).is_err());
    }
    #[test]
    fn hidden_tool_must_be_discovered_and_promoted_before_batch_admission(){
        let catalog=sample();
        let mut visible=project(catalog.clone(),"Read source and run tests");
        let mut driver=dominion_turn_core::driver::Driver::new(4,8);
        assert!(!dominion_turn_core::driver::Driver::advertised(
            "mcp__archive_publish",&visible));
        assert_eq!(driver.admit_batch([("call-publish","mcp__archive_publish")],
            &visible,true),Err("tool was not advertised"));
        let found=search_catalog(&catalog,&json!({
            "query":"archive publish","limit":5
        })).unwrap();
        assert!(found["matches"].as_array().unwrap().iter().any(|hit|
            hit["name"]=="mcp__archive_publish"));
        assert_eq!(promote(&mut visible,&catalog,&found),1);
        assert!(driver.admit_batch([("call-publish","mcp__archive_publish")],
            &visible,true).is_ok(),
            "only the actual promoted and enabled schema can be executed");
        assert_eq!(driver.admit_batch([("call-hidden","mcp__unrelated_missing")],
            &visible,true),Err("tool was not advertised"));
    }

    #[test]
    fn explicitly_minimal_modes_do_not_activate_progressive_discovery(){
        assert!(!should_enable(90,10,false,true));
        assert!(!should_enable(90,0,false,false));
        assert!(!should_enable(90,10,true,false));
        assert!(!should_enable(18,10,false,false));
        assert!(should_enable(90,10,false,false));
        assert_eq!(project(sample()[..5].to_vec(),"anything").len(),5);
        assert!(project(sample()[..14].to_vec(),"anything").is_empty(),
            "unrelated MCP endpoints must stay in discovery, not consume provider schemas");
    }
}
