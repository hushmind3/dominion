//! Owner- and provider-independent tool-schema projection for each turn.
//! Discovery and selection expose only existing, approved tool authority.
//! The agent loop receives a bounded, stable catalog; it does not implement
//! OAuth, MCP catalog filtering, lazy skills or group peer visibility.
use super::*;

/// Strict one-call no-tool path for a new ordinary greeting or an inline
/// translation. Run this before enumerating installed MCP schemas/accounts:
/// tool discovery itself must not tax a request that cannot use any tools.
/// References, prior work, attachments and explicit file/web operations must
/// keep normal context and tool authority.
fn no_tools_required(request:&TurnRequest,session:&Value,acceptance:&Option<(String,Value)>)->bool{
    if !request.attachment_ids.is_empty() || acceptance.is_some()
        || session["roomId"].is_string() || session["teamUiOwnerJobId"].is_string()
        || request.parent_task_id.is_some() || request.task_id.is_some()
        || request.system_override.as_ref().is_some_and(|s|!s.trim().is_empty()){

        return false;
    }
    let text=request.input.trim().trim_matches(|ch:char|
        ch.is_whitespace() || matches!(ch,'.'|'!'|'?'|'。'|'！'|'？'|'~'))
        .trim().to_lowercase();
    if matches!(text.as_str(),
        "안녕"|"안녕하세요"|"안녕 반가워"|"안녕하세요 반가워요"|
        "반가워"|"반갑습니다"|"좋은 아침"|"좋은 아침입니다"|
        "고마워"|"감사합니다"|"고맙습니다"|
        "hello"|"hi"|"hey"|"good morning"|"thanks"|"thank you"){
        return true;
    }
    let inline_translation=text.chars().count()<=350
        && ["번역해","번역해줘","번역해 주세요","번역해주","translate "].iter()
            .any(|term|text.contains(term))
        && !["파일","문서","폴더","웹","검색","사이트","링크","url",
            "http","github","깃허브","이전","방금","앞에서","그거",
            "이거","저거","계속","이어","첨부","pdf","텍스트 파일",
            ".md",".txt",".docx",".html",".py",".rs",".json",
            "src/","docs/","/mnt/","/workspace/"]
            .iter().any(|term|text.contains(term));
    inline_translation
}

pub(super) struct Catalog{
    pub definitions:Vec<Value>,
    pub discovery:Vec<Value>,
    pub progressive_enabled:bool,
    pub simple_creation_target:Option<std::path::PathBuf>,
    pub lightweight_only:bool,
    pub is_room:bool,
}

