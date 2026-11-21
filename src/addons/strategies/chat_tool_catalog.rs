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

pub(super) fn normalize_tool_arguments(mut arguments: Value, strategy: &str) -> Value {
    if strategy != "normalize-paths" {
        return arguments;
    }
    let Some(map) = arguments.as_object_mut() else {
        return arguments;
    };
    for key in ["path", "cwd", "file", "filename"] {
        let Some(raw) = map.get(key).and_then(Value::as_str) else {
            continue;
        };
        let mut normalized = raw.trim().replace('\\', "/");
        while normalized.contains("//") {
            normalized = normalized.replace("//", "/");
        }
        while normalized.starts_with("./") {
            normalized = normalized[2..].to_string();
        }
        if !normalized.is_empty() {
            map.insert(key.to_string(), Value::String(normalized));
        }
    }
    arguments
}

pub(super) fn tool_name(definition: &Value) -> &str {
    definition["function"]["name"]
        .as_str()
        .or_else(|| definition["name"].as_str())
        .unwrap_or("")
}

/// Private conversation does not need a schema catalog solely because the
/// user enabled tools. Actual tool actions still use the same native gateway.
pub(super) fn private_tool_request(input:&str)->bool {
    let q=input.trim().to_lowercase();
    if matches!(q.as_str(),"메인"|"main"|"계속"|"계속해"|"다음 단계"|"다음단계"){
        return true;
    }
    let compact=q.chars().filter(|ch|!ch.is_whitespace()).collect::<String>();
    let resource=["파일","폴더","코드","레포","저장소","프로젝트",
        "깃허브","github","브랜치","커밋","로그"]
        .iter().any(|term|q.contains(term));
    let inspection=["읽어","읽고","열어","검토","리뷰","확인","살펴","분석",
        "보여","진단"].iter().any(|term|q.contains(term));
    if resource && inspection{return true;}
    [
        "검색해","검색해서","검색좀","찾아줘","찾아봐","찾아보고",
        "조사해","조사해서","조회해","웹에서","인터넷에서",
        "깃허브에서","github.com","http://","https://",
        "파일 읽","파일을 읽","파일 보여","파일을 보여","파일 수정","파일을 수정",
        "파일 삭제","파일을 삭제","폴더를","폴더 만들",
        "코드 수정","코드를 수정","코드 고쳐","코드를 고쳐",
        "고쳐줘","수정해","수정하","삭제해","삭제하","만들어","만들어줘",
        "생성해","작성해","추가해","적용해","실행해","실행해줘",
        "테스트해","테스트 돌","빌드해","설치해","배포해",
        "열어줘","열어봐","클릭해","접속해","위임해",
        "이어서","이어가","이어 해","이어해","진행해","진행하",
        "다운로드해","저장해","기억해둬","기억해 둬",
        "read file","write file","edit file","run tests","search the web",
        "look up","browse to","open url","deploy","install package"
    ].iter().any(|mark|q.contains(mark)||compact.contains(
        &mark.chars().filter(|ch|!ch.is_whitespace()).collect::<String>()))
}
#[cfg(test)]
mod private_tool_request_tests {
    use super::*;
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
            "코드 리뷰해줘","파일 확인해"] {
            assert!(private_tool_request(q),"{q}");
        }
    }
}

/// Casual room turns carry zero tool schemas. Action requests select the
/// exact native schemas the harness judges relevant; no model call is used.
pub(super) fn room_tool_request(input: &str) -> bool {
    let lower = input.to_lowercase();
    [
        "파일", "폴더", "읽고", "읽어", "읽어줘", "읽어봐", "읽기",
        "쓰기", "써줘", "써 줘", "저장해", "저장하", "수정해", "수정하",
        "삭제해", "삭제하", "만들어", "만들어줘", "작성해", "작성하",
        "열어줘", "열어봐", "보여줘", "보여 줘", "실행", "명령", "터미널",
        "빌드", "테스트", "설치", "검색", "웹",
        "http", "url", "api", "코드", "버그", "도구", "계속", "이어", "마저",
        "결과", "가져와", "확인해", "진행해", "팀", "병렬", "협업", "team", "parallel",
        "스킬", "메모리", "브라우저", "깃허브", "github", "shell", "file", "read",
        "write", "run", "search", "fetch", "install", "build", "test", "tool",
        "browser", "memory", "delegate", "task",
    ]
    .iter()
    .any(|term| lower.contains(term))
}

pub(super) fn recent_room_tools(state: &AppState, session_id: &str) -> std::collections::HashSet<String> {
    // Session-scoped index lookup: no full-transcript JSON load on every turn.
    state.store.recent_message_tool_names(session_id, 30).unwrap_or_default()
}

