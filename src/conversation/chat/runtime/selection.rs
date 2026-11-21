//! Tool catalog selection, read batches and effect ordering.
use super::*;

/// Limits parallel reads to a contiguous observation-only segment.
/// No batch can overtake a write, shell command or other effect.
pub(crate) fn adjacent_read_batch_end(calls: &[ToolCall], start: usize, max_batch: usize) -> usize {
    dominion_agent_sdk::runtime::read_batch_end(calls, start, max_batch, |call| {
        tool_is_read_only(&call.name)
    })
}

pub(super) fn parallel_read_admitted(
    call: &ToolCall, session: &Value, definitions: &[Value],
    strategies: &[Value], input: &str, completed: &[Value],
) -> bool {
    if !tool_is_read_only(&call.name)
 {
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
    dominion_agent_sdk::runtime::builtin_is_read_only(name)
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

/// Exact greetings need neither tools nor progressive tool-discovery
/// schemas. Do not generalize this to deictic followups ("그거 확인해",
/// "계속해") or requests with attachments; the turn host owns that context.
pub(super) fn pure_greeting(input:&str)->bool {
    let normalized=input.chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch,'!'|'?'|'.'|'~'|'。'))
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(normalized.as_str(),
        "안녕"|"안녕하세요"|"안녕하세요반가워요"|"반가워요"|
        "반갑습니다"|"좋은아침"|"좋은아침입니다"|
        "hi"|"hello"|"hey"|"thanks"|"thankyou"|
        "고마워"|"고마워요"|"감사합니다")
}

/// Only context-free explanatory requests can skip tool discovery safely.
/// A deictic follow-up, evidence/source lookup, recent/current fact or owner
/// work request must retain the existing authorized tool and history path.
pub(super) fn standalone_explanation(input:&str)->bool {
    let lower=input.trim().to_lowercase();
    if lower.len()>500 || lower.chars().count()<4 {return false;}
    const CONTEXT:&[&str]=&[
        "그거","그것","이거","이것","저거","위에서","앞에서","방금","아까",
        "전에","이전","이어","계속","다시","지난","현재","최신","오늘","지금",
        "실시간","출처","원문","원본","링크","근거","자료","첨부","내가",
        "우리","내 ","my ","our ","this ","that ","above","previous",
        "검색","찾아","조사","확인","읽어","수정","작성","저장","실행",
        "빌드","테스트","코드","파일","깃허브","github","브랜치",
        "url","http","계정","예약","메시지","업로드","전송",
        "원화","환율","가격","날씨","속보"
    ];
    if CONTEXT.iter().any(|word|lower.contains(word)) {return false;}
    ["뭐야","뭔가요","뭐예요","무엇인가","무엇인가요",
     "개념 설명","개념을 설명","원리 설명","뜻을 설명",
     "설명해줘","설명해주세요","어떤 뜻","무슨 뜻",
     "차이가 뭐","차이점 설명","what is ","explain ","define "]
        .iter().any(|word|lower.contains(word))
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
        "결과", "가져와", "확인해", "진행해",
        "위임", "에이전트", "delegate", "agent",
        "재시도", "복구", "실패", "오류", "retry", "recover", "failed", "failure", "error",
        "스킬", "메모리", "브라우저", "깃허브", "github", "구글", "google", "지메일", "gmail",
        "메일", "드라이브", "drive", "캘린더", "calendar", "일정", "docs", "shell", "file", "read",
        "write", "run", "search", "fetch", "install", "build", "test", "tool",
        "browser", "memory", "task",
    ]
    .iter()
    .any(|term| lower.contains(term))
}