pub(super) async fn prepare(
    state:&AppState,request:&TurnRequest,session:&Value,
    session_id:&str,execution_id:&str,
    acceptance:&Option<(String,Value)>,
    decision_policy:&crate::engine::turn_policy::TurnPolicy<'_>,
)->Result<Catalog,String>{
    // Direct Chat only exposes public read-only web capabilities, if asked.
    // Return before native/MCP/skill/peer discovery or any file fast path.
    if request.direct_chat {
        let lightweight_only=!request.web_search&&no_tools_required(request,session,acceptance);
        let definitions=if request.web_search&&request.tools_enabled
            && request.tools_execution_enabled {
            tools::definitions().into_iter().filter(|tool|
                matches!(tool_name(tool),"web_search"|"http_fetch"))
                .collect()
        }else{Vec::new()};
        return Ok(Catalog{
            definitions,discovery:Vec::new(),progressive_enabled:false,
            simple_creation_target:None,lightweight_only,is_room:false,
        });
    }
    let simple_creation_target=if request.tools_enabled && request.tools_execution_enabled
        && !session["roomId"].is_string() {
        simple_file_creation(state,&request.input,&acceptance)
    }else{None};
    let lightweight_only=no_tools_required(request,session,acceptance);
    let mut tool_definitions = if request.tools_enabled && request.tools_execution_enabled
        && !lightweight_only {
        tools::definitions()
    } else {
        Vec::new()
    };
    if request.tools_enabled && request.tools_execution_enabled && !lightweight_only
        && (session["sdkUiPolicy"].is_null() || session["sdkUiPolicy"]["mcpEnabled"]==true){
        tool_definitions.extend(crate::engine::addons::tool_definitions(state).await);
    }
    // Account-specific API tools require a real OAuth/CLI grant. A Google
    // login in our visible browser profile only grants browser_control UI
    // access; it must not be mistaken for a REST API bearer grant.
    tools::retain_connected_account_tools(state,&mut tool_definitions);
    // Only advertise the lazy skill loader when at least one approved,
    // visible skill exists for this participant. Its schema otherwise adds
    // tokens to every unrelated call, even if no skill could be read.
    let skill_loader=if request.tools_enabled && request.tools_execution_enabled
        && !lightweight_only && (session["sdkUiPolicy"].is_null()
            || session["sdkUiPolicy"]["skillsEnabled"]!=false){
        let scope=crate::engine::memory::MemoryScope::for_session(
            &state.store,session_id,request.agent_id.as_deref());
        state.store.all("skill").into_iter().any(|skill|
            skill["enabled"]!=false && skill["status"]!="draft"
            && crate::engine::memory::visible(&skill,&scope))
            .then(||tool_definitions.iter().find(|tool|tool_name(tool)=="skill_read").cloned())
            .flatten()
    }else{None};
    if skill_loader.is_none(){
        tool_definitions.retain(|tool|tool_name(tool)!="skill_read");
    }
    let is_room = session["roomId"].is_string();
    // Source-addressed peer reads are useful for exact group recall and
    // synthesis, but a tool schema on every casual room greeting wastes
    // provider input. Keep the capability available at the native executor;
    // only advertise it when the owner's request can need full peer sources.
    let peer_reader=if is_room && request.tools_enabled && request.tools_execution_enabled
        && session["roomId"].as_str()
            .and_then(|id|state.store.get("room",id))
            .is_some_and(|room|room["agentIds"].as_array().is_some_and(|members|members.len()>=5))
        && ["원문","인용","발언","각자","누가","기억","종합","전체 의견",
            "quote","verbatim","recall","who said","all agents","each agent"]
            .iter().any(|word|request.input.to_lowercase().contains(word))
    {
        tool_definitions.iter().find(|tool|tool_name(tool)=="room_message_read").cloned()
    }else{None};
    if peer_reader.is_none(){
        tool_definitions.retain(|tool|tool_name(tool)!="room_message_read");
    }
    // Freeze a same-turn provider-catalog ceiling *after* connection
    // filtering, then apply the same user and read-only restrictions that
    // the native dispatcher enforces. Search may not promote outside this.
    let mut discovery_catalog=tool_definitions.clone();
    if session["parallelReadOnly"]==true || session["readOnly"]==true {
        discovery_catalog.retain(|definition|
            tools::parallel_read_only_tool(tool_name(definition)));
    }
    if session["sdkUiPolicy"].is_object() {
        discovery_catalog.retain(|definition|
            tools::ui_tool_allowed(&session["sdkUiPolicy"],tool_name(definition)));
    }
    if is_room && request.tools_enabled && request.tools_execution_enabled {
        let recent = recent_room_tools(state, session_id);
        tool_definitions =
            select_room_tools(tool_definitions, &request.input, &recent, acceptance.is_some());
    }
    let discovery = decision_policy.discovery(is_room);
    if !is_room {
        tool_definitions = select_tool_definitions(tool_definitions, &request.input, &discovery);
    }
    if let Some(path)=&simple_creation_target{
        // A bounded single-file goal needs only file and shell capabilities.
        // Avoid sending unrelated web, memory, MCP and delegation schemas.
        let root=state.config.workspace.canonicalize()
            .unwrap_or_else(|_|state.config.workspace.clone());
        let inside=path.starts_with(&root);
        tool_definitions=tools::definitions().into_iter().filter(|definition|{
            let name=tool_name(definition);
            matches!(name,"shell_exec"|"terminal_exec") || if inside{
                matches!(name,"workspace_list"|"workspace_read"|"workspace_write")
            }else{
                matches!(name,"file_list"|"file_read"|"file_write")
            }
        }).collect();
        if !request.tools_enabled || !request.tools_execution_enabled{
            tool_definitions.clear();
        }
        decision_policy.unrated("tool.discovery","single-file creation");
        if let Some(mut execution)=state.store.get("execution",execution_id){
            execution["contextPolicy"]=json!("single-file-creation");
            state.store.put("execution",&execution);
        }
    }
    if session["parallelReadOnly"]==true {
        tool_definitions.retain(|definition|tools::parallel_read_only_tool(tool_name(definition)));
    }
    if !session["sdkUiPolicy"].is_null(){
        tool_definitions.retain(|definition|
            tools::ui_tool_allowed(&session["sdkUiPolicy"],tool_name(definition)));
        if simple_creation_target.is_none() {
            // Home NativeAgentPanel is an ordinary task interface.
            // Keep task-critical capabilities but do not spend
            // the first provider call listing every unrelated native tool.
            tool_definitions=select_panel_tool_definitions(tool_definitions,&request.input);
        }
    }
    // Discovery is a view over the same owner-approved registry, never a
    // new execution engine. Explicit core/query-relevant modes and single-
    // file fast paths preserve their previous tool declarations unchanged.
    let explicitly_scoped=!discovery.is_null()
        && !matches!(
            discovery["implementation"].as_str()
                .or_else(|| discovery["variant"].as_str()),
            None|Some("all")
        );
    let progressive_enabled=capability::should_enable(
        discovery_catalog.len(),tool_definitions.len(),
        simple_creation_target.is_some(),explicitly_scoped);
    if progressive_enabled {
        tool_definitions=capability::project(tool_definitions,&request.input);
        // Keep the search schema first even when provider tool budgets trim
        // large optional declarations later in the same turn.
        tool_definitions.insert(0,capability::definition());
        if let Some(mut execution)=state.store.get("execution",execution_id){
            execution["progressiveToolDiscovery"]=json!(true);
            state.store.put("execution",&execution)
                .ok_or("도구 탐색 설정을 실행 기록에 저장하지 못했습니다.")?;
        }
    }
    // Selectors may omit the loader because the current owner message does
    // not mention a skill by name. Keep this one read-only capability when
    // the context index lists active skills, or lazy-loading would be a dead
    // end. It still respects native SDK/owner policy and the single-file
    // minimal-tool path.
    if simple_creation_target.is_none() {
        if let Some(loader)=skill_loader {
            if !tool_definitions.iter().any(|tool|tool_name(tool)=="skill_read") {
                tool_definitions.push(loader);
            }
        }
    }
    if let Some(loader)=peer_reader{
        if request.tools_enabled && request.tools_execution_enabled
            && simple_creation_target.is_none()
            && (session["sdkUiPolicy"].is_null()
                || tools::ui_tool_allowed(&session["sdkUiPolicy"],"room_message_read"))
            && !tool_definitions.iter().any(|tool|tool_name(tool)=="room_message_read"){
            tool_definitions.push(loader);
        }
    }
    // Stable tool schema serialization is a provider cache boundary: MCP
    // discovery and skill registration may complete in different orders.
    tool_definitions=canonical_tool_definitions(tool_definitions);
    Ok(Catalog{
        definitions:tool_definitions,discovery:discovery_catalog,
        progressive_enabled,simple_creation_target,lightweight_only,is_room,
    })
}

