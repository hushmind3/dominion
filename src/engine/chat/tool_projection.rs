//! Bounded model-facing tool schema projection shared by room, Home and native
//! agent execution. Selection never grants tool permissions: execution policy
//! is still enforced by the Rust dispatcher / approvals.
use crate::state::AppState;
use serde_json::Value;

pub(super) fn tool_name(definition: &Value) -> &str {
    definition["function"]["name"]
        .as_str()
        .or_else(|| definition["name"].as_str())
        .unwrap_or("")
}

/// Casual room turns carry zero tool schemas. Action requests select the
/// exact native schemas the harness judges relevant; no model call is used.
/// Keep the model-facing catalog free of undefined/malformed tool calls.
/// Adapter-specific authorization remains in the native tool policy layer.
fn valid_tool_definition(definition: &Value) -> bool {
    let name=tool_name(definition);
    !name.is_empty() && name.len()<=256
        && !name.chars().any(|c|c.is_control() || c.is_whitespace())
}

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
        "스킬", "메모리", "브라우저", "깃허브", "github", "shell", "file", "read",
        "write", "run", "search", "fetch", "install", "build", "test", "tool",
        "browser", "memory", "delegate", "task",
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
    let definitions=definitions.into_iter().filter(valid_tool_definition).collect::<Vec<_>>();
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
            &["workspace_read", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "workspace_apply_patch", "file_write"],
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
            &["delegate_task", "task_status", "task_result"],
        ),
        (
            &["브라우저", "browser", "클릭", "click"],
            &["browser_control"],
        ),
        (&["github", "깃허브"], &["github_api"]),
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
        // A verifiable room action needs the normal read/edit/run toolchain,
        // not every unrelated browser, account, RSI and MCP schema. The
        // owner can still explicitly request any enabled optional capability.
        return definitions.into_iter().filter(|definition|{
            matches!(tool_name(definition),
                "workspace_list"|"workspace_read"|"workspace_read_range"|
                "code_search"|"workspace_replace_range"|"workspace_replace"|
                "workspace_write"|"workspace_apply_patch"|"shell_exec"|
                "terminal_exec"|"web_search"|"http_fetch")
                || (acceptance && tool_name(definition).starts_with("mcp__")
                    && query.to_lowercase().contains("mcp"))
        }).collect();
    }
    scored.truncate(8);
    scored.into_iter().map(|(_, definition)| definition).collect()
}


/*
 * Main Home-panel tool inventory projection.
 *
 * A long-running autonomous coding job still needs the complete native
 * read/edit/test toolchain. Only unrelated domains are omitted: their huge
 * schemas were previously re-sent on every model round even when disabled
 * in the UI. Explicit user intent restores optional native capabilities;
 * enabled MCP definitions remain eligible and are not silently discarded.
 */
pub(super) fn select_panel_tool_definitions(definitions:Vec<Value>,query:&str)->Vec<Value>{
    let text=query.to_lowercase();
    let mentions=|terms:&[&str]|terms.iter().any(|word|text.contains(word));
    let external_file=mentions(&["절대경로","외부 폴더","다른 드라이브",
        "outside workspace","absolute path","c:\\","d:\\","/home/","/users/",
        "/mnt/","/tmp/","/var/","/etc/","/workspace/"]);
    let wants_memory=mentions(&["기억","메모리","memory","remember"]);
    let wants_save_memory=mentions(&["기억해","기억해 둬","메모리에 저장",
        "remember this","save memory"]);
    let wants_skill=mentions(&["스킬","skill","재사용 절차","reusable procedure"]);
    let wants_browser=mentions(&["브라우저","browser","웹페이지","웹사이트","사이트",
        "웹 검색","웹에서","네이버","구글","접속","페이지 이동",
        "click the page","스크린샷","screenshot","ui test","e2e test",
        "open url","visit url","website","web page"]);
    let wants_github=mentions(&["깃허브","github","git push","git commit",
        "커밋","푸시","레포지토리","리포지토리","repository",
        "풀 리퀘스트","pull request","이슈 생성","issue 생성"]);
    let wants_accounts=mentions(&["연결 계정","로그인 계정","oauth","connected account"]);
    definitions.into_iter().filter(valid_tool_definition).filter(|definition|{
        match tool_name(definition){
            // The user-visible work UI already exposes its own capability
            // flags; this projection is deliberately about advertised schema
            // cost, not executable permissions.
            "file_list"|"file_read"|"file_write"=>external_file,
            "memory_store"=>wants_save_memory,
            "memory_search"=>wants_memory,
            "skill_propose"=>wants_skill,
            "browser_control"=>wants_browser,
            "github_api"=>wants_github,
            "account_status"=>wants_accounts,
            // Tools with user-selectable policy already filtered by
            // ui_tool_allowed; preserve them when the owner opts in.
            _=>true,
        }
    }).collect()
}

pub(super) fn select_tool_definitions(
    definitions: Vec<Value>,
    query: &str,
    algorithm: &Value,
) -> Vec<Value> {
    // No empty / malformed function schemas are ever advertised.
    let definitions=definitions.into_iter().filter(valid_tool_definition).collect::<Vec<_>>();
    let strategy = algorithm["implementation"]
        .as_str()
        .or_else(|| algorithm["variant"].as_str())
        .unwrap_or("all");
    if strategy == "all" {
        return definitions;
    }
    let default_core = [
        "workspace_list", "workspace_read", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "workspace_apply_patch", "shell_exec",
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

pub(super) fn trim_tool_definitions(definitions: Vec<Value>, budget: usize) -> Vec<Value> {
    let mut kept = Vec::new();
    let mut used = 0usize;
    for definition in definitions.into_iter().filter(valid_tool_definition) {
        let size = serde_json::to_vec(&definition)
            .map(|json| json.len()).unwrap_or(usize::MAX);
        // Never send even the first oversize schema; a single giant
        // definition can consume the entire model context / provider budget.
        if size > budget.saturating_sub(used) {
            continue;
        }
        used += size;
        kept.push(definition);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn malformed_catalog_entries_cannot_displace_actual_tools() {
        let blank=json!({"type":"function","function":{"name":""}});
        let missing=json!({"type":"function","function":{}});
        let malformed=json!({"type":"function","function":{"name":"bad tool"}});
        let valid=json!({"type":"function","function":{"name":"workspace_read","parameters":{"type":"object"}}});
        let definitions=vec![blank,missing,malformed,valid.clone()];
        assert_eq!(select_tool_definitions(definitions.clone(),"search",
            &json!({"implementation":"all"})),vec![valid.clone()]);
        assert_eq!(select_room_tools(definitions.clone(),"파일 읽어",
            &Default::default(),false),vec![valid.clone()]);
        assert_eq!(select_panel_tool_definitions(definitions.clone(),"읽어"),
            vec![valid.clone()]);
        assert_eq!(trim_tool_definitions(definitions,10000),vec![valid]);
    }

    #[test]
    fn full_schema_budget_is_honored_including_first_definition() {
        let huge=json!({"function":{"name":"mcp_huge","description":"x".repeat(4000)}});
        let small=json!({"function":{"name":"workspace_read"}});
        let budget=serde_json::to_vec(&small).unwrap().len();
        let kept=trim_tool_definitions(vec![huge,small.clone()],budget);
        assert_eq!(kept,vec![small]);
        assert!(trim_tool_definitions(vec![json!({"function":{"name":"large","description":"y".repeat(99)}})],1).is_empty());
    }
}
