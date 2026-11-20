#[path = "../addons/integrations/native_tools.rs"]
mod native_tools;
#[path = "../addons/integrations/native_github.rs"]
mod native_github;
#[path = "../addons/integrations/native_web.rs"]
mod native_web;
use native_github::{github_api,github_file_read,github_file_update};
#[cfg(test)]
use native_github::github_file_url;
pub(crate) use native_web::http_fetch;
use native_web::web_search;
#[cfg(test)]
use native_web::{public_http_url,parse_ddg_search_results,parse_bing_rss_search_results};
#[path = "../addons/integrations/tool_github_repositories.rs"]
mod github_repositories;
#[path = "../addons/integrations/tool_github_replace.rs"]
mod github_replace;
#[path = "../addons/integrations/tool_delegation.rs"]
mod delegation;
use delegation::{delegate, task_payload};
#[cfg(test)]
use delegation::delegation_depth;
pub(crate) use delegation::{profile_tool_allowed, profile_tool_visible};
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

    let mut definitions = vec![
        tool("workspace_list", "List files and folders in the workspace", object(json!({"path": text("Relative folder path")}), vec![])),
        tool("workspace_read", "Read a UTF-8 file from the workspace", object(json!({"path": text("Relative file path")}), vec!["path"])),
        tool("workspace_read_range", "Read only a line range from a workspace file. Use the line numbers from the repository index instead of reading a large file end to end.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1, "description":"Inclusive end line; at most 400 lines are returned"})}), vec!["path","startLine"])),
        tool("workspace_git_status","Inspect Git changes in the current workspace without running a shell",object(json!({}),vec![])),
        tool("workspace_git_diff","Inspect the current workspace patch without shell hooks or external diff helpers",object(json!({}),vec![])),
        tool("code_search", "Search the persistent repository index for file paths, symbols and line numbers. Use this before reading a large file.", object(json!({"query": text("Function, type, path or keyword fragment"), "limit": json!({"type":"integer","minimum":1,"maximum":50})}), vec!["query"])),
        tool("workspace_replace_range", "Replace a previously read inclusive line range with newText only. Use startLine/endLine and sourceSha256 returned by workspace_read_range; old text is not required.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1,"description":"Inclusive line number; at most 120 lines"}), "newText": text("Replacement lines (without old source text)"), "expectedSha256": text("Exact sourceSha256 from the successful prior read")}), vec!["path","startLine","endLine","newText","expectedSha256"])),
        tool("workspace_replace", "Replace one exact block in a workspace file without resending the whole file (approval required). oldText must occur exactly once.", object(json!({"path": text("Relative file path"), "oldText": text("Exact existing text to replace; must occur exactly once"), "newText": text("Replacement text"), "expectedSha256": text("Optional SHA-256 observed before editing")}), vec!["path","oldText","newText"])),
        tool("workspace_write", "Write a UTF-8 file inside the workspace (approval required)", object(json!({"path": text("Relative file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("file_list", "List any folder on this computer using an absolute path", object(json!({"path": text("Absolute folder path")}), vec!["path"])),
        tool("file_read", "Read any UTF-8 text file using an absolute path", object(json!({"path": text("Absolute file path")}), vec!["path"])),
        tool("file_write", "Create or overwrite any file using an absolute path (approval required)", object(json!({"path": text("Absolute file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("shell_exec", "Run a shell command on this computer and return stdout/stderr (approval required)", object(json!({"command": text("Shell command line"), "cwd": text("Optional absolute working directory")}), vec!["command"])),
        tool("terminal_exec", "Run an executable with an argument array without a shell (approval required)", object(json!({"command": text("Executable"), "args": json!({"type": "array", "items": {"type": "string"}})}), vec!["command"])),
        tool("web_search", "Search the public web for current information", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("http_fetch", "Fetch a URL and return its readable text content", object(json!({"url": text("HTTP or HTTPS URL")}), vec!["url"])),
        tool("memory_store", "Save a grounded memory for this agent. Quote the user's exact words; invented facts are rejected. Shared memories are managed by the user.", object(json!({"text": text("Fact to remember, maximum 1600 characters"), "evidenceQuote": text("Exact quote from the current user message supporting the fact"), "sourceMessageId": text("Optional earlier source message ID"), "type": json!({"type":"string","enum":["fact","preference","decision","procedure","lesson"]}), "key": text("Optional stable fact key; a new value replaces the previous value for this key"), "scope": json!({"type":"string","enum":["agent"]})}), vec!["text", "evidenceQuote"])),
        tool("memory_search", "Find saved knowledge relevant to a topic", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("skill_view", "Load an enabled skill's original instructions on demand by exact skill ID or name; page large skills by startLine", object(json!({"id": text("Enabled skill ID or exact name from the skill index"),"startLine":json!({"type":"integer","minimum":1}),"maxLines":json!({"type":"integer","minimum":1,"maximum":160})}), vec!["id"])),
        tool("skill_propose", "Draft a reusable skill for user review", object(json!({"name": text("Skill name"), "instructions": text("Reusable instructions")}), vec!["name", "instructions"])),
        tool("delegate_task", "Delegate a self-contained task to another agent and optionally wait for its result", object(json!({
            "agentId": text("Target agent id"),
            "prompt": text("Task instructions"),
            "wait": json!({"type":"boolean","description":"Wait for completion (default true)"}),
            "timeoutSeconds": json!({"type":"number","description":"5-300 seconds"}),
            "budget": json!({"type":"integer","minimum":1,"description":"Child allowance in the parent budget unit (tokens or USD micros)"}),
            "readOnly":json!({"type":"boolean","description":"Constrain the worker to observational tools"}),
            "isolatedWorktree":json!({"type":"boolean","description":"Use an independent Git worktree and enforced shell sandbox; integrate explicitly with task_integrate"}),
            "profile": json!({"type":"string","enum":["general","inspection","implementation","verification"],
                "description":"2026 role-scoped child task: inspection is read-only; implementation and verification require isolatedWorktree; verification admits only a sandboxed Cargo test runner and observational tools"})
        }), vec!["agentId", "prompt"])),
        tool("task_integrate","Integrate a completed isolated child patch into this parent workspace (approval required)",object(json!({"jobId":text("Completed child job id")}),vec!["jobId"])),
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
        tool("github_repositories","Discover connected-account repositories before selecting a file and committing edits",object(json!({
            "query":text("Optional owner or repository name fragment, max 100 characters"),
            "page":json!({"type":"integer","minimum":1,"maximum":10}),
            "writableOnly":json!({"type":"boolean","description":"Only show repositories with push permission"})
        }),vec![])),
        tool("github_api", "Call the GitHub REST API as the connected account. Write methods require approval (approval required)", object(json!({"method": json!({"type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"]}), "path": text("GitHub API path beginning with /"), "body": json!({"type": "object"})}), vec!["method", "path"])),
        tool("github_file_read","Read a GitHub repository file using the connected account; returns its exact content SHA for safe edits",object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "path":text("Repository-relative file path"),
            "branch":text("Branch or commit ref, default branch when omitted")
        }),vec!["owner","repo","path"])),
        tool("github_file_replace","Replace exactly one GitHub text block using a previously observed SHA. Commits and remotely verifies the change without resending the whole file (approval required)",object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "path":text("Repository-relative file path"),"branch":text("Explicit branch name"),
            "expectedSha":text("40-character SHA from github_file_read"),
            "oldText":text("Exact non-empty text that appears once in the file"),
            "newText":text("Replacement text, can be empty"),"message":text("Git commit message")
        }),vec!["owner","repo","path","branch","expectedSha","oldText","newText","message"])),
        tool("github_file_update","Update one GitHub repository file using an observed SHA, create a commit and re-read the remote file (approval required)",object(json!({
            "owner":text("Repository owner"),"repo":text("Repository name"),
            "path":text("Repository-relative file path"),
            "branch":text("Target branch name"),"expectedSha":text("SHA from github_file_read, required"),
            "content":text("Complete new UTF-8 file content"),
            "message":text("Git commit message")
        }),vec!["owner","repo","path","branch","expectedSha","content","message"])),
    ];
    definitions.extend(crate::engine::capability::definitions());
    definitions
}

#[cfg(test)]
mod lazy_skill_tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    #[tokio::test]
    async fn enabled_skill_reads_by_id_without_leaking_disabled_or_draft_skills() {
        let dir=std::env::temp_dir().join(format!("dominion-skill-view-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        let state=crate::state::AppState::new(config,store,vault);
        state.store.put("skill",&json!({"id":"enabled-skill","scope":"global",
            "name":"Current research","description":"Short catalog description",
            "instructions":"Heading\nSECRET-SKILL-INSTRUCTION\nFinish",
            "enabled":true,"status":"active"}));
        state.store.put("skill",&json!({"id":"draft-skill","scope":"global",
            "name":"Draft","instructions":"Do not load","enabled":true,"status":"draft"}));
        let ctx=ToolContext{
            workspace:&dir,store:&state.store,state:&state,
            session_id:"test-session",agent_id:None,
            tools_enabled:true,tools_execution_enabled:true,
        };
        let result=execute(&ctx,"skill_view",&json!({"id":"enabled-skill","startLine":2,"maxLines":1}),"run").await.unwrap();
        assert_eq!(result["content"],"SECRET-SKILL-INSTRUCTION");
        assert_eq!(result["totalLines"],3);
        assert_eq!(result["hasMore"],true);
        assert!(execute(&ctx,"skill_view",&json!({"id":"draft-skill"}),"run").await.is_err());
        assert!(execute(&ctx,"skill_view",&json!({"id":"nonexistent"}),"run").await.is_err());
        let _=std::fs::remove_dir_all(dir);
    }
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

pub fn requires_approval(name: &str) -> bool {
    matches!(
        name,
        "workspace_write"
            | "workspace_replace"
            | "workspace_replace_range"
            | "file_write"
            | "shell_exec"
            | "terminal_exec"
            | "task_integrate"
            | "github_api"
            | "github_file_update"
            | "github_file_replace"
            | "browser_control"
    )
}

pub fn observational(name:&str)->bool{
    matches!(name,"dominion_tool_search" | "dominion_tool_call" | "workspace_git_status"|"workspace_git_diff"|"workspace_list"|"workspace_read"|"workspace_read_range"|"code_search"|"file_list"|"file_read"|"memory_search"|"skill_view"|"web_search"|"http_fetch"|"task_status"|"task_result"|"account_status"|"github_file_read"|"github_repositories")
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &Value,
    run_id: &str,
) -> Result<Value, String> {
    let session = ctx.store.get("session",ctx.session_id).unwrap_or(Value::Null);
    if !profile_tool_allowed(&session,name,args) {
        return Err("위임 역할에 허용되지 않는 도구 또는 명령입니다.".into());
    }
    // Tool schemas can be cached across a policy change. Revalidate at the
    // actual execution boundary, not only when the model saw its catalog.
    if !crate::engine::extensions::tool_allowed(
        ctx.state,ctx.session_id,ctx.agent_id,name) {
        return Err("이 세션에서 해당 부가기능이 비활성화되어 있습니다.".into());
    }
    if session["readOnly"]==true && !observational(name) {
        return Err("읽기 전용 위임 작업에는 이 도구를 실행할 권한이 없습니다.".into());
    }
    let parent_isolated=ctx.store.get("session",ctx.session_id).is_some_and(|s|s["delegationWorkspace"].is_object());
    if parent_isolated&&(name.starts_with("mcp__")||matches!(name,"browser_control"|"github_api"|"github_repositories"|"github_file_read"|"github_file_update"|"github_file_replace")){
        return Err("격리 위임 작업에서는 외부 변경 도구를 실행할 수 없습니다.".into());
    }
    if parent_isolated&&name=="delegate_task"&&args["isolatedWorktree"]!=true{
        return Err("격리 작업의 하위 위임도 isolatedWorktree가 필요합니다.".into());
    }
    if parent_isolated
        && matches!(name,"file_write"|"file_read"|"file_list") {
        let target=canonical_target(Path::new(&required_arg(args,"path")?)).map_err(|e|e.to_string())?;
        if !target.starts_with(ctx.workspace.canonicalize().map_err(|e|e.to_string())?){return Err("격리 위임에서는 외부 파일에 접근할 수 없습니다.".into());}
    }
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
            // Honor the bounded read-range contract. Silently cutting a requested
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
            )
            .await
        }
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
            )
            .await
        }
        "shell_exec" => {
            let command = required_arg(args, "command")?;
            let cwd = match string_arg(args, "cwd") {
                Some(dir) => PathBuf::from(dir),
                None => ctx.workspace.to_path_buf(),
            };
            if crate::engine::sandbox::required(ctx.state,ctx.session_id){
                if !cwd.canonicalize().map_err(|e|e.to_string())?.starts_with(ctx.workspace.canonicalize().map_err(|e|e.to_string())?){return Err("격리 셸의 작업 폴더는 위임 작업공간 내부여야 합니다.".into());}
                let mut child=crate::engine::sandbox::script(ctx.workspace,&command)?;
                child.current_dir(&cwd);
                return run_command(&mut child,Duration::from_secs(120)).await.map(|output|output_payload(&output));
            }
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
            if crate::engine::sandbox::required(ctx.state,ctx.session_id){
                let mut child=crate::engine::sandbox::command(ctx.workspace,&command,&argv)?;
                return run_command(&mut child,Duration::from_secs(120)).await.map(|output|output_payload(&output));
            }
            executable(&command, &argv, ctx.workspace).await
        }
        "dominion_tool_search" => crate::engine::capability::search_for_agent(
            ctx.state,ctx.session_id,ctx.agent_id,args).await,
        "dominion_tool_call" => Err("발견한 도구는 승인·효과 추적 게이트를 통해서만 호출할 수 있습니다.".into()),
        other => native_tools::execute(ctx,other,args).await,
    }
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
pub(crate) fn resolve_workspace(workspace: &Path, relative: &str) -> Result<PathBuf, String> {
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
    // Search the current workspace's persisted code index.
    let meta=match crate::engine::code_index::get(ctx.state,ctx.workspace){
        Some(meta)=>meta,
        None=>crate::engine::code_index::refresh(ctx.state,ctx.workspace)?,
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
        // Deleted/renamed runtime files may remain in an older SQLite index.
        // A cached symbol is navigation data, not evidence its source exists.
        if !resolve_workspace(ctx.workspace,path).is_ok_and(|path|path.is_file()){continue;}
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
    let mut result = checked_write(ctx, path, &updated, run_id, true, expected_sha256).await?;
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
    let mut result = checked_write(ctx, path, &updated, run_id, true, Some(expected_sha256)).await?;
    if result["written"] == true {
        result["replaced"] = json!(true);
        result["replacedLines"] = json!(end-start+1);
        result["postSha256"] = json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    }
    Ok(result)
}

pub(crate) fn mutation_lock(state:&crate::state::AppState,path:&Path)->std::sync::Arc<tokio::sync::Mutex<()>>{
    let canonical=canonical_target(path).unwrap_or_else(|_|path.to_owned());
    let mut locks=state.file_mutations.lock().unwrap();
    locks.retain(|_,lock|lock.strong_count()!=0);
    if let Some(lock)=locks.get(&canonical).and_then(std::sync::Weak::upgrade){return lock;}
    let lock=std::sync::Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(canonical,std::sync::Arc::downgrade(&lock));lock
}

async fn checked_write(
    ctx: &ToolContext<'_>,
    path: &Path,
    content: &str,
    run_id: &str,
    workspace_write: bool,
    expected_sha256: Option<&str>,
) -> Result<Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let policy = preflight_policy(ctx, path, run_id);
    let canonical = canonical_target(path);
    let lock_path = match &canonical {
        Ok(path) => path.as_path(),
        Err(error) if policy.is_some() => return Err(error.to_string()),
        Err(_) => path,
    };
    let lock=mutation_lock(ctx.state,lock_path);
    let _guard = lock.lock().await;
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
                "preflightPath": target}),
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
        "preflightRevision": policy["revision"], "preflightPath": target, "observedSha256": observed}),
    )
}

