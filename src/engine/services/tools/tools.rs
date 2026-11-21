#[path = "tools_patch.rs"]
mod patch;
#[path = "tools_external_harness.rs"]
mod external_harness;
#[path = "tools_team.rs"]
mod team;
#[path = "tools_room_peer.rs"]
mod room_peer;
#[path = "tools_remote_artifacts.rs"]
mod remote_artifacts;

use crate::db::Store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Everything a tool needs in order to run. Tools are pure Rust: no Node,
/// no sidecar process.
pub struct ToolContext<'a> {
    pub workspace: &'a Path,
    pub store: &'a Store,
    pub state: &'a crate::state::AppState,
    pub session_id: &'a str,
    pub agent_id: Option<&'a str>,
    /// Parent turn's model tool permission. Delegation is attenuated, never
    /// widened.
    pub tools_enabled: bool,
    /// Parent turn's overall execution permission.
    pub tools_execution_enabled: bool,
}

/// OpenAI-style tool definitions handed to the model.
pub fn definitions() -> Vec<Value> {
    let object = |properties: Value, required: Vec<&str>| {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    };
    let text = |description: &str| json!({ "type": "string", "description": description });

    let mut defs=vec![
        tool("workspace_list", "List files and folders in the workspace", object(json!({"path": text("Relative folder path")}), vec![])),
        tool("workspace_read", "Read a UTF-8 file from the workspace", object(json!({"path": text("Relative file path")}), vec!["path"])),
        tool("workspace_read_range", "Read only a line range from a workspace file. Use the line numbers from the repository index instead of reading a large file end to end.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1, "description":"Inclusive end line; at most 400 lines are returned"})}), vec!["path","startLine"])),
        tool("code_search", "Search the persistent repository index for file paths, symbols and line numbers. Use this before reading a large file.", object(json!({"query": text("Function, type, path or keyword fragment"), "limit": json!({"type":"integer","minimum":1,"maximum":50})}), vec!["query"])),
        tool("workspace_replace_range", "Replace a previously read inclusive line range with newText only. Use startLine/endLine and sourceSha256 returned by workspace_read_range; old text is not required.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1,"description":"Inclusive line number; at most 120 lines"}), "newText": text("Replacement lines (without old source text)"), "expectedSha256": text("Exact sourceSha256 from the successful prior read")}), vec!["path","startLine","endLine","newText","expectedSha256"])),
        tool("workspace_replace", "Replace one exact block in a workspace file without resending the whole file (approval required). oldText must occur exactly once.", object(json!({"path": text("Relative file path"), "oldText": text("Exact existing text to replace; must occur exactly once"), "newText": text("Replacement text"), "expectedSha256": text("Optional SHA-256 observed before editing")}), vec!["path","oldText","newText"])),
        tool("workspace_write", "Write a UTF-8 file inside the workspace (approval required)", object(json!({"path": text("Relative file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("workspace_apply_patch", "Apply one Codex-style multi-file patch across up to 32 files: *** Begin Patch, *** Add/Update/Delete File, optional *** Move to, @@ hunks, *** End Patch. Entire patch is validated before writes; on commit failure inspect the returned partial receipts and re-read files. Approval required.", object(json!({
            "input": text("One Codex-style multi-file patch. Prefix changed lines with + or -, context lines with one space.")
        }), vec!["input"])),
        tool("file_list", "List any folder on this computer using an absolute path", object(json!({"path": text("Absolute folder path")}), vec!["path"])),
        tool("file_read", "Read any UTF-8 text file using an absolute path", object(json!({"path": text("Absolute file path")}), vec!["path"])),
        tool("file_write", "Create or overwrite any file using an absolute path (approval required)", object(json!({"path": text("Absolute file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("shell_exec", "Run a shell command on this computer and return stdout/stderr (approval required)", object(json!({"command": text("Shell command line"), "cwd": text("Optional absolute working directory")}), vec!["command"])),
        tool("terminal_exec", "Run an executable with an argument array without a shell (approval required)", object(json!({"command": text("Executable"), "args": json!({"type": "array", "items": {"type": "string"}})}), vec!["command"])),
        tool("web_search", "Search the public web for current information", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("http_fetch", "Fetch readable page text, excluding HTML script/style/navigation. Returns a short page with nextOffset; follow it to read more, instead of injecting entire pages into model context.", object(json!({"url": text("HTTP or HTTPS URL"), "offset": json!({"type":"integer","minimum":0,"description":"Unicode character offset; use prior nextOffset"}), "maxChars": json!({"type":"integer","minimum":1000,"maximum":16000,"description":"Page size, default 6000; request larger only when needed"})}), vec!["url"])),
        tool("memory_store", "Save a grounded memory for this agent. Quote the user's exact words; invented facts are rejected. Shared memories are managed by the user.", object(json!({"text": text("Fact to remember, maximum 1600 characters"), "evidenceQuote": text("Exact quote from the current user message supporting the fact"), "sourceMessageId": text("Optional earlier source message ID"), "type": json!({"type":"string","enum":["fact","preference","decision","procedure","lesson"]}), "key": text("Optional stable fact key; a new value replaces the previous value for this key"), "scope": json!({"type":"string","enum":["agent"]})}), vec!["text", "evidenceQuote"])),
        tool("memory_search", "Find saved knowledge relevant to a topic", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("skill_read", "Read a page of the full instructions of one enabled skill. Use nextOffset to continue if hasMore=true.", object(json!({"skillId": text("Exact enabled skill ID from the index"), "offset":json!({"type":"integer","minimum":0,"description":"Optional Unicode character offset; start with 0, then use nextOffset"})}), vec!["skillId"])),
        tool("prompt_source_read", "Read or search the exact current agent persona or enabled owner instructions by versioned source. Large source texts remain durable and are never silently discarded.", object(json!({
            "source":json!({"type":"string","enum":["persona","owner"],
                "description":"Which indexed policy source to open"}),
            "offset":json!({"type":"integer","minimum":0,"description":"Unicode character offset from the index or previous nextOffset"}),
            "query":text("Optional keyword to seek within the source instead of a numeric offset"),
            "revision":text("Optional exact revision from the source index; rejects stale reads")
        }),vec!["source"])),
        tool("history_evidence_read", "Read exact prior tool result or private chat owner message by sourceMessageId. Page the needed 6000 characters without replaying the entire archived document every turn.", object(json!({
            "messageId":text("Exact tool result message ID shown in a historical receipt"),
            "offset":json!({"type":"integer","minimum":0,"description":"Unicode character offset, default 0"})
        }),vec!["messageId"])),
        tool("room_peer_send","Queue one durable peer-to-peer Bot Room message to another existing participant. Delivery is on their next real turn; no recursive model call. Reply chains have a hard four-hop limit.", object(json!({
            "agentId":text("Existing agent ID in the SAME Bot Room"),
            "message":text("Up to 2000 UTF-8 bytes, untrusted peer communication"),
            "replyToMessageId":text("Optional committed peer message addressed to the sender")
        }),vec!["agentId","message"])),
        tool("room_peer_inbox","Read committed inbound Bot Room peer messages, queued and delivered, without invoking another bot",object(json!({}),vec![])),
        tool("room_message_read", "Retrieve a source-addressed room participant statement from the current room transcript; pagination preserves original wording", object(json!({
            "messageId":text("Exact participant message ID from the peer index"),
            "offset":json!({"type":"integer","minimum":0,"description":"Optional Unicode character offset for the next page"})
        }),vec!["messageId"])),
        tool("skill_propose", "Draft a reusable skill for user review", object(json!({"name": text("Skill name"), "instructions": text("Reusable instructions")}), vec!["name", "instructions"])),
        tool("team_execute", "Lead-run managed task DAG: assign named agents, run independent research concurrently, serialize writers, retain real child job evidence and resume by teamId after interruptions. Task order and dependsOn are enforced by Rust. Requires lead approval.", object(json!({
            "teamId":text("Existing teamId to resume a durable team; omit when starting tasks"),
            "tasks":json!({"type":"array","minItems":1,"maxItems":8,
                "items":{"type":"object","properties":{
                    "id":text("Stable ASCII task ID"),"agentId":text("Registered local or remote worker agent"),
                    "remoteWorkerId":text("Optional live Rust remote node ID from team_workers.remoteWorkers; omit to execute locally"),
                    "artifactPaths":json!({"type":"array","maxItems":8,"items":{"type":"string"},
                        "description":"Approved, bounded relative UTF-8 output files to transfer from a remote Writer; requires readOnly=false. Local merge is a separately approved action."}),
                    "prompt":text("Self-contained task with expected result and checks"),
                    "dependsOn":json!({"type":"array","items":{"type":"string"}}),
                    "successCriteria":json!({"type":"array","maxItems":20,"description":"Optional frozen native acceptance checks; file_exists, file_equals or command_succeeded are verified by the Rust worker", "items":{"type":"object"}}),
                    "readOnly":json!({"type":"boolean","description":"Default true; false requests an exclusive writer stage with normal approval policy"})
                },"required":["id","agentId","prompt"],"additionalProperties":false}}),
            "wait":json!({"type":"boolean","description":"Wait for the next ready waves (default true)"}),
            "timeoutSeconds":json!({"type":"integer","minimum":5,"maximum":300}),
        }),vec![])),
        tool("team_workers","Discover eligible registered Worker agents (IDs, roles and models) so the Lead can propose an executable Team DAG without guessing agent IDs",object(json!({}),vec![])),
        tool("team_status","Read the durable lead-owned Task Team DAG and every native worker job receipt without launching new work",object(json!({
            "teamId":text("Team id returned by team_execute")
        }),vec!["teamId"])),
        tool("team_remote_apply","Apply a completed remote Writer file receipt in the Lead's local workspace only after user approval; checks frozen base SHA-256, source hash and native patch preflight",object(json!({
            "teamId":text("Owning Lead team ID"),"remoteTaskId":text("Completed remote Writer task ID from team_status")
        }),vec!["teamId","remoteTaskId"])),
        tool("delegate_task", "Delegate a self-contained task to another agent and optionally wait for its result", object(json!({
            "agentId": text("Target agent id"),
            "prompt": text("Task instructions"),
            "wait": json!({"type":"boolean","description":"Wait for completion (default true)"}),
            "timeoutSeconds": json!({"type":"number","description":"5-300 seconds"})
        }), vec!["agentId", "prompt"])),
        tool("delegate_parallel", "Launch 2-4 independent read-only research tasks concurrently in isolated Rust job sessions; return bounded results or durable job IDs. No shared workspace writes are allowed.", object(json!({
            "tasks": json!({"type":"array","minItems":2,"maxItems":4,
                "items":{"type":"object","properties":{
                    "agentId":{"type":"string"},"prompt":{"type":"string"}
                },"required":["agentId","prompt"],"additionalProperties":false}}),
            "wait":json!({"type":"boolean","description":"Wait for the concurrent tasks (default true)"}),
            "timeoutSeconds":json!({"type":"integer","minimum":5,"maximum":300})
        }),vec!["tasks"])),
        tool("task_status", "Check a delegated task's status", object(json!({"jobId": text("Job id")}), vec!["jobId"])),
        tool("task_result", "Read a delegated task's result, artifacts and final answer", object(json!({"jobId": text("Job id")}), vec!["jobId"])),
        tool("account_status", "List OAuth accounts connected to DOMINION", object(json!({}), vec![])),
        tool("browser_control", "Control a local Chrome/Chromium window: open pages, observe text and interactive elements, click, type, press keys and capture screenshots (approval required)", object(json!({
            "action": json!({"type": "string", "enum": ["status", "open", "observe", "click", "type", "press", "screenshot", "close"]}),
            "url": text("URL to open"),
            "index": json!({"type": "number", "description": "Element index returned by observe"}),
            "text": text("Text to type into the indexed element"),
            "key": text("Key to press: Enter, Escape, Tab, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Backspace")
        }), vec!["action"])),

        tool("google_drive_search", "Search files in connected Google Drive and return file IDs, names, types, links and canEdit.", object(json!({
            "query":text("File name search, blank for recent files"),"pageToken":text("Optional pagination cursor")
        }),vec![])),
        tool("google_docs_read", "Read a Google Doc's title, text, and revision ID before editing.", object(json!({
            "documentId":text("Google Docs document ID")
        }),vec!["documentId"])),
        tool("google_docs_replace", "Replace case-sensitive text in a Google Doc after owner approval; requires revision ID returned by google_docs_read and verifies remotely.", object(json!({
            "documentId":text("Google Docs document ID"),
            "find":text("Exact source text"),"replace":text("Replacement text"),
            "expectedRevisionId":text("Revision ID returned by google_docs_read")
        }),vec!["documentId","find","replace","expectedRevisionId"])),
        tool("google_sheets_read", "Read a Google Sheets A1 range with unformatted values so a write can be checked against the observed cells.", object(json!({
            "spreadsheetId":text("Google Sheets ID"),"range":text("A1 range, e.g. Sheet1!A1:C10")
        }),vec!["spreadsheetId","range"])),
        tool("google_sheets_update", "Update a Google Sheets A1 range after owner approval, only if expectedValues match a fresh read. Verify via remote readback.", object(json!({
            "spreadsheetId":text("Google Sheets ID"),"range":text("A1 range"),
            "values":json!({"type":"array","items":{"type":"array","items":{"type":["string","number","boolean"]}}}),
            "expectedValues":json!({"type":"array","description":"Exact prior values from google_sheets_read","items":{"type":"array"}})
        }),vec!["spreadsheetId","range","values","expectedValues"])),
        tool("github_repository_activity", "List open GitHub issues and pull requests for the selected repository using the connected account.", object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name")
        }),vec!["owner","repo"])),
        tool("github_issue_create", "Create a GitHub issue after owner approval and verify it by fetching the remote issue.", object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "title":text("Issue title"),"body":text("Issue description")
        }),vec!["owner","repo","title"])),
        tool("github_branch_create", "Create a feature branch from the repository base SHA after approval; verify remote branch before code edits.", object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "newBranch":text("New branch name"),"baseBranch":text("Optional source branch")
        }),vec!["owner","repo","newBranch"])),
        tool("github_pr_create", "Create a pull request from a feature branch into the default or chosen base branch after owner approval, then verify it remotely.", object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "head":text("Feature branch"),"base":text("Optional base branch"),
            "title":text("PR title"),"body":text("Description of changes and tests")
        }),vec!["owner","repo","head","title"])),
        tool("github_repositories", "List repositories available to the connected GitHub account, with owner, repository, default branch and explicit write permission. Choose a repository before reading or editing; supports page and writable-only filtering.", object(json!({
            "query":text("Optional repository name fragment; filtered within this page"),
            "page":json!({"type":"integer","minimum":1,"maximum":10}),
            "writableOnly":json!({"type":"boolean","description":"Only repositories with confirmed push permission"})
        }),vec![])),
        tool("github_file_read", "Read a UTF-8 text file from an explicitly selected GitHub owner/repository/branch through the connected account; returns SHA for safe updates.", object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "path":text("Repository-relative file path"),"branch":text("Optional ref, defaults to repository default branch")
        }),vec!["owner","repo","path"])),
        tool("github_file_commit", "Commit a text file through the connected GitHub account after owner approval. Read first, provide expectedSha for existing files, then verify by remote re-read. Never blindly retry uncertain effects.", object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "path":text("Repository-relative file path"),"branch":text("Target branch; defaults to repository default"),
            "content":text("Complete UTF-8 file contents"),"message":text("Commit message"),
            "expectedSha":text("Exact prior SHA returned by github_file_read, required for existing files")
        }),vec!["owner","repo","path","content","message"])),
        tool("github_api", "Call the GitHub REST API as the connected account. Write methods require approval (approval required)", object(json!({"method": json!({"type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"]}), "path": text("GitHub API path beginning with /"), "body": json!({"type": "object"})}), vec!["method", "path"])),
    ];
    // No installed external harness is packaged or started by DOMINION.
    // With no owner-configured adapter, even its tool schema is absent.
    let enabled_harnesses=external_harness::configured_ids();
    if !enabled_harnesses.is_empty(){
        let description=format!(
            "Delegate a task to one separately installed owner-approved harness ({}). No runtime is bundled. DOMINION retains its single Rust execution and approval gateway.",
            enabled_harnesses.join(", "),
        );
        defs.push(tool("external_harness_run",&description,
            object(json!({
                "harnessId":{"type":"string","enum":enabled_harnesses},
                "prompt":text("Self-contained delegated task with expected result"),
                "bridgeSessionId":text("Optional DOMINION-issued owner-scoped session UUID for a configured external resume"),
                "timeoutSeconds":{"type":"integer","minimum":5,"maximum":600},
            }),vec!["harnessId","prompt"])));
        defs.push(tool("external_harness_sessions",
            "List owner-scoped durable Codex, Claude or Qwen bridge sessions and safe-resume eligibility without exposing remote thread IDs",
            object(json!({}),vec![])));
    }
    defs
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