pub(super) fn recent_room_tools(state: &AppState, session_id: &str) -> std::collections::HashSet<String> {
    state
        .store
        .messages(session_id)
        .into_iter()
        .rev()
        .take(30)
        .flat_map(|message| {
            message["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|call| {
            call["function"]["name"]
                .as_str()
                .map(|name| name.to_lowercase())
        })
        .collect()
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
            &[
                "실행", "명령", "터미널", "빌드", "테스트", "설치", "shell", "run", "build", "test", "install",
                "재시도", "복구", "실패", "오류", "retry", "recover", "failed", "failure", "error",
            ],
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
        (&["스킬", "skill"], &["skill_search","skill_read","skill_resource_read","skill_propose"]),
        (
            &["위임", "delegate", "task", "에이전트"],
            &["delegate_task", "task_status", "task_result"],
        ),
        (
            &["브라우저", "browser", "클릭", "click"],
            &["browser_control"],
        ),
        (&["github", "깃허브", "저장소", "브랜치", "PR", "이슈", "issue", "pull request"],
            &["github_work_read","github_work_write","github_file_read","github_file_commit","github_api"]),
        (&["구글","google","지메일","gmail","메일","이메일","드라이브","drive",
            "캘린더","calendar","일정","docs","문서"],
            &["google_workspace_read","google_workspace_write"]),
        (&["계정", "oauth", "account"], &["account_status"]),
    ];
    // Rank by index rather than cloning every (potentially large) JSON
    // tool schema, including candidates we will drop after top-k selection.
    let mut scored: Vec<(i32, usize)> = definitions
        .iter().enumerate()
        .filter_map(|(index, definition)| {
            let name = tool_name(definition);
            let name_lower = name.to_lowercase();
            let description=definition["function"]["description"].as_str()
                .or_else(||definition["description"].as_str()).unwrap_or("");
            let mut score=dominion_agent_sdk::tool_relevance(name,description,query);
            for (terms, tools) in groups {
                if terms.iter().any(|term| lower.contains(term))
                    && tools.contains(&name_lower.as_str())
                {
                    // An explicit owner action must survive the bounded
                    // shortlist even when generic description matches are
                    // stronger (e.g. "write files and run a command").
                    score += 100;
                }
            }
            if recent.contains(&name_lower) {
                score += 8;
            }
            (score > 0).then_some((score, index))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    if scored.is_empty() {
        if acceptance {
            return definitions;
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
    // Rank for membership, then return the permitted schemas in their
    // stable host catalogue order. Query/recent-tool scores can change on
    // every owner turn without changing the selected tool SET. Returning
    // score order would rewrite the provider's leading tool schema prefix
    // and unnecessarily invalidate every speaker's cached working view.
    scored.truncate(8);
    scored.sort_unstable_by_key(|(_, index)|*index);
    let mut available=definitions.into_iter().map(Some).collect::<Vec<_>>();
    scored.into_iter().filter_map(|(_,index)|available[index].take()).collect()
}

/// The historical default advertised the *entire* connected catalog on
/// every ordinary chat turn. For a large catalog, show a small task-relevant
/// set and retain SAME-RUN discover/call access to the remaining authorized
/// tools. This is a provider schema optimization, never a host permission
/// removal. An owner-activated full catalog remains an explicit choice.
pub(super) fn select_chat_tools(
    definitions: Vec<Value>, query: &str, algorithm: &Value,
) -> Vec<Value> {
    let legacy_full_default=(algorithm.is_null() || (
        algorithm["source"]=="builtin"
        && algorithm["implementation"].as_str()
            .or_else(||algorithm["variant"].as_str())==Some("all")
        && algorithm["ownerActivated"]!=true));
    if legacy_full_default && definitions.len()>18 {
        // Keep a compact set of useful read probes even when a request does
        // not contain a keyword such as "read" or "search". A model may
        // autonomously inspect context before answering; eliminating every
        // direct tool here would break legitimate existing agent behavior.
        if !room_tool_request(query) {
            return definitions.into_iter().filter(|definition|matches!(
                tool_name(definition),
                "workspace_list" | "workspace_read" | "memory_search" | "web_search"
            )).collect();
        }
        return select_room_tools(definitions,query,
            &std::collections::HashSet::new(),false);
    }
    select_tool_definitions(definitions,query,algorithm)
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
    // Query-relevant: reuse the SDK's bounded intent signal over tool
    // NAME/DESCRIPTION, not arbitrary JSON parameter text. The owner tool
    // catalog and configured core set remain authoritative.
    let extra = algorithm["parameters"]["extra"].as_u64().unwrap_or(8) as usize;
    let mut matched = definitions
        .iter().enumerate()
        .filter(|(_,definition)| !core.contains(tool_name(definition)))
        .map(|(index,definition)| {
            let score=dominion_agent_sdk::tool_relevance(
                tool_name(definition),
                definition["function"]["description"].as_str()
                    .or_else(||definition["description"].as_str()).unwrap_or(""),
                query,
            );
            (score, index)
        })
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| b.0.cmp(&a.0));
    let keep_extra=matched.into_iter().take(extra)
        .map(|(_,index)|index).collect::<std::collections::HashSet<_>>();
    // Retain original catalog ordering with O(1) membership checks rather
    // than comparing and cloning nested schema objects at each selection.
    definitions.into_iter().enumerate()
        .filter_map(|(index,definition)|
            (core.contains(tool_name(&definition))||keep_extra.contains(&index))
                .then_some(definition))
        .collect()
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


#[cfg(test)]
mod room_catalog_cache_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ordinary_chat_progressively_discloses_tools_without_dropping_host_access() {
        let mut tools=(0..30).map(|index| {
            let name=format!("mcp__unrelated_{index}");
            json!({"type":"function","function":{"name":name,
                "description":"Unrelated service endpoint","parameters":{"type":"object"}}})
        }).collect::<Vec<_>>();
        for name in ["workspace_read","web_search","shell_exec"] {
            tools.push(json!({"type":"function","function":{"name":name,
                "description":name,"parameters":{"type":"object"}}}));
        }
        let default=json!({"source":"builtin","implementation":"all"});
        let casual=select_chat_tools(tools.clone(),"안녕하세요",&default);
        assert_eq!(casual.iter().map(tool_name).collect::<Vec<_>>(),
            vec!["workspace_read","web_search"],
            "preserve spontaneous read/search ability without sending 33 full schemas");
        let searching=select_chat_tools(tools.clone(),"웹 검색해 주세요",&default);
        assert!(searching.len()<=8);
        assert!(searching.iter().any(|row|tool_name(row)=="web_search"));
        assert!(super::capability::should_expose(
            tools.len(),searching.len(),true,true,false),
            "the search/call bridge keeps the complete authorized catalog reachable");
        let explicit=json!({"source":"builtin","implementation":"all",
            "ownerActivated":true});
        assert_eq!(select_chat_tools(tools.clone(),"안녕하세요",&explicit),tools,
            "the owner can explicitly restore the full catalog");
    }

    #[test]
    fn korean_chat_github_intent_selects_connector_but_never_grants_it() {
        let mut tools=(0..40).map(|index| json!({
            "type":"function","function":{"name":format!("mcp__unrelated_{index}"),
                "description":"Unrelated provider operation",
                "parameters":{"type":"object"}}
        })).collect::<Vec<_>>();
        tools.push(json!({"type":"function","function":{
            "name":"github_work_read","description":"Read GitHub pull requests and issues",
            "parameters":{"type":"object","properties":{"operation":{"type":"string"}}}
        }}));
        tools.push(json!({"type":"function","function":{
            "name":"google_workspace_read","description":"Read Gmail and Calendar",
            "parameters":{"type":"object"}
        }}));
        let default=json!({"source":"builtin","implementation":"all"});
        let query="깃허브에서 열린 풀리퀘스트 번호들을 확인해 주세요";
        let selected=select_chat_tools(tools.clone(),query,&default);
        assert!(selected.iter().any(|tool|tool_name(tool)=="github_work_read"));
        assert!(!selected.iter().any(|tool|tool_name(tool)=="google_workspace_read"));
        assert!(selected.len()<=8);
        // Removing the connected account from the trusted input catalog makes
        // it unreachable; relevance is not a new account grant.
        tools.retain(|tool|tool_name(tool)!="github_work_read");
        let disconnected=select_chat_tools(tools,query,&default);
        assert!(!disconnected.iter().any(|tool|tool_name(tool)=="github_work_read"));
    }

    #[test]
    fn exact_greetings_skip_tools_without_hiding_followup_capabilities() {
        for input in ["안녕","안녕하세요 반가워요","안녕하세요!","hello","감사합니다"] {
            assert!(pure_greeting(input),"{input}");
        }
        for input in ["계속해","그거 확인해","안녕 파일 읽어줘",
            "안녕하세요. 어제 작업 이어서 해주세요","웹 검색해줘",
            "비용 계산해줘","이것 좀 설명해줘"] {
            assert!(!pure_greeting(input),"{input}");
        }
    }

    #[test]
    fn context_free_explanations_skip_unrelated_tools_without_losing_followups(){
        for question in ["양자 얽힘이 뭐야?","추상화 원리 설명해줘",
            "이론과 가설의 차이가 뭐야?","What is an event loop?"] {
            assert!(standalone_explanation(question),"{question}");
        }
        for request in ["그거 설명해줘","아까 원문 링크 설명해줘",
            "전에 하던 작업 계속해","오늘 최신 뉴스 설명해줘",
            "파일 구조 설명해줘","깃허브 브랜치 설명해줘",
            "업로드한 자료 설명해줘","내 계정 상태 설명해줘",
            "웹 검색해줘","그런데 어제 이야기 기억해?"] {
            assert!(!standalone_explanation(request),"{request}");
        }
    }

    #[test]
    fn room_tool_catalog_order_stays_stable_across_relevance_changes() {
        let tools=["shell_exec","workspace_write","workspace_read"]
            .into_iter().map(|name|json!({
                "type":"function","function":{"name":name,"description":name,
                    "parameters":{"type":"object","properties":{}}}
            })).collect::<Vec<_>>();
        let recent=["shell_exec","workspace_write","workspace_read"]
            .into_iter().map(str::to_string).collect();
        let read=select_room_tools(tools.clone(),"read file",&recent,false);
        let write=select_room_tools(tools.clone(),"write file",&recent,false);
        let names=|list:&[Value]|list.iter().map(|tool|tool_name(tool).to_owned()).collect::<Vec<_>>();
        assert_eq!(names(&read),vec!["shell_exec","workspace_write","workspace_read"]);
        assert_eq!(names(&write),names(&read),
            "relevance reranking alone must not change the model tool schema prefix");
        assert_eq!(names(&read),names(&tools));
    }
}
