//! Owner- and provider-independent tool-schema projection for each turn.
//! Discovery and selection expose only existing, approved tool authority.
//! The agent loop receives a bounded, stable catalog; it does not implement
//! OAuth, MCP catalog filtering, lazy skills or group peer visibility.
use super::*;

pub(super) struct Catalog{
    pub definitions:Vec<Value>,
    pub discovery:Vec<Value>,
    pub progressive_enabled:bool,
    pub simple_creation_target:Option<std::path::PathBuf>,
    pub is_room:bool,
}

pub(super) async fn prepare(
    state:&AppState,request:&TurnRequest,session:&Value,
    session_id:&str,execution_id:&str,
    acceptance:&Option<(String,Value)>,
    decision_policy:&crate::engine::turn_policy::TurnPolicy<'_>,
)->Result<Catalog,String>{
    let simple_creation_target=if !session["roomId"].is_string() {
        simple_file_creation(state,&request.input,&acceptance)
    }else{None};
    let mut tool_definitions = if request.tools_enabled && request.tools_execution_enabled {
        tools::definitions()
    } else {
        Vec::new()
    };
    if request.tools_enabled && request.tools_execution_enabled
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
        && (session["sdkUiPolicy"].is_null()
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
        progressive_enabled,simple_creation_target,is_room,
    })
}