/// Only advertise REST tools if a corresponding account grant exists.
/// Compute CLI status once per turn, not once for every model tool schema.
/// Browser cookies never become implied API permission.
pub fn retain_connected_account_tools(state:&crate::state::AppState,defs:&mut Vec<Value>){
    let github_ready=state.read_secret("oauth:github:token").is_some() ||
        crate::engine::oauth::cli_enabled(state,"github") &&
        crate::engine::cli_accounts::status("github").connected;
    let drive=crate::engine::oauth::has_granted_scope(state,"google","drive.readonly");
    let docs=crate::engine::oauth::has_granted_scope(state,"google","documents");
    let sheets=crate::engine::oauth::has_granted_scope(state,"google","spreadsheets");
    defs.retain(|definition|{
        let name=definition["function"]["name"].as_str().unwrap_or("");
        match name{
            "google_drive_search"=>drive,
            "google_docs_read"|"google_docs_replace"=>docs,
            "google_sheets_read"|"google_sheets_update"=>sheets,
            "github_api"|"github_repositories"|"github_repository_activity"|
            "github_file_read"|"github_file_commit"|"github_issue_create"|
            "github_branch_create"|"github_pr_create"=>github_ready,
            _=>true
        }
    });
}
pub fn requires_approval(name: &str) -> bool {
    matches!(
        name,
        "workspace_write"
            | "workspace_apply_patch"
            | "workspace_replace"
            | "workspace_replace_range"
            | "file_write"
            | "shell_exec"
            | "terminal_exec"
            | "external_harness_run"
            | "team_execute"
            | "team_remote_apply"
            | "github_api"
            | "github_file_commit"
            | "github_issue_create"
            | "github_branch_create"
            | "github_pr_create"
            | "google_docs_replace"
            | "google_sheets_update"
            | "browser_control"
    )
}

/// Read-only parallel branches share a live workspace, so writes, shell,
/// arbitrary absolute reads, external MCP and further delegation are denied.
pub fn parallel_read_only_tool(name:&str)->bool{
    matches!(name,"workspace_list"|"workspace_read"|"workspace_read_range"|
        "code_search"|"memory_search"|"skill_read"|"room_message_read"|"history_evidence_read"|"prompt_source_read"|"web_search"|"http_fetch"|
        "github_file_read"|"github_repositories"|"github_repository_activity"|
        "google_drive_search"|"google_docs_read"|"google_sheets_read"|
        // Pure discovery only exposes this child's existing read-only catalog.
        "dominion_tool_search")
}

