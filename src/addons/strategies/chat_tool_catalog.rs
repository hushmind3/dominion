//! Lazy per-task tool catalog selection, schema limits, and read-only gates.
use super::*;

pub(super) fn parallel_read_admitted(
    call: &ToolCall, _session: &Value, _definitions: &[Value],
    strategies: &[Value], input: &str, completed: &[Value],
) -> bool {
    if !tool_is_read_only(&call.name) {
        return false;
    }
    let Ok(arguments) = serde_json::from_str::<Value>(&call.arguments) else { return false; };
    if !arguments.is_object() { return false; }
    !strategies.iter().any(|strategy| {
        (strategy["enforce"] == true || strategy["verification"]["enforce"] == true)
            && crate::engine::procedure::applies(strategy, input, Some(&call.name), Some(&arguments))
            && crate::engine::procedure::missing_requirement(
                strategy, &call.name, &arguments, completed,
            ).is_some()
    })
}

pub(super) fn tool_is_read_only(name: &str) -> bool {
    matches!(
        name,
        "workspace_list" | "workspace_read" | "workspace_read_range" | "file_list" | "file_read"
            | "web_search" | "http_fetch" | "memory_search"
            | "task_status" | "task_result" | "account_status" | "github_file_read"
            | "github_repositories"
    )
}

pub(super) fn mark_tool_argument_signal(
    state: &AppState, request: &TurnRequest, execution_id: &str, ok: bool,
) {
    let _ = crate::engine::algorithms::choose_for_agent(
        state,
        "tool.arguments",
        request.agent_id.as_deref(),
        Some(execution_id),
    );
    crate::engine::algorithms::mark_signal(state, execution_id, "tool.arguments", ok);
}

pub(super) use dominion_algorithms::tool_selection::{
 normalize_tool_arguments,tool_name,private_tool_request,room_tool_request,
 select_room_tools,select_tool_definitions,trim_tool_definitions,
};

#[cfg(test)]
mod private_tool_request_tests {
    use super::*;
    #[test]
    fn short_room_continuation_keeps_native_gateway_without_casual_schema_bloat() {
        assert!(room_tool_request("그럼 빨리해"));
        assert!(room_tool_request("바로 해"));
        assert!(!room_tool_request("안녕"));
        assert!(!room_tool_request("그거 빨리해?"));
        let definitions=vec![
            json!({"type":"function","function":{"name":"workspace_read",
                "description":"Read a file","parameters":{"type":"object"}}}),
            json!({"type":"function","function":{"name":"workspace_write",
                "description":"Write a file","parameters":{"type":"object"}}}),
        ];
        let selected=select_room_tools(definitions.clone(),"그럼 빨리해",
            &std::collections::HashSet::new(),false);
        assert!(!selected.is_empty());
        assert!(selected.iter().any(|tool|tool_name(tool)=="workspace_read"));
        assert!(select_room_tools(definitions,"안녕",
            &std::collections::HashSet::new(),false).is_empty());
    }
    #[test]
    fn plain_questions_bypass_schemas_but_explicit_actions_retain_tools() {
        for q in ["안녕","안녕하세요 반가워요",
            "러스트의 장점을 한 문장으로 알려줘","이 문장 영어로 번역해줘",
            "Rust 파일 시스템의 장점을 설명해줘"] {
            assert!(!private_tool_request(q),"{q}");
        }
        for q in ["src/main.rs 파일 읽어줘","웹에서 최신 자료 검색해줘",
            "러스트 테스트 돌려줘","메인","이어서 작업해",
            "깃허브에서 코드 수정해","검색 좀 해줘",
            "코드 리뷰해줘","파일 확인해",
            "workspace_list 도구로 작업 환경을 확인해","계속 확인",
            "그럼 빨리해","바로 해","마저 해","끝까지 해",
            "그거 해","처리해","전수점검해","얼른 해라"] {
            assert!(private_tool_request(q),"{q}");
        }
        for q in ["왜 빨리해?", "그걸 해?", "아까 빠르게 했나요?",
            "잘 부탁해", "Rust 장점을 설명해줘"] {
            assert!(!private_tool_request(q),"{q} should not expose work tools");
        }
    }
}

/// Casual room turns carry zero tool schemas. Action requests select the
/// exact native schemas the harness judges relevant; no model call is used.
pub(super) fn recent_room_tools(state: &AppState, session_id: &str) -> std::collections::HashSet<String> {
    // Session-scoped index lookup: no full-transcript JSON load on every turn.
    state.store.recent_message_tool_names(session_id, 30).unwrap_or_default()
}

/// Provider-visible progressive disclosure. The native dispatcher retains the
/// full, owner-enabled catalog; the provider sees a bounded shortlist and
/// stable discovery/search tools, never hundreds of MCP parameter schemas.
pub(super) fn progressive_tools(definitions: Vec<Value>, query: &str, pending_action: bool) -> Vec<Value> {
    if definitions.is_empty() { return definitions; }
    let mut selected = crate::engine::capability::definitions();
    let mut seen = selected.iter().map(tool_name)
        .map(str::to_string).collect::<std::collections::HashSet<_>>();
    let full = definitions.into_iter().filter(|definition|
        !matches!(tool_name(definition),"dominion_tool_search"|"dominion_tool_call")
    ).collect::<Vec<_>>();
    // Native core tools stay directly callable: the gateway is a token saver
    // for a long MCP tail, not a gate that hides file/shell/web tools from an
    // ordinary turn. Weak or local models call tools by their real names, and
    // a rejection there costs the whole turn.
    let picked = if full.len() <= 18 || pending_action {
        full.clone()
    } else {
        let mut short=Vec::new();
        // Basic navigation and evidence remain visible even with many MCPs.
        for name in ["workspace_list","workspace_read","workspace_read_range",
            "code_search","skill_view","shell_exec","web_search","delegate_task"] {
            if let Some(definition)=full.iter().find(|d|tool_name(d)==name) {
                short.push(definition.clone());
            }
        }
        for definition in select_room_tools(
            full.clone(),query,&std::collections::HashSet::new(),true
        ) {
            if !short.iter().any(|item|tool_name(item)==tool_name(&definition)) {
                short.push(definition);
            }
            if short.len()>=14 {break;}
        }
        short
    };
    for definition in picked {
        if seen.insert(tool_name(&definition).to_string()) {
            selected.push(definition);
        }
    }
    selected
}