pub(super) fn select_room_tools(
    definitions: Vec<Value>,
    query: &str,
    recent: &std::collections::HashSet<String>,
    acceptance: bool,
) -> Vec<Value> {
    if definitions.is_empty() {
        return Vec::new();
    }
    if !acceptance && !room_tool_request(query) {
        return Vec::new();
    }
    let lower = query.to_lowercase();
    let groups: &[(&[&str], &[&str])] = &[
        (
            &[
                "파일", "폴더", "읽고", "읽어", "읽어줘", "읽어봐", "읽기",
                "열어줘", "열어봐", "보여줘", "보여 줘", "read", "file",
            ],
            &["workspace_list", "workspace_read", "workspace_read_range", "file_list", "file_read"],
        ),
        (
            &[
                "쓰기", "써줘", "써 줘", "저장해", "저장하", "수정해", "수정하",
                "삭제해", "삭제하", "만들어", "만들어줘", "작성해", "작성하",
                "기록", "남겨", "write", "save", "edit", "delete", "create",
            ],
            &["workspace_read", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "file_write"],
        ),
        (
            &["실행", "명령", "터미널", "빌드", "테스트", "설치", "shell", "run", "build", "test", "install"],
            &["shell_exec", "terminal_exec"],
        ),
        (
            &["검색", "웹", "http", "url", "fetch", "search"],
            &["web_search", "http_fetch"],
        ),
        (
            &["메모리", "memory", "기억해둬", "기억해 둬", "저장해"],
            &["memory_store", "memory_search"],
        ),
        (&["스킬", "skill"], &["skill_propose"]),
        (
            &["위임", "delegate", "task", "에이전트"],
            &["delegate_task", "team_execute", "team_integrate",
                "task_integrate", "task_status", "task_result"],
        ),
        (
            &["팀", "협업", "병렬", "team", "parallel", "orchestrat", "multi-agent"],
            &["team_execute", "team_integrate", "task_status", "task_result"],
        ),
        (
            &["브라우저", "browser", "클릭", "click"],
            &["browser_control"],
        ),
        (&["github", "깃허브"], &["github_repositories","github_file_read","github_file_replace","github_file_update","github_api"]),
        (&["계정", "oauth", "account"], &["account_status"]),
    ];
    let mut scored: Vec<(i32, Value)> = definitions
        .iter()
        .filter_map(|definition| {
            let name = tool_name(definition);
            let name_lower = name.to_lowercase();
            let haystack = serde_json::to_string(definition)
                .unwrap_or_default()
                .to_lowercase();
            let mut score = 0i32;
            if lower.contains(&name_lower) {
                score += 30;
            }
            for token in lower
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|token| token.chars().count() > 1)
            {
                if haystack.contains(token) {
                    score += 2;
                }
            }
            for (terms, tools) in groups {
                if terms.iter().any(|term| lower.contains(term))
                    && tools.contains(&name_lower.as_str())
                {
                    score += 12;
                }
            }
            if recent.contains(&name_lower) {
                score += 8;
            }
            (score > 0).then(|| (score, definition.clone()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    if scored.is_empty() {
        if acceptance {
            // A requested action with no lexical match still needs a way to
            // inspect and execute. Sending every MCP schema instead can crowd
            // the provider's context before the first useful tool call.
            let fallback = [
                "workspace_list", "workspace_read", "workspace_write",
                "shell_exec", "web_search", "delegate_task",
                "task_status", "task_result",
            ];
            let selected = fallback
                .iter()
                .filter_map(|name| definitions.iter().find(|definition| tool_name(definition) == *name))
                .cloned()
                .collect::<Vec<_>>();
            return if selected.is_empty() {
                definitions.into_iter().take(8).collect()
            } else {
                selected
            };
        }
        return definitions
            .into_iter()
            .filter(|definition| {
                matches!(
                    tool_name(definition),
                    "workspace_list" | "workspace_read" | "workspace_read_range" | "workspace_replace_range" | "workspace_replace" | "workspace_write" | "shell_exec" | "web_search"
                )
            })
            .collect();
    }
    scored.truncate(8);
    scored.into_iter().map(|(_, definition)| definition).collect()
}

pub(super) fn select_tool_definitions(
    definitions: Vec<Value>,
    query: &str,
    algorithm: &Value,
) -> Vec<Value> {
    let strategy = algorithm["implementation"]
        .as_str()
        .or_else(|| algorithm["variant"].as_str())
        .unwrap_or("all");
    if strategy == "all" {
        return definitions;
    }
    let default_core = [
        "workspace_list", "workspace_read", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "shell_exec",
        "web_search", "memory_store", "memory_search", "delegate_task",
        "task_status", "task_result",
    ];
    let core = algorithm["parameters"]["tools"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_else(|| {
            default_core
                .iter()
                .map(|name| name.to_string())
                .collect::<std::collections::BTreeSet<_>>()
        });
    if strategy == "core" {
        return definitions
            .into_iter()
            .filter(|definition| core.contains(tool_name(definition)))
            .collect();
    }
    // query-relevant: always keep the core set, add the best matches.
    let tokens = query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let extra = algorithm["parameters"]["extra"].as_u64().unwrap_or(8) as usize;
    let mut matched = definitions
        .iter()
        .filter(|definition| !core.contains(tool_name(definition)))
        .map(|definition| {
            let haystack = serde_json::to_string(definition)
                .unwrap_or_default()
                .to_lowercase();
            let score = tokens
                .iter()
                .filter(|token| haystack.contains(token.as_str()))
                .count();
            (score, definition.clone())
        })
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| b.0.cmp(&a.0));
    let keep_extra = matched
        .into_iter()
        .take(extra)
        .map(|(_, definition)| definition)
        .collect::<Vec<_>>();
    definitions
        .into_iter()
        .filter(|definition| {
            core.contains(tool_name(definition))
                || keep_extra
                    .iter()
                    .any(|extra| tool_name(extra) == tool_name(definition))
        })
        .collect()
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

pub(super) fn trim_tool_definitions(definitions: Vec<Value>, budget: usize) -> Vec<Value> {
    let mut kept = Vec::new();
    let mut used = 0usize;
    for definition in definitions {
        let size = serde_json::to_string(&definition)
            .map(|text| text.len())
            .unwrap_or(0);
        if used + size > budget && !kept.is_empty() {
            continue;
        }
        used += size;
        kept.push(definition);
    }
    kept
}