/// The UI's optional capabilities are a persisted execution policy.
pub fn ui_tool_allowed(policy:&Value,name:&str)->bool{
    if matches!(name,"algorithm_propose"|"unsafe_audit"){return false;}
    if policy.is_null(){return true;}
    if name.starts_with("skill_") && policy["skillsEnabled"]==false{return false;}
    if name.starts_with("mcp__") && policy["mcpEnabled"]!=true{return false;}
    if matches!(name,"web_search"|"http_fetch") && policy["webEnabled"]!=true{return false;}
    if matches!(name,"delegate_task"|"delegate_parallel"|"task_status"|"task_result"|
        "team_execute"|"team_remote_apply"|"team_status"|"team_workers"|"external_harness_sessions"|"external_harness_run") && policy["subagentsEnabled"]!=true{return false;}
    true
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &Value,
    run_id: &str,
) -> Result<Value, String> {
    let session=ctx.store.get("session",ctx.session_id);
    if session.as_ref().is_some_and(|s|!ui_tool_allowed(
        &crate::engine::execution_scope::active_panel_policy(ctx.state,s),name)){
        return Err("사용자 설정에서 비활성화한 도구입니다.".into());
    }
    if session.as_ref().is_some_and(|s|s["parallelReadOnly"]==true)
        && !parallel_read_only_tool(name){
        return Err("병렬 조사 에이전트는 읽기 전용 도구만 사용할 수 있습니다.".into());
    }
    // Cross-job resource locks: shell / terminal / source mutations must not
    // race on the same workspace. Safe parallel reads remain unrestricted.
    // Held until the tool returns, including its committed evidence.
    let _resource_guard=crate::engine::resource_lock::acquire(ctx.workspace,name,args).await;
    let _durable_writer_lease=crate::engine::resource_lock::acquire_durable(
        ctx.state,ctx.workspace,name,args,ctx.session_id,
    ).await?;
    match name {
        "workspace_list" => {
            let path = resolve_workspace(ctx.workspace, string_arg(args, "path").unwrap_or("."))?;
            list_dir(&path).await
        }
        "workspace_read" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            observed_read(ctx, &path, run_id).await
        }
        "workspace_read_range" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            let start = args["startLine"].as_u64().unwrap_or(1).max(1);
            let requested_end = args["endLine"].as_u64().unwrap_or(start.saturating_add(199));
            let result = observed_read(ctx, &path, run_id).await?;
            let text = result["content"].as_str().unwrap_or_default();
            let lines = text.lines().collect::<Vec<_>>();
            let begin = (start as usize).min(lines.len());
            // Match the advertised tool contract. Silently cutting a requested
            // 400-line read to 120 forces repeated paid prefix replays.
            let line_cap = 399;
            let end = (requested_end as usize).min(lines.len()).max(begin).min(begin.saturating_add(line_cap));
            let mut numbered = String::new();
            for (offset, line) in lines[begin.saturating_sub(1)..end].iter().enumerate() {
                numbered.push_str(&format!("{:>5} {}\n", begin + offset, line));
            }
            let mut output = json!({
                "path": result["path"],
                "startLine": begin.max(1),
                "endLine": end,
                "totalLines": lines.len(),
                "sourceSha256":format!("{:x}",Sha256::digest(text.as_bytes())),
                "content": numbered,
            });
            if let Some(observation) = result.get("fileObservation") {
                output["fileObservation"] = observation.clone();
            }
            Ok(output)
        }
        "code_search" => {
            let query = required_arg(args, "query")?;
            let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 50) as usize;
            search_code_index(ctx, &query, limit)
        }
        "workspace_replace" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            replace_text(
                ctx,
                &path,
                &required_arg(args, "oldText")?,
                &required_arg(args, "newText")?,
                args["expectedSha256"].as_str(),
                run_id,
            )
            .await
        }
        "workspace_replace_range" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            replace_range(
                ctx,
                &path,
                args["startLine"].as_u64().ok_or("startLine must be a positive integer")?,
                args["endLine"].as_u64().ok_or("endLine must be a positive integer")?,
                &required_arg(args, "newText")?,
                &required_arg(args, "expectedSha256")?,
                run_id,
            ).await
        }
        "workspace_write" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            checked_write(
                ctx,
                &path,
                &required_arg(args, "content")?,
                run_id,
                name == "workspace_write",
                args["expectedSha256"].as_str(),
                args["requireAbsent"] == true,
            )
            .await
        }
        "workspace_apply_patch" => {
            let input=required_arg(args,"input")?;
            let targets=patch::target_paths(ctx.workspace,&input)?;
            let locks={
                let mut registry=ctx.state.file_mutations.lock().unwrap();
                registry.retain(|_,entry|entry.strong_count()!=0);
                targets.iter().map(|path|{
                    if let Some(lock)=registry.get(path).and_then(std::sync::Weak::upgrade){
                        lock
                    }else{
                        let lock=std::sync::Arc::new(tokio::sync::Mutex::new(()));
                        registry.insert(path.clone(),std::sync::Arc::downgrade(&lock));
                        lock
                    }
                }).collect::<Vec<_>>()
            };
            let mut guards=Vec::with_capacity(locks.len());
            for lock in &locks {guards.push(lock.lock().await);}
            let result=patch::apply(ctx.workspace,&input).await?;
            // Large repositories are indexed lazily. Mark the existing index
            // stale only when bytes actually changed, including partial
            // commits, so the next code_search refreshes before navigating.
            if result["changed"]==true {
                if let Ok(workspace)=ctx.workspace.canonicalize(){
                    let id=crate::engine::code_index::index_id(&workspace);
                    if let Some(mut index)=ctx.store.get("code_index",&id){
                        index["dirty"]=json!(true);
                        ctx.store.put("code_index",&index);
                    }
                }
            }
            Ok(result)
        },
        "file_list" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            list_dir(&path).await
        }
        "file_read" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            observed_read(ctx, &path, run_id).await
        }
        "file_write" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            checked_write(
                ctx,
                &path,
                &required_arg(args, "content")?,
                run_id,
                name == "workspace_write",
                args["expectedSha256"].as_str(),
                args["requireAbsent"] == true,
            )
            .await
        }
        "shell_exec" => {
            let command = required_arg(args, "command")?;
            let cwd = match string_arg(args, "cwd") {
                Some(dir) => PathBuf::from(dir),
                None => ctx.workspace.to_path_buf(),
            };
            shell(&command, &cwd).await
        }
        "terminal_exec" => {
            let command = required_arg(args, "command")?;
            let argv: Vec<String> = args
                .get("args")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            executable(&command, &argv, ctx.workspace).await
        }
        "browser_control" => {
            let action = required_arg(args, "action")?;
            let owner_id=format!("{}:{}",ctx.agent_id.unwrap_or("user"),ctx.session_id);
            crate::engine::browser::execute_for_owner(
                &action,
                args,
                &ctx.state.config.data_dir,
                ctx.workspace,
                &owner_id,
            )
            .await
        }
        "web_search" => {
            let query = required_arg(args, "query")?;
            web_search(&query).await
        }
        "http_fetch" => {
            let url = required_arg(args, "url")?;
            let offset = args["offset"].as_u64().unwrap_or(0).min(1_000_000) as usize;
            let page_chars = args["maxChars"].as_u64().unwrap_or(6_000).clamp(1_000,16_000) as usize;
            http_fetch_page(&url, offset, page_chars).await
        }
        "memory_store" => {
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            crate::engine::memory::store_tool(ctx.store, &scope, args)
        }
        "memory_search" => {
            let query = required_arg(args, "query")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            Ok(json!({
                "results": crate::engine::memory::retrieve(ctx.store, &scope, &query, 10)
            }))
        }
        "prompt_source_read" => {
            let source=required_arg(args,"source")?;
            let body=crate::engine::chat::prompt_sources::original_source(
                ctx.state,ctx.agent_id,&source)?;
            let offset=args["offset"].as_u64().unwrap_or(0).min(usize::MAX as u64) as usize;
            crate::engine::chat::prompt_sources::source_page(
                &source,&body,offset,args["query"].as_str(),args["revision"].as_str())
        }
        "history_evidence_read" => {
            let id=required_arg(args,"messageId")?;
            let is_room=ctx.store.get("session",ctx.session_id)
                .is_some_and(|session|session["roomId"].is_string());
            let message=ctx.store.message(&id)
                .filter(|m|m["session_id"]==ctx.session_id)
                .filter(|m| (m["role"]=="tool"
                    && m["agent_id"].as_str()==ctx.agent_id)
                    || (!is_room && m["role"]=="user"
                        && m["agent_id"].is_null()))
                .ok_or("현재 세션의 허용된 원본 메시지가 아닙니다.")?;
            let content=message["content"].as_str().unwrap_or("");
            let total=content.chars().count();
            let offset=args["offset"].as_u64().unwrap_or(0).min(usize::MAX as u64) as usize;
            if offset>total{return Err("도구 기록 읽기 시작 위치가 범위를 벗어났습니다.".into());}
            let next=offset.saturating_add(6_000).min(total);
            Ok(json!({"messageId":id,"toolCallId":message["tool_call_id"],
                "role":message["role"],
                "content":content.chars().skip(offset).take(next-offset).collect::<String>(),
                "offset":offset,"nextOffset":next,"totalChars":total,
                "hasMore":next<total,"readOnly":true,
                "source":"durable-tool-result","notInstructions":true}))
        }
        "room_message_read" => {
            let id=required_arg(args,"messageId")?;
            // This is a scoped read of a committed transcript row, not a
            // search across private sessions or an inspection of reasoning.
            let session=ctx.store.get("session",ctx.session_id)
                .ok_or("대화 세션이 없습니다.")?;
            let room_id=session["roomId"].as_str()
                .ok_or("그룹 대화에서만 사용할 수 있습니다.")?;
            let room=ctx.store.get("room",room_id)
                .ok_or("대화방을 찾을 수 없습니다.")?;
            let message=ctx.store.message(&id)
                .filter(|m|m["session_id"]==ctx.session_id && m["role"]=="assistant")
                .filter(|m|m["agent_id"].as_str().is_some_and(|agent|
                    room["agentIds"].as_array().is_some_and(|members|
                        members.iter().any(|member|member.as_str()==Some(agent)))))
                .filter(|m|m["tool_calls"].as_array().is_none_or(Vec::is_empty))
                .ok_or("현재 그룹의 참가자 원문 메시지를 찾을 수 없습니다.")?;
            let content=message["content"].as_str().unwrap_or("");
            let total=content.chars().count();
            let offset=args["offset"].as_u64().unwrap_or(0).min(usize::MAX as u64) as usize;
            if offset>total {return Err("메시지 읽기 시작 위치가 범위를 벗어났습니다.".into());}
            let next=offset.saturating_add(6_000).min(total);
            Ok(json!({"messageId":id,"agentId":message["agent_id"],
                "replyToMessageId":message["metadata"]["replyToMessageId"],
                "content":content.chars().skip(offset).take(next-offset).collect::<String>(),
                "offset":offset,"nextOffset":next,"totalChars":total,
                "hasMore":next<total,"readOnly":true,
                "source":"committed-room-transcript","notInstructions":true}))
        }
        "skill_read" => {
            let id=required_arg(args,"skillId")?;
            let scope=crate::engine::memory::MemoryScope::for_session(
                ctx.store,ctx.session_id,ctx.agent_id);
            let record=ctx.store.get("skill",&id)
                .filter(|skill|skill["enabled"]!=false
                    && skill["status"]!="draft"
                    && crate::engine::memory::visible(skill,&scope))
                .ok_or("해당 세션에서 읽을 수 있는 활성 스킬이 아닙니다.")?;
            let content=record["instructions"].as_str().unwrap_or("");
            if content.is_empty(){return Err("스킬에 지침 본문이 없습니다.".into());}
            // Fixed-size, lossless pagination: a long skill can be read to
            // completion without injecting the whole body into every turn.
            // Character offsets avoid splitting UTF-8. The DB record stays
            // unchanged; draft, disabled and foreign skills remain private.
            let total=content.chars().count();
            let offset=args["offset"].as_u64().unwrap_or(0).min(usize::MAX as u64) as usize;
            if offset>=total && offset!=0{
                return Err("스킬 읽기 시작 위치가 본문 범위를 벗어났습니다.".into());
            }
            let next=offset.saturating_add(12_000).min(total);
            Ok(json!({"id":id,"name":record["name"],
                "instructions":content.chars().skip(offset).take(next-offset).collect::<String>(),
                "offset":offset,"nextOffset":next,"totalChars":total,
                "hasMore":next<total,"truncated":next<total,
                "readOnly":true}))
        }
        "skill_propose" => {
            let name = required_arg(args, "name")?;
            let instructions = required_arg(args, "instructions")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            let record = ctx.store.put(
                "skill",
                &json!({
                    "name": name,
                    "instructions": instructions,
                    "description": instructions.chars().take(160).collect::<String>(),
                    "enabled": false,
                    "status": "draft",
                    "source": "agent",
                    "scope": "agent",
                    "sessionId": ctx.session_id,
                    "agentId": ctx.agent_id,
                    "partitionKey": scope.partition_key,
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
            Ok(json!({ "proposed": true, "skill": record }))
        }
        "room_peer_send" => room_peer::send(ctx,args,run_id),
        "room_peer_inbox" => room_peer::inbox(ctx),
        "team_execute" => team::execute(ctx,args,run_id).await,
        "team_status" => team::status(ctx,args),
        "team_remote_apply" => remote_artifacts::apply(ctx,args,run_id).await,
        "team_workers" => Ok(team::workers(ctx)),
        "delegate_task" => delegate(ctx, args).await,
        "external_harness_run" => external_harness::run(ctx,args).await,
        "external_harness_sessions" => Ok(external_harness::sessions(ctx)),
        "delegate_parallel" => delegate_parallel(ctx,args).await,
        "task_status" => task_payload(ctx, args, false),
        "task_result" => task_payload(ctx, args, true),
        "account_status" => Ok(json!({
            "providers": crate::engine::oauth::status(ctx.state),
        })),
        "github_api" => github_api(ctx, args).await,
        "github_repositories" => crate::engine::github_work::repositories(ctx.state,args).await,
        "github_file_read" => crate::engine::github_work::read(ctx.state,args).await,
        "github_file_commit" => crate::engine::github_work::commit(ctx.state,args).await,
        "github_repository_activity" => crate::engine::github_work::activity(ctx.state,args).await,
        "github_issue_create" => crate::engine::github_work::issue_create(ctx.state,args).await,
        "github_branch_create" => crate::engine::github_work::branch_create(ctx.state,args).await,
        "github_pr_create" => crate::engine::github_work::pr_create(ctx.state,args).await,
        "google_drive_search" => crate::engine::google_work::drive_search(ctx.state,args).await,
        "google_docs_read" => crate::engine::google_work::docs_read(ctx.state,args).await,
        "google_docs_replace" => crate::engine::google_work::docs_replace(ctx.state,args).await,
        "google_sheets_read" => crate::engine::google_work::sheets_read(ctx.state,args).await,
        "google_sheets_update" => crate::engine::google_work::sheets_update(ctx.state,args).await,
        other => Err(format!("Unknown tool: {other}")),
    }
}

async fn github_api(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let token = crate::engine::oauth::fresh_access_token(ctx.state,"github").await
        .map_err(|error|format!("{error} GitHub 계정을 다시 연결하거나 권한을 확인하세요."))?;
    let method = string_arg(args, "method").unwrap_or("GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
        return Err("지원하지 않는 HTTP 메서드입니다.".into());
    }
    let path = required_arg(args, "path")?;
    if !path.starts_with('/') || path.contains("..") {
        return Err("GitHub API 경로는 / 로 시작해야 합니다.".into());
    }
    let method =
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| error.to_string())?;
    let mut request = reqwest::Client::new()
        .request(method, format!("https://api.github.com{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(30));
    if let Some(body) = args.get("body").filter(|value| !value.is_null()) {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
    Ok(json!({
        "status": status,
        "body": parsed.to_string().chars().take(20_000).collect::<String>(),
    }))
}

const MAX_DELEGATION_DEPTH: usize = 4;

/// Number of delegated sessions between the owner-facing session and this one.
/// A chain cannot loop forever, so a cycle is cut defensively.
fn delegation_depth(store: &Store, session_id: &str) -> usize {
    let mut depth = 0;
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(session_id.to_string());
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(session) = store.get("session", &id) else {
            break;
        };
        if session["delegated"].as_bool() == Some(true) {
            depth += 1;
        }
        current = session["parentSessionId"].as_str().map(str::to_string);
    }
    depth
}

async fn delegate(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let agent_id = required_arg(args, "agentId")?;
    // A delegated worker cannot delegate beyond a bounded depth. This keeps a
    // runaway chain from consuming unbounded workers.
    if delegation_depth(ctx.store, ctx.session_id) >= MAX_DELEGATION_DEPTH {
        return Err(format!(
            "위임 깊이 제한({MAX_DELEGATION_DEPTH})에 도달했습니다. 상위 작업으로 결과를 모아 처리하세요."
        ));
    }
    let prompt = required_arg(args, "prompt")?;
    let agent = ctx
        .state
        .store
        .get("agent", &agent_id)
        .ok_or_else(|| format!("위임 대상 에이전트를 찾을 수 없습니다: {agent_id}"))?;
    let wait = args["wait"].as_bool().unwrap_or(true);
    let read_only=args["readOnly"]==true;
    let timeout = args["timeoutSeconds"]
        .as_u64()
        .unwrap_or(90)
        .clamp(5, 300);
    // An isolated parent project must survive async child queue dispatch and
    // process restart. The in-memory scoped AppState alone is insufficient:
    // the global scheduler may claim a child before start_now runs.
    let inherited_workspace=ctx.store.get("session",ctx.session_id)
        .and_then(|session|session["workspaceDir"].as_str().map(str::to_string));
    let mut child_session=json!({
        "title": format!("Delegate · {}", prompt.chars().take(36).collect::<String>()),
        "agentId": agent_id,
        "providerId": agent["providerId"],
        "model": agent["model"],
        "parentSessionId": ctx.session_id,
        "teamId": args["teamId"],
        "teamNodeId": args["teamNodeId"],
        "delegated": true,
        "parallelReadOnly":read_only,
    });
    if let Some(workspace)=inherited_workspace.as_deref(){
        child_session["workspaceDir"]=json!(workspace);
    }
    let session = ctx
        .state
        .store
        .put("session",&child_session)
        .ok_or("위임 세션을 만들 수 없습니다.")?;
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let mut child_input=json!({
            "prompt": prompt,
            "agentId": agent_id,
            "providerId": agent["providerId"],
            "model": agent["model"],
            "sessionId": session_id,
            "toolsEnabled": ctx.tools_enabled && ctx.tools_execution_enabled,
            "toolsExecutionEnabled": ctx.tools_execution_enabled,
            "parentSessionId": ctx.session_id,
            "teamId": args["teamId"],
            "teamNodeId": args["teamNodeId"],
            "parentAgentId": ctx.agent_id,
            "delegation": true,
            "parallelDelegationReadOnly":read_only,
            "maxAutonomousAttempts":if read_only {2}else{3},
    });
    if let Some(criteria)=args.get("successCriteria").filter(|v|!v.is_null()){
        crate::engine::verification::validate_criteria(criteria)?;
        child_input["successCriteria"]=criteria.clone();
    }
    if let Some(workspace)=inherited_workspace{
        child_input["workspaceDir"]=json!(workspace);
    }
    let job = crate::engine::jobs::enqueue(ctx.state,"prompt",child_input)
        .ok_or("위임 작업을 만들 수 없습니다.")?;
    let job_id = job["id"].as_str().unwrap_or("").to_string();
    crate::engine::jobs::start_now(ctx.state.clone(), job_id.clone());

    if !wait {
        return Ok(json!({
            "jobId": job_id, "status": "queued", "sessionId": session_id,
            "hint": "task_status 또는 task_result로 확인하세요."
        }));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        if ctx.state.is_cancelled(ctx.session_id) {
            return Err("실행이 중단되었습니다. 위임 작업은 백그라운드에서 계속됩니다.".into());
        }
        let Some(current) = ctx.state.store.get("job", &job_id) else {
            return Err("위임 작업 기록이 사라졌습니다.".into());
        };
        let status = current["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed" | "cancelled" | "needs_review") {
            return task_payload(ctx, &json!({"jobId": job_id}), true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(json!({
                "jobId": job_id, "status": status, "sessionId": session_id,
                "hint": "아직 실행 중입니다. task_result로 나중에 확인하세요."
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Fan out independent investigation across dedicated agent sessions. All
/// children start before waiting, so the job engine executes them in parallel
/// instead of blocking the parent on the first child.
async fn delegate_parallel(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let tasks=args["tasks"].as_array().ok_or("tasks must be an array")?;
    if !(2..=4).contains(&tasks.len()){
        return Err("병렬 작업은 2~4개를 동시에 요청하세요.".into());
    }
    if delegation_depth(ctx.store,ctx.session_id)>=MAX_DELEGATION_DEPTH {
        return Err("병렬 위임의 최대 깊이에 도달했습니다.".into());
    }
    let historical=ctx.store.all("job").iter()
        .filter(|job|job["input"]["parentSessionId"].as_str()==Some(ctx.session_id)
            && job["input"]["parallelDelegationReadOnly"]==true).count();
    if historical.saturating_add(tasks.len())>8{
        return Err("한 부모 세션에서 병렬 조사 작업은 최대 8개입니다.".into());
    }
    // Validate every candidate before side-effecting queue writes. Invalid
    // agent IDs or oversized prompts must not leave a partial orphan fan-out.
    for task in tasks {
        let agent=task["agentId"].as_str()
            .filter(|s|!s.trim().is_empty()).ok_or("agentId is required")?;
        let prompt=task["prompt"].as_str()
            .filter(|s|!s.trim().is_empty()&&s.len()<=4000)
            .ok_or("각 하위 지시는 1~4000바이트여야 합니다.")?;
        if ctx.store.get("agent",agent).is_none(){
            return Err(format!("위임 대상 에이전트를 찾을 수 없습니다: {agent}"));
        }
        if prompt.trim().is_empty(){return Err("하위 작업 지시가 비어 있습니다.".into());}
    }
    let mut job_ids=Vec::with_capacity(tasks.len());
    for task in tasks {
        let created=delegate(ctx,&json!({
            "agentId":task["agentId"],"prompt":task["prompt"],
            "readOnly":true,"wait":false,
        })).await?;
        let job_id=created["jobId"].as_str().ok_or("위임 작업 ID가 없습니다.")?;
        job_ids.push(job_id.to_string());
    }
    let wait=args["wait"].as_bool().unwrap_or(true);
    let timeout=args["timeoutSeconds"].as_u64().unwrap_or(120).clamp(5,300);
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(timeout);
    if wait {
        loop {
            if ctx.state.is_cancelled(ctx.session_id) {
                return Err("부모 작업이 취소되었습니다. 하위 작업 ID는 영구 작업 기록에 남습니다.".into());
            }
            let finished=job_ids.iter().all(|id|
                ctx.store.get("job",id).is_some_and(|job|matches!(
                    job["status"].as_str(),
                    Some("completed"|"failed"|"cancelled"|"needs_review")
                )));
            if finished || std::time::Instant::now()>=deadline{break;}
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    let results=job_ids.iter().map(|id|
        task_payload(ctx,&json!({"jobId":id}),true)).collect::<Result<Vec<_>,_>>()?;
    let complete=results.iter().all(|result|matches!(
        result["status"].as_str(),Some("completed"|"failed"|"cancelled"|"needs_review")
    ));
    // The parent needs a synthesis, not four complete raw tool transcripts.
    // Explicit task_result retains complete evidence on demand.
    let summaries=results.iter().map(summarize_parallel_result).collect::<Vec<_>>();
    Ok(json!({
        "status":if complete{"completed"}else{"partial"},
        "parallel":true,
        "readOnly":true,
        "tasks":summaries,
        "hint":if complete{"검증 기록을 바탕으로 결과를 종합하세요."}else{
            "일부 작업이 진행 중입니다. task_status/task_result로 이어서 확인하세요."
        },
    }))
}

/// Keep parallel fan-in context bounded independently of child log size.
fn summarize_parallel_result(result:&Value)->Value {
    let brief=result["content"].as_str().unwrap_or("").chars()
        .take(1800).collect::<String>();
    let reason=result["error"].as_str().unwrap_or("").chars()
        .take(400).collect::<String>();
    json!({
        "jobId":result["jobId"],
        "status":result["status"],
        "verified":result["result"]["verified"]==true,
        "answer":brief,
        "error":reason,
        "evidenceCount":result["operations"].as_array().map_or(0,Vec::len),
        "fullReceipt":"task_result(jobId)",
    })
}

fn task_payload(ctx: &ToolContext<'_>, args: &Value, with_result: bool) -> Result<Value, String> {
    let job_id = required_arg(args, "jobId")?;
    let job = ctx
        .state
        .store
        .get("job", &job_id)
        .ok_or_else(|| format!("작업을 찾을 수 없습니다: {job_id}"))?;
    // A job result is visible only to the session that created it or to the
    // parent session that delegated it.
    let owned = job["input"]["sessionId"].as_str() == Some(ctx.session_id)
        || job["input"]["parentSessionId"].as_str() == Some(ctx.session_id);
    if !owned {
        return Err("이 작업에 접근할 권한이 없습니다.".into());
    }
    let status = job["status"].as_str().unwrap_or("unknown");
    let mut payload = json!({
        "jobId": job_id,
        "kind": job["kind"],
        "status": status,
        "stepIndex": job["stepIndex"],
        "stepTotal": job["stepTotal"],
        "error": job["error"],
        "result": job["result"],
    });
    if !with_result || !matches!(status, "completed" | "failed" | "needs_review") {
        return Ok(payload);
    }
    let Some(session_id) = job
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .or_else(|| job["sessionId"].as_str())
    else {
        return Ok(payload);
    };
    let messages = ctx.state.store.messages(session_id);
    let answer = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .and_then(|message| message["content"].as_str())
        .unwrap_or("")
        .chars()
        .take(8000)
        .collect::<String>();
    // Delegated result queries must not deserialize every historical tool
    // receipt across every session. Fetch only executions for this session,
    // then use the indexed runId lookup to collect its bounded evidence.
    let mut operations=Vec::new();
    let executions=ctx.state.store.session_executions(session_id)?;
    for execution in executions {
        if let Some(run_id)=execution["id"].as_str(){
            let items=ctx.state.store.execution_operations(run_id)?;
            for operation in items {
                if operation["status"]=="completed" {
                    operations.push(json!({
                        "tool":operation["tool"],
                        "arguments":operation["arguments"],
                        "result":operation["result"],
                    }));
                    if operations.len()>=20 {break;}
                }
            }
        }
        if operations.len()>=20 {break;}
    }
    payload["sessionId"] = json!(session_id);
    payload["content"] = json!(answer);
    payload["operations"] = json!(operations);
    Ok(payload)
}

fn required_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("Missing argument: {key}"))
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|value| value.as_str())
}

pub const WORKSPACE_ESCAPE: &str = "Path escapes the workspace";

/// Reject `..` traversal and symlink escapes for workspace-scoped tools.
pub(super) fn resolve_workspace(workspace: &Path, relative: &str) -> Result<PathBuf, String> {
    let base = canonical_target(workspace).map_err(|error| error.to_string())?;
    let candidate = Path::new(relative);
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(WORKSPACE_ESCAPE.into());
    }
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };
    let resolved = canonical_target(&joined).map_err(|error| error.to_string())?;
    if !resolved.starts_with(&base) {
        return Err(WORKSPACE_ESCAPE.into());
    }
    Ok(resolved)
}

/// Resolve existing ancestors even when the final file has not been created.
fn canonical_target(path: &Path) -> std::io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(error);
            }
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(error);
            };
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            Ok(canonical_target(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

async fn list_dir(path: &Path) -> Result<Value, String> {
    let mut entries = Vec::new();
    let mut reader = tokio::fs::read_dir(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    while let Ok(Some(entry)) = reader.next_entry().await {
        let metadata = entry.metadata().await.ok();
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "type": if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) { "dir" } else { "file" },
            "size": metadata.as_ref().map(|m| m.len()).unwrap_or(0),
        }));
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    Ok(json!({ "path": path.to_string_lossy(), "entries": entries }))
}

async fn read_text(path: &Path) -> Result<Value, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err("Target is not a regular file".into());
    }
    if metadata.len() > 1_500_000 {
        return Err("File is larger than 1.5 MB".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy(), "content": text }))
}

fn preflight_policy(ctx: &ToolContext<'_>, path: &Path, run_id: &str) -> Option<Value> {
    let run = ctx.store.get("execution", run_id)?;
    let policy = &run["preflightPolicy"];
    (crate::engine::policy::is_preflight_kernel(&policy["parameters"]["kernel"])
        && policy["targetPath"].as_str().is_none_or(|target| {
            let target = Path::new(target);
            path == target || canonical_target(path).is_ok_and(|path| path == target)
        }))
        .then(|| policy.clone())
}

async fn observed_read(ctx: &ToolContext<'_>, path: &Path, run_id: &str) -> Result<Value, String> {
    if preflight_policy(ctx, path, run_id).is_none() {
        return read_text(path).await;
    }
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut result = read_text(&canonical).await?;
    result["path"] = json!(path.to_string_lossy());
    let text = result["content"].as_str().unwrap_or_default();
    result["fileObservation"] = json!({
        "path": canonical.to_string_lossy(),
        "sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
    });
    Ok(result)
}

fn search_code_index(
    ctx: &ToolContext<'_>,
    query: &str,
    limit: usize,
) -> Result<Value, String> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Err("code_search query는 비어 있을 수 없습니다.".into());
    }
    // Index the selected real user workspace, never a detached campaign.
    let meta=match crate::engine::code_index::get(ctx.state,ctx.workspace) {
        Some(meta) if meta["dirty"]!=true=>meta,
        _=>crate::engine::code_index::refresh(ctx.state,ctx.workspace)?,
    };
    let index_id = meta["id"].as_str().unwrap_or("");
    let terms=needle.split_whitespace().map(|term|term.trim_matches(['`','\"','\'','(',')',',']))
        .filter(|term|!term.is_empty() && !matches!(*term,"fn"|"pub"|"async"|"struct"|"enum"))
        .collect::<Vec<_>>();
    let score=|value:&str|{
        let value=value.to_lowercase();
        if value==needle {1000} else {
            u64::from(value.contains(&needle))*200 + terms.iter().map(|term|
                if value==*term {100} else {u64::from(value.contains(term))*20}).sum::<u64>()
        }
    };
    let mut matches=Vec::new();
    for file in ctx.store.all("code_index_file") {
        if file["indexId"].as_str()!=Some(index_id) {continue;}
        let path=file["path"].as_str().unwrap_or("");
        let rank=score(path);
        if rank>0 {matches.push((rank,json!({"path":path,"kind":"file","name":path,"line":1})));}
        for symbol in file["symbols"].as_array().into_iter().flatten() {
            let name=symbol["name"].as_str().unwrap_or("");let rank=score(name);
            if rank==0 {continue;}
            matches.push((rank,json!({"path":path,"kind":symbol["kind"],"name":name,"line":symbol["line"]})));
        }
    }
    // Literal diagnostics and field names often are not symbol names. Search
    // the actual workspace bytes when the index has no navigation hit.
    if matches.is_empty() {
        for file in ctx.store.all("code_index_file") {
            if file["indexId"].as_str()!=Some(index_id) {continue;}
            let Some(path)=file["path"].as_str() else {continue;};
            let Ok(full)=resolve_workspace(ctx.workspace,path) else {continue;};
            let Ok(content)=std::fs::read_to_string(full) else {continue;};
            for (index,line) in content.lines().enumerate().filter(|(_,line)|line.to_lowercase().contains(&needle)).take(limit) {
                matches.push((100,json!({"path":path,"kind":"literal","line":index+1,
                    "name":line.trim().chars().take(180).collect::<String>()})));
            }
        }
    }
    matches.sort_by(|(a,left),(b,right)|b.cmp(a)
        .then_with(||left["path"].as_str().cmp(&right["path"].as_str()))
        .then_with(||left["line"].as_u64().cmp(&right["line"].as_u64())));
    matches.dedup_by(|(_,a),(_,b)|a==b);
    matches.truncate(limit);
    let matches=matches.into_iter().map(|(_,row)|row).collect::<Vec<_>>();
    Ok(json!({"query":query,"indexId":index_id,"indexedFiles":meta["files"],"matchCount":matches.len(),"matches":matches}))
}

