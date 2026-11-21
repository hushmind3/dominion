//! Bounded model-facing tool schema projection shared by room, Home and native
//! agent execution. Selection never grants tool permissions: execution policy
//! is still enforced by the Rust dispatcher / approvals.
use crate::state::AppState;
use serde_json::Value;

/// A model-origin call is admitted only if this provider round actually
/// declared its tool. ProgramIR and approved native procedures are separate
/// authorities and continue to use their own validated tool contracts.
pub(super) fn advertised_tool(definitions:&[Value],requested:&str)->bool{
    !requested.is_empty() && definitions.iter()
        .any(|definition|tool_name(definition)==requested)
}

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

/// 2026 provider-prefix contract: registration timing must not change the
/// serialized tools array for an identical set of owner-enabled schemas.
/// Keep the search gateway and core read/edit/test primitives ahead of
/// optional capabilities so context-budget clipping never promotes an MCP
/// schema over an indispensable file operation. Among identical tool names,
/// full schema bytes break ties deterministically. This changes visibility
/// order only: dispatcher permissions and approvals stay authoritative.
pub(super) fn canonical_tool_definitions(mut definitions:Vec<Value>)->Vec<Value>{
    let priority=|name:&str|->usize{match name{
        "dominion_tool_search"=>0,
        // Without a source reader, an indexed policy or archived owner
        // constraint would be silently unavailable after schema trimming.
        // Pin read-only source capabilities ahead of bulky optional tools.
        "prompt_source_read"=>1,"history_evidence_read"=>2,
        "workspace_list"=>3,"workspace_read"=>4,"workspace_read_range"=>5,
        "code_search"=>6,"workspace_apply_patch"=>7,
        "workspace_replace_range"=>8,"workspace_replace"=>9,
        "workspace_write"=>10,"shell_exec"=>11,"terminal_exec"=>12,
        "web_search"=>13,"http_fetch"=>14,
        "skill_read"=>15,"room_message_read"=>16,
        _=>100,
    }};
    definitions.sort_by(|a,b|{
        let left=tool_name(a);
        let right=tool_name(b);
        priority(left).cmp(&priority(right))
            .then_with(||left.cmp(right))
            .then_with(||a.to_string().cmp(&b.to_string()))
    });
    definitions
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
        "browser", "memory", "delegate", "task", "팀", "협업", "team", "worker", "lead",
        "동료에게", "peer", "inbox", "보내", "전달",
        "구글", "드라이브", "시트", "문서", "google", "drive", "docs", "sheets",
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
            &["팀", "협업", "team", "worker", "lead", "작업 분해", "멀티에이전트", "subagent"],
            &["team_workers", "team_execute", "team_status", "delegate_task"],
        ),
        (
            &["동료에게", "봇에게", "전달해", "메시지 보내", "peer", "inbox", "받은편지함"],
            &["room_peer_send", "room_peer_inbox"],
        ),
        (
            &["외부 에이전트", "외부 실행기", "codex", "claude code", "qwen code", "bridge"],
            &["external_harness_run", "external_harness_sessions"],
        ),
        (
            &["브라우저", "browser", "클릭", "click"],
            &["browser_control"],
        ),
        (&["github", "깃허브", "repository", "레포지토리", "리포지토리", "저장소"], &["github_repositories","github_repository_activity","github_file_read","github_file_commit","github_branch_create","github_issue_create","github_pr_create","github_api"]),
        (&["구글", "google", "드라이브", "drive", "docs", "문서", "시트", "sheets", "스프레드시트"], &["google_drive_search","google_docs_read","google_docs_replace","google_sheets_read","google_sheets_update","browser_control"]),
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
            // An installed MCP search tool is a more specific match than
            // GitHub's generic create/edit schema when the owner asks to
            // search issues. Do not let new native tools evict MCP search.
            let issue_search=lower.contains("이슈") || lower.contains("issue");
            let searching=lower.contains("검색") || lower.contains("search");
            if name_lower.starts_with("mcp__") && issue_search && searching
                && (name_lower.contains("issue") || name_lower.contains("search")){
                score += 32;
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
        // not every unrelated browser, account and MCP schema. The
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
    // A local docs/ directory or .github/workflows path is not a
    // cloud connector request. Require explicit provider/browser intent.
    let google_service=mentions(&[
        "google docs","google drive","google sheets",
        "docs.google.com","drive.google.com","sheets.google.com",
        "구글 문서","구글문서","구글 드라이브","구글드라이브",
        "구글 시트","구글시트",
    ]);
    let wants_browser=google_service || mentions(&[
        "브라우저","browser","웹페이지","웹사이트","사이트",
        "웹 검색","웹에서","네이버","구글 검색","구글에서",
        "github.com","깃허브 접속","접속","페이지 이동",
        "click the page","스크린샷","screenshot","ui test","e2e test",
        "open url","visit url","website","web page","http://","https://",
    ]);
    let wants_github=mentions(&["깃허브","github","git push","git commit",
        "커밋","푸시","레포지토리","리포지토리","repository",
        "풀 리퀘스트","pull request","이슈 생성","issue 생성"]);
    let wants_google=google_service || mentions(&[
        "구글 계정","google account","구글 로그인","google login",
        "드라이브에 업로드","드라이브에서 가져","온라인 스프레드시트",
    ]);
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
            "github_api"|"github_repositories"|"github_repository_activity"|"github_file_read"|
            "github_file_commit"|"github_issue_create"|"github_branch_create"|
            "github_pr_create"=>wants_github,
            "account_status"=>wants_accounts || wants_google,
            "google_drive_search"|"google_docs_read"|"google_docs_replace"|
            "google_sheets_read"|"google_sheets_update"=>wants_google,
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
    // An operator-configured external harness is one *delegation target*,
    // not a second engine. Keep it alongside native delegation when the
    // selected core allows that category. No configured adapter: no schema.
    let team_intent=["팀","협업","team","lead","worker","멀티에이전트"]
        .iter().any(|word|query.to_lowercase().contains(word));
    let external_intent=["external harness","외부 실행기","codex","claude code","qwen code"]
        .iter().any(|word|query.to_lowercase().contains(word));
    let delegated=|name:&str|core.contains("delegate_task") && (
        (matches!(name,"team_workers"|"team_execute"|"team_status") && team_intent)
        || (matches!(name,"external_harness_run"|"external_harness_sessions")
            && external_intent)
    );
    if strategy == "core" {
        return definitions
            .into_iter()
            .filter(|definition| {
                let name=tool_name(definition);
                core.contains(name) || delegated(name)
            })
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
            core.contains(tool_name(definition)) || delegated(tool_name(definition))
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
    fn real_team_and_peer_tools_are_advertised_for_explicit_room_intent(){
        let make=|name:&str|json!({"type":"function","function":{
            "name":name,"description":name,
            "parameters":{"type":"object","properties":{}}
        }});
        let catalog=[
            "team_workers","team_execute","team_status","room_peer_send",
            "room_peer_inbox","external_harness_sessions","workspace_read",
            "delegate_task","task_status"
        ].iter().map(|name|make(name)).collect::<Vec<_>>();
        let names=|query:&str|select_room_tools(catalog.clone(),query,
            &Default::default(),false).into_iter()
            .map(|entry|tool_name(&entry).to_string())
            .collect::<std::collections::HashSet<_>>();
        let team=names("에이전트 팀을 만들고 병렬 worker를 실행해");
        for name in ["team_workers","team_execute","team_status"]{
            assert!(team.contains(name),"Team intent needs {name}");
        }
        let peers=names("동료에게 메시지 보내");
        assert!(peers.contains("room_peer_send"));
        assert!(peers.contains("room_peer_inbox"));
        let core=select_tool_definitions(catalog,"팀 작업 분해",
            &json!({"implementation":"core"}));
        assert!(advertised_tool(&core,"team_workers"));
        assert!(advertised_tool(&core,"team_execute"));
        assert!(!advertised_tool(&core,"room_peer_send"));
    }
    #[test]
    fn priority_reserves_source_reader_under_tight_provider_schema_budget(){
        let make=|name:&str,description:&str|json!({
            "type":"function",
            "function":{"name":name,"description":description,
                "parameters":{"type":"object","properties":{},"required":[]}}
        });
        let search=make("dominion_tool_search","Search tools");
        let persona=make("prompt_source_read","Recover owner/persona original");
        let evidence=make("history_evidence_read","Recover archived exact event");
        let bulky=make("optional_heavy",&"OVERLONG_SCHEMA_DESCRIPTION".repeat(1000));
        let ordered=canonical_tool_definitions(vec![
            bulky,persona.clone(),evidence.clone(),search.clone()]);
        let budget=search.to_string().len()+persona.to_string().len()+evidence.to_string().len()+4;
        let result=trim_tool_definitions(ordered,budget);
        assert!(advertised_tool(&result,"dominion_tool_search"));
        assert!(advertised_tool(&result,"prompt_source_read"));
        assert!(advertised_tool(&result,"history_evidence_read"));
        assert!(!advertised_tool(&result,"optional_heavy"));
    }

    #[test]
    fn canonical_tools_preserve_cache_prefix_across_mcp_registration_order(){
        let mk=|name:&str|json!({"type":"function","function":{
            "name":name,"description":format!("{name} tool"),
            "parameters":{"type":"object","properties":{"path":{"type":"string"}}}
        }});
        let a=vec![mk("mcp__zeta"),mk("workspace_write"),mk("mcp__alpha"),
            mk("shell_exec"),mk("workspace_read"),mk("dominion_tool_search")];
        let mut b=a.clone();
        b.reverse();
        let sorted=canonical_tool_definitions(a.clone());
        assert_eq!(sorted,canonical_tool_definitions(b));
        assert_eq!(sorted[0]["function"]["name"],"dominion_tool_search");
        assert_eq!(sorted[1]["function"]["name"],"workspace_read");
        assert_eq!(sorted[2]["function"]["name"],"workspace_write");
        assert_eq!(sorted[3]["function"]["name"],"shell_exec");
        assert_eq!(sorted[4]["function"]["name"],"mcp__alpha");
        assert_eq!(sorted[5]["function"]["name"],"mcp__zeta");
        let clipped=trim_tool_definitions(sorted.clone(),
            sorted[0].to_string().len()+sorted[1].to_string().len()
                +sorted[2].to_string().len());
        assert!(clipped.iter().any(|tool|tool_name(tool)=="workspace_read"));
        assert!(clipped.iter().any(|tool|tool_name(tool)=="workspace_write"));
        let changed=mk("mcp__alpha");
        assert_eq!(changed,a[2],"sorting must not mutate tool schema payloads");
    }

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
    fn local_docs_paths_do_not_activate_google_or_browser_tool_schemas(){
        let definition=|name:&str|json!({"type":"function","function":{
            "name":name,"parameters":{"type":"object"}
        }});
        let catalog=[
            "workspace_read","workspace_write","shell_exec",
            "browser_control","google_docs_read","google_drive_search",
            "google_sheets_read","account_status"
        ].iter().map(|name|definition(name)).collect::<Vec<_>>();
        let names=|query:&str|{
            select_panel_tool_definitions(catalog.clone(),query)
                .iter().map(tool_name).map(str::to_string)
                .collect::<std::collections::HashSet<_>>()
        };
        for query in [
            "docs/a.txt와 docs/b.txt를 만들고 SUMMARY.md 생성해",
            "로컬 문서 docs/a.txt를 읽어",
            "C:\\docs\\a.txt를 수정하고 시트 파일을 로컬에 저장해"
        ]{
            let selected=names(query);
            assert!(selected.contains("workspace_write"),"{query}");
            for unrelated in ["browser_control","google_docs_read",
                "google_drive_search","google_sheets_read","account_status"]{
                assert!(!selected.contains(unrelated),"{query} must not advertise {unrelated}");
            }
        }
        for query in [
            "Google Docs 문서를 읽어",
            "https://docs.google.com/document/d/123 수정해",
            "구글 드라이브에서 문서를 가져와"
        ]{
            let selected=names(query);
            assert!(selected.contains("google_docs_read"),"{query}");
            assert!(selected.contains("google_drive_search"),"{query}");
            assert!(selected.contains("browser_control"),"{query}");
            assert!(selected.contains("account_status"),"{query}");
        }
        assert!(names("브라우저에서 사이트 열어").contains("browser_control"));
    }

    #[test]
    fn model_cannot_name_an_unadvertised_tool_even_if_it_exists_elsewhere(){
        let round=vec![
            json!({"function":{"name":"workspace_read"}}),
            json!({"function":{"name":"dominion_tool_search"}}),
        ];
        assert!(advertised_tool(&round,"workspace_read"));
        assert!(advertised_tool(&round,"dominion_tool_search"));
        assert!(!advertised_tool(&round,"workspace_write"));
        assert!(!advertised_tool(&round,"mcp__private__delete"));
        assert!(!advertised_tool(&round,""));
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