#[cfg(test)]
mod lightweight_tests{
    use super::*;
    use serde_json::json;
    fn request(input:&str)->TurnRequest{
        TurnRequest::from_value(&json!({"input":input,"toolsEnabled":true,
            "toolsExecutionEnabled":true})).unwrap()
    }
    #[test]
    fn ordinary_greetings_and_inline_translation_have_no_model_tool_schemas(){
        let session=json!({"id":"chat-1"});
        for text in ["안녕","안녕하세요 반가워요","안녕하세요!","hello",
            "Translate this to Korean: Nice to meet you",
            "The sky is blue를 한국어로 번역해줘"]{
            assert!(no_tools_required(&request(text),&session,&None),"{text}");
        }
    }
    #[tokio::test]
    async fn new_chat_greeting_is_zero_schema_before_mcp_enumeration_and_action_keeps_tools(){
        let dir=std::env::temp_dir().join(format!("dominion-lightweight-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=crate::config::Config{port:0,data_dir:dir.join("data"),
            web_dir:dir.join("web"),workspace:dir.join("workspace")};
        let state=crate::state::AppState::new(cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()));
        let session=state.store.put("session",&json!({"id":"new-chat"})).unwrap();
        state.store.put("execution",&json!({"id":"exec-new","status":"running"}));
        let policy=crate::engine::turn_policy::TurnPolicy::new(&state,None,"exec-new");
        let hello=request("안녕");
        let light=prepare(&state,&hello,&session,"new-chat","exec-new",&None,&policy).await.unwrap();
        let prompt=super::super::context::system_prompt_with_manifest_at(
            &state,&hello,"new-chat",&hello.input,None,Some("exec-new"));
        let tokens=super::super::room_context::estimate(&prompt.prompt);
        assert!(light.definitions.is_empty(),"an ordinary greeting has no model-visible tool schemas");
        assert!(light.discovery.is_empty(),"no MCP enumeration on a greeting");
        eprintln!("SUB2_GREETING_MEASUREMENT old_schema_count=13 old_schema_tokens_est=1607 \
            current_schema_count={} current_schema_tokens_est=0 system_prompt_tokens_est={} \
            (model-independent estimator; no paid provider request)",light.definitions.len(),tokens);
        let work=request("Read README.md and run tests");
        let real=prepare(&state,&work,&session,"new-chat","exec-new",&None,&policy).await.unwrap();
        assert!(real.definitions.iter().any(|tool|tool_name(tool)=="workspace_read"),
            "explicit file work must retain real tool schemas");
        assert!(real.definitions.iter().any(|tool|tool_name(tool)=="shell_exec"),
            "explicit test work must retain the approved Rust executable port");
        let web_only=TurnRequest::from_value(&json!({
            "input":"올해 국내 반도체 업계 동향을 웹에서 찾아줘",
            "conversationMode":"chat","webSearch":true,
            "toolsEnabled":true,"toolsExecutionEnabled":true
        })).unwrap();
        let web=prepare(&state,&web_only,&session,"new-chat","exec-new",&None,&policy).await.unwrap();
        let names=web.definitions.iter().map(tool_name).collect::<Vec<_>>();
        assert_eq!(names.len(),2,"only public web search and page fetch are available");
        assert!(names.contains(&"web_search")&&names.contains(&"http_fetch"),"{names:?}");
        assert!(web.discovery.is_empty(),"web-only direct Chat must not discover MCP or skills");
        assert!(web.simple_creation_target.is_none(),"web-only Chat must never prepare file writes");
        let mut blocked=request("https://example.org 읽어");
        blocked.tools_enabled=false;
        let no_tool=prepare(&state,&blocked,&session,"new-chat","exec-new",&None,&policy).await.unwrap();
        assert!(no_tool.definitions.is_empty(),"disabled native tools stay disabled even for URLs");
        drop(state);let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn owner_work_sources_and_attached_or_room_requests_keep_tools(){
        let session=json!({"id":"chat-1"});
        for text in ["이어서 해","방금 파일 번역해","README.md 읽고 번역해줘",
            "안녕, 웹에서 검색해","그거 번역해","이거 번역해","안녕 지난 작업 계속해"]{
            assert!(!no_tools_required(&request(text),&session,&None),"{text}");
        }
        let attachment=TurnRequest::from_value(&json!({
            "input":"안녕","attachmentIds":["00000000-0000-4000-8000-000000000001"]
        })).unwrap();
        assert!(!no_tools_required(&attachment,&session,&None));
        assert!(!no_tools_required(&request("안녕"),&json!({"roomId":"r"}),&None));
        assert!(!no_tools_required(&request("안녕"),&session,
            &Some(("work".into(),json!([])))));
    }
}