async fn replace_text(
    ctx: &ToolContext<'_>,
    path: &Path,
    old_text: &str,
    new_text: &str,
    expected_sha256: Option<&str>,
    run_id: &str,
) -> Result<Value, String> {
    if old_text.is_empty() {
        return Err("oldText는 비어 있을 수 없습니다.".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let occurrences = text.matches(old_text).count();
    if occurrences != 1 {
        return Ok(json!({
            "error": "oldText가 파일 안에서 정확히 한 번만 나타나야 합니다.",
            "occurrences": occurrences,
            "path": path.to_string_lossy(),
        }));
    }
    let updated = text.replacen(old_text, new_text, 1);
    let mut result = checked_write(ctx, path, &updated, run_id, true, expected_sha256, false).await?;
    result["replaced"] = json!(true);
    result["replacedChars"] = json!(old_text.chars().count());
    if result["written"] == true { result["postSha256"] = json!(format!("{:x}",Sha256::digest(updated.as_bytes()))); }
    Ok(result)
}

/// A token-efficient edit path: the model emits changed lines, not a second
/// copy of source text already returned by workspace_read_range. The frozen
/// file digest is checked both before assembling the patch and again under
/// the regular per-file write lock, so concurrent edits cannot be lost.
async fn replace_range(
    ctx: &ToolContext<'_>, path: &Path, start: u64, end: u64,
    new_text: &str, expected_sha256: &str, run_id: &str,
) -> Result<Value, String> {
    if start == 0 || end < start || end.saturating_sub(start) >= 120 {
        return Err("Replace range must contain 1-120 inclusive source lines".into());
    }
    if expected_sha256.len() != 64
        || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("expectedSha256 must be the sourceSha256 from workspace_read_range".into());
    }
    if new_text.len() > 64_000 {
        return Err("Replacement is larger than the source edit limit".into());
    }
    let source = tokio::fs::read_to_string(path)
        .await.map_err(|error| format!("{}: {error}", path.display()))?;
    let digest = format!("{:x}", Sha256::digest(source.as_bytes()));
    if digest != expected_sha256 {
        return Ok(json!({"error":"Source changed since the observed range",
            "staleVersion":true, "expectedSha256":expected_sha256,
            "observedSha256":digest, "path":path.to_string_lossy()}));
    }
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let from = (start - 1) as usize;
    let to = end as usize;
    if to > lines.len() {
        return Err(format!("Line range {start}..{end} exceeds {} lines; read a valid range first", lines.len()));
    }
    let before = lines[..from].iter().map(|part| part.len()).sum::<usize>();
    let after = lines[..to].iter().map(|part| part.len()).sum::<usize>();
    let old = &source[before..after];
    let mut replacement = new_text.to_string();
    if !replacement.is_empty() && old.ends_with("\r\n") && !replacement.ends_with('\n') {
        replacement.push_str("\r\n");
    } else if !replacement.is_empty() && old.ends_with('\n') && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    let updated = format!("{}{}{}", &source[..before], replacement, &source[after..]);
    if updated == source {
        return Ok(json!({"path":path.to_string_lossy(), "written":false, "unchanged":true}));
    }
    let mut result = checked_write(ctx, path, &updated, run_id, true, Some(expected_sha256), false).await?;
    if result["written"] == true {
        result["replaced"] = json!(true);
        result["replacedLines"] = json!(end-start+1);
        result["postSha256"] = json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    }
    Ok(result)
}

async fn checked_write(
    ctx: &ToolContext<'_>,
    path: &Path,
    content: &str,
    run_id: &str,
    workspace_write: bool,
    expected_sha256: Option<&str>,
    require_absent: bool,
) -> Result<Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let policy = preflight_policy(ctx, path, run_id);
    let canonical = canonical_target(path);
    let lock_path = match &canonical {
        Ok(path) => path.as_path(),
        Err(error) if policy.is_some() => return Err(error.to_string()),
        Err(_) => path,
    };
    let lock = {
        let mut locks = ctx.state.file_mutations.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = locks.get(lock_path).and_then(std::sync::Weak::upgrade) {
            lock
        } else {
            let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
            locks.insert(lock_path.to_path_buf(), std::sync::Arc::downgrade(&lock));
            lock
        }
    };
    let _guard = lock.lock().await;
    if require_absent {
        let target=canonical.as_ref().map_err(|e|e.to_string())?;
        // The candidate was tested against a missing file. Use create_new
        // under the same native lock; do not overwrite a file created since.
        if tokio::fs::symlink_metadata(target).await.is_ok() {
            return Ok(json!({"error":"candidate target now exists",
                "staleVersion":true,"expectedAbsent":true}));
        }
        let parent=target.parent().ok_or("candidate target has no parent")?;
        tokio::fs::create_dir_all(parent).await.map_err(|e|e.to_string())?;
        let mut file=match tokio::fs::OpenOptions::new()
            .write(true).create_new(true).open(target).await {
            Ok(file)=>file,
            Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>
                return Ok(json!({"error":"candidate target appeared during commit",
                    "staleVersion":true,"expectedAbsent":true})),
            Err(error)=>return Err(error.to_string()),
        };
        file.write_all(content.as_bytes()).await.map_err(|e|e.to_string())?;
        file.flush().await.map_err(|e|e.to_string())?;
        return Ok(json!({"path":path.to_string_lossy(),"written":true,
            "bytes":content.len(),"changed":true,
            "postSha256":format!("{:x}",Sha256::digest(content.as_bytes())),
            "conditionalAbsent":true}));
    }
    let Some(policy) = policy else {
        // Even without the opt-in guard, a caller that supplies a frozen
        // version gets a compare-before-write under the same per-file lock.
        let Some(expected) = expected_sha256 else {
            return write_text(path, content).await;
        };
        let canonical = canonical.map_err(|error| error.to_string())?;
        let bytes = match tokio::fs::read(&canonical).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(json!({
                    "error": format!("파일을 다시 읽지 못했습니다: {error}"),
                    "staleVersion": true,
                    "expectedSha256": expected,
                }))
            }
        };
        let observed = format!("{:x}", Sha256::digest(&bytes));
        if observed != expected {
            return Ok(json!({
                "error": "파일이 읽은 뒤 변경되었습니다. 다시 읽고 판단하세요.",
                "staleVersion": true,
                "expectedSha256": expected,
                "observedSha256": observed,
                "path": path.to_string_lossy(),
            }));
        }
        tokio::fs::write(&canonical, content)
            .await
            .map_err(|error| format!("{}: {error}", path.display()))?;
        return Ok(json!({
            "path": path.to_string_lossy(),
            "bytes": content.len(),
            "written": true,
            "conditionalSha256": observed,
            "postSha256": format!("{:x}",Sha256::digest(content.as_bytes())),
            "changed": observed!=format!("{:x}",Sha256::digest(content.as_bytes())),
        }));
    };
    let canonical = canonical.map_err(|error| error.to_string())?;
    let blocked = |reason: &str| {
        json!({"error": format!("Preflight: {reason}. Read this exact file with workspace_read or file_read, then retry with its current content."),
            "preflightBlocked": true, "preflightRevision": policy["revision"]})
    };
    let target = canonical.to_string_lossy();
    let operations = ctx.store.execution_operations(run_id)?;
    let previous = operations.iter().rev().find(|operation| {
        (operation["status"] == "completed" || operation["result"]["preflightBlocked"] == true)
            && (operation["fileObservation"]["path"].as_str() == Some(target.as_ref())
                || operation["result"]["preflightPath"].as_str() == Some(target.as_ref()))
    });
    let metadata = match tokio::fs::metadata(&canonical).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if previous.is_some() {
                return Ok(blocked("the previously observed target disappeared"));
            }
            if let Some(parent) = canonical
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            // An absent target can be created, but must not overwrite a file
            // that appeared between the probe and the open.
            let mut file = match tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&canonical)
                .await
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Ok(blocked("the target now exists"));
                }
                Err(error) => return Err(error.to_string()),
            };
            file.write_all(content.as_bytes())
                .await
                .map_err(|error| error.to_string())?;
            file.flush().await.map_err(|error| error.to_string())?;
            return Ok(
                json!({"path": path.to_string_lossy(), "bytes": content.len(), "written": true,
                "preflightPath": target,
                "postSha256":format!("{:x}",Sha256::digest(content.as_bytes())),
                "changed":true}),
            );
        }
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() {
        return Err("Preflight target is not a regular file".into());
    }
    let revision = previous.and_then(|operation| operation["fileObservation"]["sha256"].as_str());
    let inline_observation = workspace_write
        && policy["parameters"]["kernel"] == crate::engine::preflight_fixture::GUARD_KERNEL
        && metadata.len() <= 16 * 1024;
    if revision.is_none() && !inline_observation {
        return Ok(blocked("there is no successful read of the existing target in this run"));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&canonical)
        .await
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let read_limit = if revision.is_none() { 16 * 1024 + 1 } else { 1_500_001 };
    (&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    let observed = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() > 1_500_000 || revision != Some(observed.as_str()) {
        let mut result = blocked(if revision.is_some() {
            "the target changed after the read"
        } else {
            "there is no successful read of the existing target in this run"
        });
        if inline_observation && bytes.len() <= 16 * 1024 {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                result["error"] = json!("Preflight: write skipped. The current file is below. Apply only the requested change and retry this write using the same path.");
                result["currentContent"] = json!(text);
                result["fileObservation"] = json!({"path":target,"sha256":observed});
            }
        }
        return Ok(result);
    }
    if let Some(expected) = expected_sha256 {
        if expected != observed {
            let mut result = blocked("the target changed after the frozen version");
            result["staleVersion"] = json!(true);
            result["expectedSha256"] = json!(expected);
            result["observedSha256"] = json!(observed);
            return Ok(result);
        }
    }
    use tokio::io::AsyncSeekExt;
    file.rewind().await.map_err(|error| error.to_string())?;
    file.write_all(content.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    file.flush().await.map_err(|error| error.to_string())?;
    file.set_len(content.len() as u64)
        .await
        .map_err(|error| error.to_string())?;
    Ok(
        json!({"path": path.to_string_lossy(), "bytes": content.len(), "written": true,
        "preflightRevision": policy["revision"], "preflightPath": target, "observedSha256": observed,
        "postSha256":format!("{:x}",Sha256::digest(content.as_bytes())),
        "changed": observed!=format!("{:x}",Sha256::digest(content.as_bytes()))}),
    )
}