async fn write_text(path: &Path, content: &str) -> Result<Value, String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "bytes": content.len(),
        "written": true
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

async fn read_command_output(
    mut stream: impl tokio::io::AsyncRead + Unpin,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    // Keep enough UTF-8 bytes for 20,001 characters so output_payload can
    // preserve its existing 20,000-character truncation indicator. Continue
    // draining after this cap so a chatty child cannot fill a pipe or memory.
    const MAX_RETAINED_BYTES: usize = 4 * 20_001;
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(kept);
        }
        let retain = count.min(MAX_RETAINED_BYTES.saturating_sub(kept.len()));
        kept.extend_from_slice(&buffer[..retain]);
    }
}

pub(crate) async fn run_command(
    command: &mut tokio::process::Command,
    limit: Duration,
) -> Result<std::process::Output, String> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let mut group = ToolProcessGroup::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or("Command stdout is not available")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Command stderr is not available")?;
    let outcome = tokio::time::timeout(limit, async {
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_command_output(stdout),
            read_command_output(stderr)
        )?;
        Ok::<_, std::io::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    match outcome {
        Ok(Ok(output)) => {
            #[cfg(unix)]
            group.disarm();
            Ok(output)
        }
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(if limit.subsec_nanos() == 0 {
            format!("Command timed out after {}s", limit.as_secs())
        } else {
            format!("Command timed out after {}ms", limit.as_millis())
        }),
    }
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_search_parsers_produce_real_source_urls_not_empty_success() {
        let html=r#"<html><a class="result__a" href="https://example.org/source-one">Official docs</a>
            <a class="result__a" href="https://example.org/source-one">Duplicate</a></html>"#;
        let parsed=parse_ddg_search_results(html);
        assert_eq!(parsed.len(),1);
        assert_eq!(parsed[0]["title"],"Official docs");
        assert_eq!(parsed[0]["url"],"https://example.org/source-one");
        assert!(parse_ddg_search_results("<html>blocked</html>").is_empty());
        let rss="<rss><item><title>Another official source</title><link>https://example.net/docs</link></item></rss>";
        let parsed=parse_bing_rss_search_results(rss);
        assert_eq!(parsed.len(),1);
        assert_eq!(parsed[0]["url"],"https://example.net/docs");
        assert!(parse_bing_rss_search_results("<rss></rss>").is_empty());
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
        let written = checked_write(&ctx, &path, "two", "run", false, Some(&expected))
            .await
            .unwrap();
        assert_eq!(written["written"], true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");

        // The same frozen version must not overwrite a later change.
        std::fs::write(&path, "external change").unwrap();
        let stale = checked_write(&ctx, &path, "three", "run", false, Some(&expected))
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
        drop(ctx);
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn code_search_finds_indexed_symbols_and_paths() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-code-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        std::fs::write(dir.join("src/engine/agent_kernel.rs"),"fn run_loop() {}\nfn record_agent_lesson() {}\n").unwrap();
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
                    "id":"idx-test:src/engine/agent_kernel.rs",
                    "indexId":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),
                    "path":"src/engine/agent_kernel.rs",
                    "symbols":[
                        {"name":"run_loop","kind":"fn","line":120},
                        {"name":"record_agent_lesson","kind":"fn","line":900}
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
        assert_eq!(result["matches"][0]["path"], "src/engine/agent_kernel.rs");
        assert_eq!(result["matches"][0]["line"], 120);
        state.store.put("code_index",&json!({"id":"foreign","sourceDir":"/another/repository","generatedAt":"2099-01-01","files":1}));
        state.store.put("code_index_file",&json!({"id":"foreign-file","indexId":"foreign","path":"private.rs","symbols":[{"name":"run_loop","kind":"fn","line":999}]}));
        let multi=search_code_index(&ctx,"fn run_loop record_agent_lesson",10).unwrap();
        assert_eq!(multi["matchCount"],2,"{multi}");
        assert!(multi["matches"].as_array().unwrap().iter().all(|row|row["path"]!="private.rs"));
        let path_result = search_code_index(&ctx, "agent_kernel.rs", 10).unwrap();
        assert!(path_result["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["kind"] == "file"));
        std::fs::remove_file(dir.join("src/engine/agent_kernel.rs")).unwrap();
        let removed=search_code_index(&ctx,"run_loop",10).unwrap();
        assert_eq!(removed["matchCount"],0,"deleted runtime must not reappear through cached symbols: {removed}");
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
    fn http_response_capture_stops_at_byte_limit() {
        let mut bytes=Vec::new();
        assert!(!append_http_chunk(&mut bytes,&vec![b'a';HTTP_FETCH_LIMIT-4]));
        assert!(append_http_chunk(&mut bytes,b"abcdefghij"));
        assert_eq!(bytes.len(),HTTP_FETCH_LIMIT);
        assert_eq!(&bytes[HTTP_FETCH_LIMIT-4..],b"abcd");
    }

    #[test]
    fn github_file_tool_paths_and_approval_are_explicit(){
        let args=json!({"owner":"hushmind3","repo":"dominion-rust",
            "path":"src/engine/tools.rs","branch":"main"});
        let url=github_file_url(&args).unwrap();
        assert_eq!(url.as_str(),"https://api.github.com/repos/hushmind3/dominion-rust/contents/src/engine/tools.rs?ref=main");
        assert!(github_file_url(&json!({"owner":"a","repo":"b","path":"../secret"})).is_err());
        assert!(github_file_url(&json!({"owner":"a","repo":"b","path":"/etc/passwd"})).is_err());
        assert!(requires_approval("github_file_update"));
        assert!(requires_approval("github_file_replace"));
        assert!(!requires_approval("github_file_read"));
        assert!(!requires_approval("github_repositories"));
        assert!(observational("github_file_read"));
        assert!(observational("github_repositories"));
        assert!(!observational("github_file_replace"));
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
        assert!(!requires_approval("workspace_read"));
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