async fn write_text(path: &Path, content: &str) -> Result<Value, String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    // A successful file operation may be a no-op. Preserve that distinction
    // so the independent verifier never accepts rewriting identical bytes.
    let prior=tokio::fs::read(path).await.ok()
        .map(|bytes|format!("{:x}",Sha256::digest(bytes)));
    tokio::fs::write(path, content)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let digest=format!("{:x}",Sha256::digest(content.as_bytes()));
    Ok(json!({
        "path": path.to_string_lossy(),
        "bytes": content.len(),
        "written": true,
        "postSha256":digest,
        "changed":prior.as_deref()!=Some(digest.as_str())
    }))
}

/// A tool command gets a private Unix process group. Killing only the shell
/// leaves its children running after Stop; the guard covers cancellation,
/// timeout and I/O errors and is disarmed only after a completed command.
#[cfg(unix)]
struct ToolProcessGroup {
    id: Option<i32>,
}
#[cfg(unix)]
impl ToolProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self {
            id: pid
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 1),
        }
    }
    fn disarm(&mut self) {
        self.id = None;
    }
}
#[cfg(unix)]
impl Drop for ToolProcessGroup {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // This is the exact child PID created with process_group(0).
            // A negative PID targets that private group, never our own group;
            // zero and special global process IDs are excluded by new().
            // SIGKILL is 9 on the supported macOS/Linux Unix hosts.
            // SAFETY: the target is the private process group created above;
            // the group id is positive and our own group is never targeted.
            unsafe {
                kill(-id, 9);
            }
        }
    }
}

/// Single native subprocess supervisor, shared by shell, terminal and
/// optional external harness delegation. No second process manager.
async fn read_command_output(
    mut stream:impl tokio::io::AsyncRead+Unpin,
    tail:bool,
)->std::io::Result<(Vec<u8>,bool)>{
    use tokio::io::AsyncReadExt;
    const MAX_RETAINED_BYTES:usize=4*20_001;
    let mut kept=Vec::new();
    let mut buf=[0u8;8192];
    let mut truncated=false;
    loop{
        let n=stream.read(&mut buf).await?;
        if n==0{return Ok((kept,truncated));}
        if tail {
            let overflow=kept.len().saturating_add(n)
                .saturating_sub(MAX_RETAINED_BYTES);
            if overflow>0 {
                truncated=true;
                if overflow>=kept.len(){kept.clear();}
                else{kept.drain(..overflow);}
            }
            kept.extend_from_slice(&buf[..n]);
        }else{
            let amount=n.min(MAX_RETAINED_BYTES.saturating_sub(kept.len()));
            if amount<n{truncated=true;}
            kept.extend_from_slice(&buf[..amount]);
        }
    }
}
struct SupervisedOutput{
    output:std::process::Output,
    stdout_truncated:bool,
    stderr_truncated:bool,
}
async fn run_command_supervised(
    command:&mut tokio::process::Command,
    limit:Duration,
    input:Option<&[u8]>,
)->Result<SupervisedOutput,String>{
    use tokio::io::AsyncWriteExt;
    command
        .stdin(if input.is_some(){std::process::Stdio::piped()}else{
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child=command.spawn().map_err(|error|error.to_string())?;
    #[cfg(unix)]
    let mut group=ToolProcessGroup::new(child.id());
    let stdout=child.stdout.take().ok_or("Command stdout is not available")?;
    let stderr=child.stderr.take().ok_or("Command stderr is not available")?;
    let pipe=child.stdin.take();
    let tail=input.is_some();
    // The stdin write is part of the SAME timeout/cancellation boundary as
    // child exit and pipe draining. A child refusing stdin cannot hang forever.
    let writer=async move {
        if let (Some(mut pipe),Some(bytes))=(pipe,input){
            pipe.write_all(bytes).await?;
            pipe.shutdown().await?;
        }
        Ok::<(),std::io::Error>(())
    };
    let outcome=tokio::time::timeout(limit,async{
        let (status,stdout,stderr,())=tokio::try_join!(
            child.wait(),read_command_output(stdout,tail),
            read_command_output(stderr,tail),writer
        )?;
        Ok::<_,std::io::Error>(SupervisedOutput{
            output:std::process::Output{
                status,stdout:stdout.0,stderr:stderr.0,
            },
            stdout_truncated:stdout.1,stderr_truncated:stderr.1,
        })
    }).await;
    match outcome{
        Ok(Ok(output))=>{
            #[cfg(unix)]
            group.disarm();
            Ok(output)
        },
        Ok(Err(error))=>Err(error.to_string()),
        Err(_)=>Err(format!("Command timed out after {}ms",limit.as_millis())),
    }
}
pub(crate) async fn run_command(
    command:&mut tokio::process::Command,
    limit:Duration,
)->Result<std::process::Output,String>{
    run_command_supervised(command,limit,None).await.map(|result|result.output)
}
pub(crate) async fn run_command_with_input(
    command:&mut tokio::process::Command,
    limit:Duration,
    input:&[u8],
)->Result<(std::process::Output,bool,bool),String>{
    let result=run_command_supervised(command,limit,Some(input)).await?;
    Ok((result.output,result.stdout_truncated,result.stderr_truncated))
}

async fn shell(command: &str, cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    #[cfg(windows)]
    let mut child = {
        let mut child = Command::new("cmd");
        child.args(["/C", command]);
        child
    };
    #[cfg(not(windows))]
    let mut child = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut child = Command::new(shell);
        child.args(["-lc", command]);
        child
    };
    child.current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

async fn executable(command: &str, args: &[String], cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    if !command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:".contains(c))
    {
        return Err("Invalid executable".into());
    }
    let mut child = Command::new(command);
    child.args(args).current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

fn output_payload(output: &std::process::Output) -> Value {
    let clip = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes).to_string();
        if text.chars().count() > 20_000 {
            text.chars().take(20_000).collect::<String>() + "\n… (truncated)"
        } else {
            text
        }
    };
    json!({
        "exitCode": output.status.code(),
        "stdout": clip(&output.stdout),
        "stderr": clip(&output.stderr),
    })
}

const HTTP_FETCH_LIMIT: usize = 1024 * 1024;
const HTTP_REDIRECT_LIMIT: usize = 5;

/// Reject non-public destinations even when URL parsing normalizes unusual
/// IPv4 literals. The same check runs again after every redirect and DNS
/// resolution; no configured proxy can bypass the pinned address.
fn public_http_ip(ip: std::net::IpAddr) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    match ip {
        IpAddr::V4(v4) => {
            let n=v4.octets();
            !v4.is_private() && !v4.is_loopback() && !v4.is_link_local()
                && !v4.is_multicast() && !v4.is_broadcast() && !v4.is_unspecified()
                && !v4.is_documentation()
                && n[0]!=0 && n[0]<224
                && !(n[0]==100 && (64..=127).contains(&n[1]))
                && !(n[0]==192 && n[1]==0 && n[2]==0)
                && !(n[0]==192 && n[1]==88 && n[2]==99)
                && !(n[0]==198 && (18..=19).contains(&n[1]))
            && v4!=Ipv4Addr::new(255,255,255,255)
        }
        IpAddr::V6(v6) => {
            if let Some(mapped)=v6.to_ipv4_mapped() {
                return public_http_ip(IpAddr::V4(mapped));
            }
            let segments=v6.segments();
            !v6.is_loopback() && !v6.is_unspecified() && !v6.is_multicast()
                && !v6.is_unique_local() && !v6.is_unicast_link_local()
                && !(segments[0]==0x2001 && segments[1]==0x0db8)
                && !(segments[0]==0x2001 && segments[1]==0)
        }
    }
}

fn public_http_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed=reqwest::Url::parse(url).map_err(|error|error.to_string())?;
    if !matches!(parsed.scheme(),"http"|"https") {
        return Err("http_fetch는 공개 HTTP(S) URL만 허용합니다.".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("http_fetch URL에 사용자 인증 정보를 넣을 수 없습니다.".into());
    }
    let host=parsed.host_str().ok_or("URL 호스트가 없습니다.")?;
    let name=host.trim_matches(|ch|ch=='['||ch==']').trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name=="localhost" || name.ends_with(".localhost")
        || name.ends_with(".local") || name.ends_with(".internal")
        || name.ends_with(".lan") || name.ends_with(".home.arpa")
        || name.ends_with(".invalid") || name.ends_with(".test") {
        return Err("내부 네트워크 호스트는 http_fetch에서 사용할 수 없습니다.".into());
    }
    if let Ok(ip)=name.parse::<std::net::IpAddr>() {
        if !public_http_ip(ip) {
            return Err("내부 또는 예약된 IP로의 http_fetch 요청을 차단했습니다.".into());
        }
    }
    Ok(parsed)
}

fn append_http_chunk(output:&mut Vec<u8>,chunk:&[u8])->bool {
    let room=HTTP_FETCH_LIMIT.saturating_sub(output.len());
    output.extend_from_slice(&chunk[..chunk.len().min(room)]);
    chunk.len()>room
}

pub(crate) async fn http_fetch_page(url: &str, offset: usize, page_chars: usize) -> Result<Value, String> {
    let mut current=public_http_url(url)?;
    for redirect in 0..=HTTP_REDIRECT_LIMIT {
        let hostname=current.host_str().ok_or("URL 호스트가 없습니다.")?
            .trim_matches(|ch|ch=='['||ch==']').to_string();
        let port=current.port_or_known_default().ok_or("URL 포트가 없습니다.")?;
        let client_builder=reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30));
        // Resolve exactly once and pin that address for this request. Every
        // resolved address must be public to prevent mixed DNS answer bypasses.
        let client_builder=if hostname.parse::<std::net::IpAddr>().is_ok() {
            client_builder
        } else {
            let resolved=tokio::net::lookup_host((hostname.as_str(),port))
                .await.map_err(|error|format!("공개 주소 DNS 조회 실패: {error}"))?
                .collect::<Vec<_>>();
            if resolved.is_empty() || resolved.iter().any(|addr|!public_http_ip(addr.ip())) {
                return Err("DNS가 사설 또는 예약된 네트워크 주소를 반환했습니다.".into());
            }
            client_builder.resolve(&hostname,resolved[0])
        };
        let response=client_builder.build().map_err(|error|error.to_string())?
            .get(current.clone())
            .header("User-Agent","DOMINION/0.2 (+rust)")
            .send().await.map_err(|error|error.to_string())?;
        if response.status().is_redirection() {
            if redirect==HTTP_REDIRECT_LIMIT {
                return Err("http_fetch 리다이렉션 상한을 초과했습니다.".into());
            }
            let location=response.headers().get(reqwest::header::LOCATION)
                .and_then(|value|value.to_str().ok())
                .ok_or("리다이렉션 Location 헤더가 없습니다.")?;
            let next=current.join(location).map_err(|error|error.to_string())?;
            current=public_http_url(next.as_str())?;
            continue;
        }
        let status=response.status().as_u16();
        let content_type=response.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|value|value.to_str().ok()).unwrap_or("").to_string();
        let mut response=response;
        let mut bytes=Vec::new();
        let mut truncated=false;
        while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())? {
            if append_http_chunk(&mut bytes,&chunk) {
                truncated=true;
                break;
            }
        }
        let body=String::from_utf8_lossy(&bytes);
        let readable=if content_type.to_ascii_lowercase().contains("html") {
            strip_html(&body)
        } else {
            body.into_owned()
        };
        let mut page = text_page(&readable, offset, page_chars);
        page["status"] = json!(status);
        page["contentType"] = json!(content_type);
        page["truncated"] = json!(truncated);
        page["maxResponseBytes"] = json!(HTTP_FETCH_LIMIT);
        page["redirects"] = json!(redirect);
        return Ok(page);
    }
    Err("http_fetch 리다이렉션 상한을 초과했습니다.".into())
}

async fn web_search(query: &str) -> Result<Value, String> {
    let response = reqwest::Client::new()
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .timeout(Duration::from_secs(25))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let html = response.text().await.map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    for chunk in html.split("result__a").skip(1).take(10) {
        let Some(href_start) = chunk.find("href=\"") else {
            continue;
        };
        let rest = &chunk[href_start + 6..];
        let Some(href_end) = rest.find('"') else {
            continue;
        };
        let href = decode_ddg(&rest[..href_end]);
        let title = rest[href_end..]
            .find('>')
            .map(|start| {
                let tail = &rest[href_end + start + 1..];
                let end = tail.find("</a>").unwrap_or(tail.len().min(200));
                strip_html(&tail[..end]).trim().to_string()
            })
            .unwrap_or_default();
        if !href.is_empty() && !title.is_empty() {
            results.push(json!({ "title": title, "url": href }));
        }
    }
    Ok(json!({ "query": query, "results": results }))
}

fn decode_ddg(raw: &str) -> String {
    let href = raw
        .strip_prefix("//")
        .map(|rest| format!("https://{rest}"))
        .unwrap_or_else(|| raw.to_string());
    if let Some(index) = href.find("uddg=") {
        let value = &href[index + 5..];
        let value = value.split('&').next().unwrap_or(value);
        return percent_decode(value);
    }
    href
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}


fn text_page(text: &str, offset: usize, page_chars: usize) -> Value {
    let total_chars = text.chars().count();
    let begin = offset.min(total_chars);
    let end = begin.saturating_add(page_chars.clamp(1_000,16_000)).min(total_chars);
    let page = text.chars().skip(begin).take(end - begin).collect::<String>();
    json!({
        "text": page,
        "offset": begin,
        "nextOffset": (end < total_chars).then_some(end),
        "hasMore": end < total_chars,
        "totalChars": total_chars,
    })
}

/// Render article text, not the site's script/style bundles or navigation chrome.
/// A subsequent http_fetch call can request any Unicode-character page of this
/// deterministic text; nothing needs to be duplicated in the next model prompt.
fn strip_html(html: &str) -> String {
    let lower=html.to_ascii_lowercase();
    let mut text=String::with_capacity(html.len().min(65_536));
    let mut cursor=0usize;
    while cursor < html.len() {
        let Some(rel)=html[cursor..].find('<') else {
            text.push_str(&html[cursor..]);
            break;
        };
        let open=cursor+rel;
        text.push_str(&html[cursor..open]);
        if lower[open..].starts_with("<!--") {
            cursor=lower[open+4..].find("-->")
                .map(|pos|open+4+pos+3).unwrap_or(html.len());
            continue;
        }
        let Some(close_rel)=html[open..].find('>') else {break;};
        let close=open+close_rel;
        let tag=lower[open+1..close].trim();
        let name=tag.trim_start_matches('/')
            .split(|ch:char|ch.is_ascii_whitespace()||ch=='/')
            .next().unwrap_or("");
        if !tag.starts_with('/') && matches!(name,
            "script"|"style"|"noscript"|"template"|"svg"|"head"|"nav"|"footer"|"aside")
        {
            let closer=format!("</{name}");
            cursor=lower[close+1..].find(&closer)
                .and_then(|offset|{
                    let at=close+1+offset;
                    lower[at..].find('>').map(|end|at+end+1)
                }).unwrap_or(html.len());
            text.push(' ');
            continue;
        }
        if matches!(name,"p"|"div"|"br"|"li"|"article"|"section"|"h1"|"h2"|"h3"|"h4"|"tr") {
            text.push(' ');
        }
        cursor=close+1;
    }
    text.replace("&amp;","&").replace("&lt;","<").replace("&gt;",">")
        .replace("&quot;","\"").replace("&#39;","'")
        .replace("&nbsp;"," ")
        .split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prompt_source_reader_rejects_stale_revisions_and_peer_persona(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-prompt-sources-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let own="PERSONA_SOURCE_".repeat(700);
        let other="PRIVATE_PEER_PERSONA_".repeat(700);
        state.store.put("session",&json!({"id":"s"})).unwrap();
        state.store.put("agent",&json!({"id":"a","name":"a","systemPrompt":own})).unwrap();
        state.store.put("agent",&json!({"id":"b","name":"b","systemPrompt":other})).unwrap();
        state.store.put("assistant_settings",&json!({"id":"global","enabled":true,
            "instructions":"OWNER POLICY"})).unwrap();
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"s",agent_id:Some("a"),tools_enabled:true,
            tools_execution_enabled:true};
        let first=execute(&ctx,"prompt_source_read",&json!({"source":"persona"}),"r").await.unwrap();
        let second=execute(&ctx,"prompt_source_read",
            &json!({"source":"persona","offset":6000}),"r").await.unwrap();
        assert_eq!(format!("{}{}",first["content"].as_str().unwrap(),
            second["content"].as_str().unwrap()),own);
        assert_eq!(first["hasMore"],true);
        assert_eq!(second["hasMore"],false);
        assert!(!json!(first).to_string().contains("PRIVATE_PEER_PERSONA_"));
        assert_eq!(execute(&ctx,"prompt_source_read",
            &json!({"source":"owner"}),"r").await.unwrap()["content"],"OWNER POLICY");
        assert!(execute(&ctx,"prompt_source_read",&json!({"source":"persona",
            "revision":"obsolete"}),"r").await.is_err());
        assert!(execute(&ctx,"prompt_source_read",&json!({"source":"other"}),"r").await.is_err());
        let no_agent=ToolContext{agent_id:None,..ctx};
        assert!(execute(&no_agent,"prompt_source_read",
            &json!({"source":"persona"}),"r").await.is_err());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn historical_evidence_reader_paginates_exact_results_and_enforces_agent_scope(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-history-reader-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:dir.join("data"),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"ours"}));
        let content=format!("{}끝","근거".repeat(3_100));
        state.store.write_message(&json!({"id":"tool-ours","session_id":"ours",
            "role":"tool","agent_id":"agent","tool_call_id":"call-1","content":content})).unwrap();
        state.store.write_message(&json!({"id":"tool-other","session_id":"other",
            "role":"tool","agent_id":"agent","tool_call_id":"call-2","content":"HIDDEN"})).unwrap();
        state.store.write_message(&json!({"id":"tool-peer","session_id":"ours",
            "role":"tool","agent_id":"other","tool_call_id":"call-3","content":"PEER"})).unwrap();
        state.store.write_message(&json!({"id":"assistant-private","session_id":"ours",
            "role":"assistant","agent_id":"agent","content":"REASONING"})).unwrap();
        state.store.write_message(&json!({"id":"owner-long","session_id":"ours",
            "role":"user","content":content})).unwrap();
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"ours",agent_id:Some("agent"),
            tools_enabled:true,tools_execution_enabled:true};
        let first=execute(&ctx,"history_evidence_read",&json!({"messageId":"tool-ours"}),"r").await.unwrap();
        let next=execute(&ctx,"history_evidence_read",
            &json!({"messageId":"tool-ours","offset":6000}),"r").await.unwrap();
        assert_eq!(format!("{}{}",first["content"].as_str().unwrap(),
            next["content"].as_str().unwrap()),content);
        assert_eq!(first["hasMore"],true);
        assert_eq!(next["hasMore"],false);
        let owner=execute(&ctx,"history_evidence_read",
            &json!({"messageId":"owner-long"}),"r").await.unwrap();
        assert_eq!(owner["role"],"user");
        assert_eq!(owner["nextOffset"],6000);
        assert_eq!(owner["hasMore"],true);
        for bad in ["tool-other","tool-peer","assistant-private","unknown"]{
            assert!(execute(&ctx,"history_evidence_read",
                &json!({"messageId":bad}),"r").await.is_err());
        }
        assert!(execute(&ctx,"history_evidence_read",
            &json!({"messageId":"tool-ours","offset":30000}),"r").await.is_err());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn room_source_reader_preserves_full_peer_history_and_private_scope(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-room-reader-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:dir.join("data"),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("room",&json!({"id":"r","agentIds":["member"]}));
        state.store.put("session",&json!({"id":"room:r","roomId":"r"}));
        let full=format!("{}끝", "기억".repeat(3_100));
        state.store.write_message(&json!({"id":"peer",
            "session_id":"room:r","role":"assistant","agent_id":"member",
            "content":full,"reasoning":"PRIVATE_CHAIN_OF_THOUGHT",
            "metadata":{"replyToMessageId":"owner"}})).unwrap();
        state.store.write_message(&json!({"id":"foreign",
            "session_id":"elsewhere","role":"assistant","agent_id":"member",
            "content":"FOREIGN_PRIVATE"})).unwrap();
        state.store.write_message(&json!({"id":"nonmember",
            "session_id":"room:r","role":"assistant","agent_id":"outsider",
            "content":"OUTSIDE_AGENT_PRIVATE"})).unwrap();
        state.store.write_message(&json!({"id":"owner",
            "session_id":"room:r","role":"user","content":"owner instructions"})).unwrap();
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"room:r",agent_id:Some("member"),
            tools_enabled:true,tools_execution_enabled:true};
        let first=execute(&ctx,"room_message_read",
            &json!({"messageId":"peer"}),"r").await.unwrap();
        assert_eq!(first["hasMore"],true);
        assert_eq!(first["nextOffset"],6_000);
        let next=execute(&ctx,"room_message_read",
            &json!({"messageId":"peer","offset":6_000}),"r").await.unwrap();
        let combined=format!("{}{}",
            first["content"].as_str().unwrap(),next["content"].as_str().unwrap());
        assert_eq!(combined,full);
        assert_eq!(next["hasMore"],false);
        assert!(!json!(first).to_string().contains("PRIVATE_CHAIN_OF_THOUGHT"));
        for bad in ["foreign","nonmember","owner","unknown"]{
            assert!(execute(&ctx,"room_message_read",
                &json!({"messageId":bad}),"r").await.is_err(),
                "reader must reject {bad}");
        }
        assert!(execute(&ctx,"room_message_read",
            &json!({"messageId":"peer","offset":20_000}),"r").await.is_err());
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn skill_read_requires_enabled_visible_skill_and_returns_original_body(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-skill-read-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:dir.join("data"),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("skill",&json!({"id":"approved","name":"Read-only playbook",
            "status":"active","enabled":true,"scope":"global",
            "instructions":"Long exact instructions that the prompt index omits"}));
        state.store.put("skill",&json!({"id":"draft","name":"Unapproved",
            "status":"draft","enabled":true,"scope":"global",
            "instructions":"Do not disclose this draft"}));
        state.store.put("skill",&json!({"id":"long","name":"Long approved",
            "status":"active","enabled":true,"scope":"global",
            "instructions":format!("{}끝", "가".repeat(12_000))}));
        state.store.put("skill",&json!({"id":"private","name":"Private",
            "status":"active","enabled":true,"scope":"agent","agentId":"someone-else",
            "instructions":"Private body"}));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"s",agent_id:Some("local"),
            tools_enabled:true,tools_execution_enabled:true};
        let read=execute(&ctx,"skill_read",
            &json!({"skillId":"approved"}),"r").await.unwrap();
        assert_eq!(read["instructions"],
            "Long exact instructions that the prompt index omits");
        assert_eq!(read["readOnly"],true);
        let first=execute(&ctx,"skill_read",&json!({"skillId":"long"}),"r").await.unwrap();
        assert_eq!(first["offset"],0);
        assert_eq!(first["nextOffset"],12_000);
        assert_eq!(first["hasMore"],true);
        assert_eq!(first["instructions"].as_str().unwrap().chars().count(),12_000);
        let second=execute(&ctx,"skill_read",
            &json!({"skillId":"long","offset":first["nextOffset"]}),"r").await.unwrap();
        assert_eq!(second["instructions"],"끝");
        assert_eq!(second["hasMore"],false);
        assert_eq!(second["totalChars"],12_001);
        assert!(execute(&ctx,"skill_read",
            &json!({"skillId":"long","offset":12_002}),"r").await.is_err());
        assert!(execute(&ctx,"skill_read",&json!({"skillId":"draft"}),"r").await.is_err());
        assert!(execute(&ctx,"skill_read",&json!({"skillId":"private"}),"r").await.is_err());
        assert!(execute(&ctx,"skill_read",&json!({"skillId":"missing"}),"r").await.is_err());
        assert!(ui_tool_allowed(&json!({"skillsEnabled":false}),"skill_read")==false);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parallel_fanin_bounds_context_but_retains_job_retrieval_keys(){
        let original=json!({"jobId":"child-1","status":"completed",
            "content":"z".repeat(100_000),
            "result":{"verified":true,"receipts":[{"large":"x".repeat(100_000)}]},
            "operations":(0..20).map(|_|json!({"output":"x".repeat(1000)})).collect::<Vec<_>>(),
        });
        let summary=summarize_parallel_result(&original);
        assert_eq!(summary["jobId"],"child-1");
        assert_eq!(summary["verified"],true);
        assert_eq!(summary["evidenceCount"],20);
        assert_eq!(summary["answer"].as_str().unwrap().chars().count(),1800);
        assert!(summary.to_string().len()<3000);
        assert!(summary.get("operations").is_none());
    }

    #[tokio::test]
    async fn parallel_agent_is_read_only_even_if_it_fabricates_tool_calls(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-fanout-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("source.txt"),"original").unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"child","parallelReadOnly":true}));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"child",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        assert!(execute(&ctx,"workspace_read",&json!({"path":"source.txt"}),"run").await.is_ok());
        assert!(execute(&ctx,"workspace_write",
            &json!({"path":"source.txt","content":"changed"}),"run").await.is_err());
        assert!(execute(&ctx,"delegate_task",
            &json!({"agentId":"other","prompt":"continue"}),"run").await.is_err());
        assert_eq!(std::fs::read_to_string(dir.join("source.txt")).unwrap(),"original");
        let invalid=delegate_parallel(&ctx,&json!({"tasks":[
            {"agentId":"missing-a","prompt":"inspect A"},
            {"agentId":"missing-b","prompt":"inspect B"}],"wait":false
        })).await.unwrap_err();
        assert!(invalid.contains("위임 대상"));
        assert!(state.store.all("job").is_empty());
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn workspace_paths_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("dominion-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(resolve_workspace(&dir, "sub").is_ok());
        assert!(resolve_workspace(&dir, "../etc/passwd").is_err());
        assert!(resolve_workspace(&dir, "/etc/passwd").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/", dir.join("outside")).unwrap();
            assert!(resolve_workspace(&dir, "outside/nonexistent.txt").is_err());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_frozen_version_write_stops_when_the_file_changed() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-frozen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let state = crate::state::AppState::new(
            config.clone(),
            store,
            Vault::open(&config.key_path()),
        );
        let path = dir.join("frozen.txt");
        std::fs::write(&path, "one").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"one"));
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let written = checked_write(&ctx, &path, "two", "run", false, Some(&expected), false)
            .await
            .unwrap();
        assert_eq!(written["written"], true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");

        // The same frozen version must not overwrite a later change.
        std::fs::write(&path, "external change").unwrap();
        let stale = checked_write(&ctx, &path, "three", "run", false, Some(&expected), false)
            .await
            .unwrap();
        assert_eq!(stale["staleVersion"], true, "{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external change");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn workspace_replace_edits_one_exact_block_without_full_content() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let path = dir.join("large.rs");
        std::fs::write(&path, "let old_value = 1;\nlet untouched = 2;\n").unwrap();
        let expected = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let result = replace_text(
            &ctx,
            &path,
            "let old_value = 1;",
            "let new_value = 3;",
            Some(&expected),
            "run",
        )
        .await
        .unwrap();
        assert_eq!(result["replaced"], true, "{result}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 2;\n"
        );
        let ambiguous = replace_text(&ctx, &path, "let", "const", None, "run")
            .await
            .unwrap();
        assert_eq!(ambiguous["occurrences"], 2, "{ambiguous}");
        assert!(result["bytes"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn range_replacement_edits_one_repeated_line_with_frozen_source_and_rejects_stale_edits() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-range-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let config=Config {port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let source="first();\nlet value = 0;\nlet value = 0;\nlast();\n";
        let path=dir.join("src/example.rs");
        std::fs::write(&path,source).unwrap();
        let digest=format!("{:x}",Sha256::digest(source.as_bytes()));
        let ctx=ToolContext {workspace:&dir,store:&state.store,state:&state,
            session_id:"s",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        state.store.put("session",&json!({"id":"s"})).unwrap();
        let no_op=execute(&ctx,"workspace_replace_range",
            &json!({"path":"src/example.rs","startLine":3,"endLine":3,
                "newText":"let value = 0;","expectedSha256":digest}),"run").await.unwrap();
        assert_eq!(no_op["unchanged"],true);
        // A no-op in ordinary user-directed editing is reported precisely;
        // legacy scoring heuristics must not reject it.
        assert!(no_op["error"].is_null());
        assert_eq!(no_op["written"],false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),source);
        let args=json!({"path":"src/example.rs","startLine":3,"endLine":3,
            "newText":"let value = 42;","expectedSha256":digest});
        let result=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap();
        assert_eq!(result["replaced"],true,"{result}");
        assert_eq!(result["replacedLines"],1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet value = 0;\nlet value = 42;\nlast();\n");
        let stale=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap();
        assert_eq!(stale["staleVersion"],true,"{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet value = 0;\nlet value = 42;\nlast();\n");
        let mut bad=args.clone();
        bad["endLine"]=json!(999);
        bad["expectedSha256"]=json!(format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap())));
        assert!(execute(&ctx,"workspace_replace_range",&bad,"run").await.is_err());
        drop(ctx); drop(state); std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn code_search_finds_indexed_symbols_and_paths() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-code-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        state
            .store
            .put(
                "code_index",
                &json!({"id":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),"sourceDir":dir.canonicalize().unwrap(),"generatedAt":"2026-11-09T00:00:00Z","files":1}),
            )
            .unwrap();
        state
            .store
            .put(
                "code_index_file",
                &json!({
                    "id":"idx-test:src/engine/workspace_worker.rs",
                    "indexId":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),
                    "path":"src/engine/workspace_worker.rs",
                    "symbols":[
                        {"name":"run_loop","kind":"fn","line":120},
                        {"name":"record_verified_result","kind":"fn","line":900}
                    ]
                }),
            )
            .unwrap();
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let result = search_code_index(&ctx, "run_loop", 10).unwrap();
        assert_eq!(result["matchCount"], 1, "{result}");
        assert_eq!(result["matches"][0]["path"], "src/engine/workspace_worker.rs");
        assert_eq!(result["matches"][0]["line"], 120);
        state.store.put("code_index",&json!({"id":"foreign","sourceDir":"/another/repository","generatedAt":"2099-01-01","files":1}));
        state.store.put("code_index_file",&json!({"id":"foreign-file","indexId":"foreign","path":"private.rs","symbols":[{"name":"run_loop","kind":"fn","line":999}]}));
        let multi=search_code_index(&ctx,"fn run_loop record_verified_result",10).unwrap();
        assert_eq!(multi["matchCount"],2,"{multi}");
        assert!(multi["matches"].as_array().unwrap().iter().all(|row|row["path"]!="private.rs"));
        let path_result = search_code_index(&ctx, "workspace_worker.rs", 10).unwrap();
        assert!(path_result["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["kind"] == "file"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delegation_depth_is_bounded_and_cycle_safe() {
        use crate::config::Config;
        use crate::db::Store;
        let dir = std::env::temp_dir().join(format!("dominion-depth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        for (id, parent) in [
            ("root", None),
            ("d1", Some("root")),
            ("d2", Some("d1")),
            ("d3", Some("d2")),
        ] {
            store.put(
                "session",
                &json!({"id":id,"parentSessionId":parent,"delegated":parent.is_some()}),
            );
        }
        assert_eq!(delegation_depth(&store, "root"), 0);
        assert_eq!(delegation_depth(&store, "d1"), 1);
        assert_eq!(delegation_depth(&store, "d3"), 3);
        store.put(
            "session",
            &json!({"id":"loop","parentSessionId":"d3","delegated":true}),
        );
        store.put(
            "session",
            &json!({"id":"d3","parentSessionId":"loop","delegated":true}),
        );
        assert!(delegation_depth(&store, "loop") <= 4);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn task_results_are_scoped_to_the_owning_or_parent_session() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-task-acl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = crate::state::AppState::new(config, store, vault);
        state.store.put(
            "job",
            &json!({
                "id":"j1","kind":"prompt","status":"completed",
                "input":{"sessionId":"child","parentSessionId":"parent"},
                "result":{"answer":"ok"}
            }),
        );
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "parent",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&ctx, &json!({"jobId":"j1"}), true).is_ok());
        let child = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "child",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&child, &json!({"jobId":"j1"}), true).is_ok());
        let stranger = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "stranger",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&stranger, &json!({"jobId":"j1"}), true).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn public_http_fetch_blocks_internal_and_redirect_destinations() {
        for url in ["http://127.0.0.1:4317","http://10.1.2.3","http://172.16.1.1",
            "http://192.168.1.1","http://169.254.169.254/latest/meta-data/",
            "http://100.100.100.100","http://[::1]/","http://[::ffff:127.0.0.1]/",
            "http://router.local/","http://metadata.google.internal/",
            "file:///etc/passwd","http://localhost:80/"] {
            assert!(public_http_url(url).is_err(),"private target passed: {url}");
        }
        assert!(public_http_url("https://example.com/").is_ok());
        assert!(public_http_url("https://1.1.1.1/").is_ok());
        assert!(public_http_url("https://[2606:4700:4700::1111]/").is_ok());
        assert!(public_http_url("https://127.0.0.1@public.example/").is_err());
        let origin=public_http_url("https://example.com/").unwrap();
        let redirected=origin.join("http://127.0.0.1:4317/private").unwrap();
        assert!(public_http_url(redirected.as_str()).is_err(),
            "redirect must not bypass local address protection");
    }


    #[test]
    fn html_extraction_excludes_site_chrome_and_preserves_article() {
        let html=r#"<html><head><title>Noise</title><style>.huge{}</style></head>
            <body><nav>Sign in and unrelated links</nav>
            <article><h1>Research result</h1><p>Evidence &amp; explanation.</p></article>
            <script>window.__NEXT_DATA__ = "very large repeated blob";</script>
            <footer>Legal menu</footer></body></html>"#;
        let clean=strip_html(html);
        assert!(clean.contains("Research result Evidence & explanation."),"{clean}");
        for unused in ["Noise","huge","Sign in","NEXT_DATA","Legal menu"] {
            assert!(!clean.contains(unused),"{clean}");
        }
    }

    #[test]
    fn fetched_text_pages_are_unicode_exact_and_recoverable() {
        let original="가나다라마바사ABC123".repeat(1100);
        let mut offset=0usize;
        let mut reconstructed=String::new();
        loop {
            let page=text_page(&original,offset,1000);
            assert!(page["text"].as_str().unwrap().chars().count()<=1000);
            reconstructed.push_str(page["text"].as_str().unwrap());
            let Some(next)=page["nextOffset"].as_u64() else {break};
            assert!(next as usize>offset);
            offset=next as usize;
        }
        assert_eq!(reconstructed,original);
    }

    #[test]
    fn http_response_capture_stops_at_byte_limit() {
        let mut bytes=Vec::new();
        assert!(!append_http_chunk(&mut bytes,&vec![b'a';HTTP_FETCH_LIMIT-4]));
        assert!(append_http_chunk(&mut bytes,b"abcdefghij"));
        assert_eq!(bytes.len(),HTTP_FETCH_LIMIT);
        assert_eq!(&bytes[HTTP_FETCH_LIMIT-4..],b"abcd");
    }

    #[tokio::test]
    async fn native_multi_file_patch_uses_the_real_tool_dispatcher_and_invalidates_code_index(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-native-patch-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("first.rs"),"fn first() {}\n").unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"s"}));
        let index_id=crate::engine::code_index::index_id(&dir.canonicalize().unwrap());
        state.store.put("code_index",&json!({"id":index_id,"sourceDir":dir,"files":1}));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"s",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let input="*** Begin Patch\n*** Update File: first.rs\n@@\n-fn first() {}\n+fn first() { println!(\"done\"); }\n*** Add File: second.rs\n+pub fn second() {}\n*** End Patch";
        let result=execute(&ctx,"workspace_apply_patch",&json!({"input":input}),"test-run")
            .await.unwrap();
        assert_eq!(result["status"],"completed","{result}");
        assert_eq!(result["applied"],2);
        assert_eq!(state.store.get("code_index",&index_id).unwrap()["dirty"],true);
        assert!(std::fs::read_to_string(dir.join("first.rs")).unwrap().contains("done"));
        assert!(std::fs::read_to_string(dir.join("second.rs")).unwrap().contains("second"));
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tool_definitions_cover_the_engine() {
        let names = definitions()
            .into_iter()
            .filter_map(|definition| definition["function"]["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        for expected in [
            "workspace_read",
            "workspace_write",
            "shell_exec",
            "memory_store",
            "memory_search",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "{expected} missing"
            );
        }
        assert!(requires_approval("shell_exec"));
        assert!(requires_approval("external_harness_run"));
        assert!(!parallel_read_only_tool("external_harness_run"));
        assert!(!requires_approval("workspace_read"));
        assert!(names.iter().any(|name|name=="delegate_parallel"));
        assert!(names.iter().any(|name|name=="workspace_apply_patch"));
        assert!(requires_approval("workspace_apply_patch"));
        assert!(!parallel_read_only_tool("workspace_apply_patch"));
        assert!(parallel_read_only_tool("workspace_read_range"));
        assert!(!parallel_read_only_tool("shell_exec"));
        assert!(!parallel_read_only_tool("workspace_write"));
        assert!(!parallel_read_only_tool("mcp__external__tool"));
        assert!(!parallel_read_only_tool("delegate_task"));
        let policy=json!({"mcpEnabled":false,"webEnabled":false,"subagentsEnabled":false});
        assert!(ui_tool_allowed(&policy,"workspace_read"));
        assert!(!ui_tool_allowed(&policy,"mcp__service__tool"));
        assert!(!ui_tool_allowed(&policy,"web_search"));
        assert!(!ui_tool_allowed(&policy,"delegate_parallel"));
        assert!(!ui_tool_allowed(&policy,"external_harness_run"));
        assert!(ui_tool_allowed(&json!({"subagentsEnabled":true}),"external_harness_run"));
        assert!(!ui_tool_allowed(&json!({"skillsEnabled":false}),"skill_propose"));
        assert!(ui_tool_allowed(&json!({"skillsEnabled":true}),"workspace_apply_patch"));
    }
    #[cfg(unix)]
    async fn wait_for_marker(path: &Path) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !path.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "test command did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[cfg(unix)]
    fn delayed_child_command(dir: &Path) -> tokio::process::Command {
        // The parent waits while an ordinary child will mutate the temp
        // workspace later. No daemon, user process or external service is used.
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args([
                "-c",
                "(sleep 0.6; printf leaked > escaped.txt) & printf ready > ready.txt; wait",
            ])
            .current_dir(dir);
        command
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.clone();
        let task = tokio::spawn(async move {
            let mut command = delayed_child_command(&cwd);
            run_command(&mut command, Duration::from_secs(10)).await
        });
        wait_for_marker(&dir.join("ready.txt")).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after Stop"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-timeout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut command = delayed_child_command(&dir);
        let result = run_command(&mut command, Duration::from_millis(150)).await;
        assert!(result.unwrap_err().contains("timed out"));
        assert!(dir.join("ready.txt").exists());
        tokio::time::sleep(Duration::from_millis(650)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after timeout"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn native_runner_handles_stdin_and_bounds_stalled_input(){
        let mut reader=tokio::process::Command::new("/bin/cat");
        let (result,stdout_clipped,stderr_clipped)=run_command_with_input(
            &mut reader,Duration::from_secs(3),b"task via stdin",
        ).await.unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout,b"task via stdin");
        assert!(!stdout_clipped && !stderr_clipped);
        // The child refuses to consume stdin. A large write must not hang
        // outside the native process supervisor's deadline.
        let mut blocked=tokio::process::Command::new("/bin/sh");
        blocked.args(["-c","sleep 2"]);
        let huge=vec![b'x';2*1024*1024];
        let started=std::time::Instant::now();
        let err=run_command_with_input(
            &mut blocked,Duration::from_millis(150),&huge,
        ).await.unwrap_err();
        assert!(err.contains("timed out"),"{err}");
        assert!(started.elapsed()<Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tool_command_preserves_output_exit_status_and_bounded_capture() {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "printf normal; printf warning >&2; exit 7"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        let payload = output_payload(&output);
        assert_eq!(payload["exitCode"], 7);
        assert_eq!(payload["stdout"], "normal");
        assert_eq!(payload["stderr"], "warning");
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "head -c 250000 /dev/zero"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.len() <= 4 * 20_001);
        assert!(output_payload(&output)["stdout"]
            .as_str()
            .unwrap()
            .contains("truncated"));
    }
}
