// Moved networking implementation stays accessible to historical crate tests.
pub(crate) use crate::integrations::network::http_fetch;
#[cfg(test)]
use crate::integrations::network::{public_http_url,append_http_chunk,HTTP_FETCH_LIMIT};
#[cfg(test)] use crate::features::platform::code_search::search_code_index;
use crate::db::Store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const MAX_EDIT_SOURCE_BYTES: usize = 1_500_000;

async fn read_source_text(path: &Path) -> Result<String, String> {
    use tokio::io::AsyncReadExt;

    let file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !file
        .metadata()
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?
        .is_file()
    {
        return Err("Target is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_EDIT_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > MAX_EDIT_SOURCE_BYTES {
        return Err("File is larger than 1.5 MB".into());
    }
    String::from_utf8(bytes).map_err(|error| format!("{}: {error}", path.display()))
}

async fn sha256_file(path: &Path) -> Result<String, String> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

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
        tool("workspace_list", "List files and folders in the workspace. Large folders return at most 200 entries; pass nextCursor as cursor to read the next page.", object(json!({"path": text("Relative folder path"), "cursor": text("nextCursor returned by the prior page"), "limit": json!({"type":"integer","minimum":1,"maximum":500})}), vec![])),
        tool("workspace_read", "Read a UTF-8 file from the workspace", object(json!({"path": text("Relative file path")}), vec!["path"])),
        tool("workspace_read_range", "Read only a line range from a workspace file. Use the line numbers from the repository index instead of reading a large file end to end.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1, "description":"Inclusive end line; at most 400 lines are returned"})}), vec!["path","startLine"])),
        tool("workspace_replace_range", "Replace a previously read inclusive line range with newText only. Use startLine/endLine and sourceSha256 returned by workspace_read_range; old text is not required.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1,"description":"Inclusive line number; at most 120 lines"}), "newText": text("Replacement lines (without old source text)"), "expectedSha256": text("Exact sourceSha256 from the successful prior read")}), vec!["path","startLine","endLine","newText","expectedSha256"])),
        tool("workspace_replace", "Replace one exact block in a workspace file without resending the whole file (approval required). oldText must occur exactly once.", object(json!({"path": text("Relative file path"), "oldText": text("Exact existing text to replace; must occur exactly once"), "newText": text("Replacement text"), "expectedSha256": text("Optional SHA-256 observed before editing")}), vec!["path","oldText","newText"])),
        tool("workspace_write", "Write a UTF-8 file inside the workspace (approval required)", object(json!({"path": text("Relative file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("file_list", "List any folder on this computer using an absolute path. Large folders return at most 200 entries; pass nextCursor as cursor to read the next page.", object(json!({"path": text("Absolute folder path"), "cursor": text("nextCursor returned by the prior page"), "limit": json!({"type":"integer","minimum":1,"maximum":500})}), vec!["path"])),
        tool("file_read", "Read any UTF-8 text file using an absolute path", object(json!({"path": text("Absolute file path")}), vec!["path"])),
        tool("file_write", "Create or overwrite any file using an absolute path (approval required)", object(json!({"path": text("Absolute file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("shell_exec", "Run a shell command on this computer and return stdout/stderr (approval required)", object(json!({"command": text("Shell command line"), "cwd": text("Optional absolute working directory")}), vec!["command"])),
        tool("terminal_exec", "Run an executable with an argument array without a shell (approval required)", object(json!({"command": text("Executable"), "args": json!({"type": "array", "items": {"type": "string"}})}), vec!["command"])),
        tool("delegate_task", "Delegate a self-contained task to another agent and optionally wait for its result", object(json!({
            "agentId": text("Target agent id"),
            "prompt": text("Task instructions"),
            "wait": json!({"type":"boolean","description":"Wait for completion (default true)"}),
            "timeoutSeconds": json!({"type":"number","description":"5-300 seconds"})
        }), vec!["agentId", "prompt"])),
        tool("task_status", "Check a delegated task's status", object(json!({"jobId": text("Job id")}), vec!["jobId"])),
        tool("task_result", "Read a delegated task's result, artifacts and final answer", object(json!({"jobId": text("Job id")}), vec!["jobId"])),


    ];
    definitions.extend(crate::features::registry::definitions());
    definitions.extend(crate::integrations::registry::definitions());
    definitions
}

/// Show connected service tools only when the owner granted that account.
pub fn definitions_for(state:&crate::state::AppState)->Vec<Value>{
    crate::integrations::registry::filter_available(state, definitions())
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

    ) || crate::integrations::registry::requires_approval(name)
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &Value,
    run_id: &str,
) -> Result<Value, String> {
    if let Some(result) = crate::features::registry::execute(ctx,name,args).await {
        return result;
    }
    if let Some(result) = crate::integrations::registry::execute(ctx,name,args).await {
        return result;
    }
    match name {
        "workspace_list" => {
            let path = resolve_workspace(ctx.workspace, string_arg(args, "path").unwrap_or("."))?;
            list_dir(&path,args).await
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
            // Respect the advertised read contract. Silently cutting a requested
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
            list_dir(&path,args).await
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
        "delegate_task" => delegate(ctx, args).await,
        "task_status" => task_payload(ctx, args, false),
        "task_result" => task_payload(ctx, args, true),
        other => Err(format!("Unknown tool: {other}")),
    }
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
    let timeout = args["timeoutSeconds"]
        .as_u64()
        .unwrap_or(90)
        .clamp(5, 300);

    let session = ctx
        .state
        .store
        .put(
            "session",
            &json!({
                "title": format!("Delegate · {}", prompt.chars().take(36).collect::<String>()),
                "agentId": agent_id,
                "providerId": agent["providerId"],
                "model": agent["model"],
                "parentSessionId": ctx.session_id,
                "delegated": true,
            }),
        )
        .ok_or("위임 세션을 만들 수 없습니다.")?;
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let job = crate::engine::jobs::enqueue(
        ctx.state,
        "prompt",
        json!({
            "prompt": prompt,
            "agentId": agent_id,
            "providerId": agent["providerId"],
            "model": agent["model"],
            "sessionId": session_id,
            "toolsEnabled": ctx.tools_enabled && ctx.tools_execution_enabled,
            "toolsExecutionEnabled": ctx.tools_execution_enabled,
            "parentSessionId": ctx.session_id,
            "parentAgentId": ctx.agent_id,
            "delegation": true,
        }),
    )
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
    // An agent asking for one delegated result must not deserialize every
    // operation ever recorded by unrelated sessions. Keep newest-first order
    // and the existing 20-receipt payload bound.
    let operations = ctx.state.store.session_execution_operations(session_id)?
        .into_iter()
        .filter(|operation| {
            operation["runId"].as_str().is_some()
                && operation["status"] == "completed"
        })
        .map(|operation| {
            json!({"tool":operation["tool"],"arguments":operation["arguments"],"result":operation["result"]})
        })
        .take(20)
        .collect::<Vec<_>>();
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

async fn list_dir(path: &Path,args:&Value) -> Result<Value, String> {
    let limit=args["limit"].as_u64().unwrap_or(200).clamp(1,500) as usize;
    let cursor=args["cursor"].as_str();
    let mut page=std::collections::BTreeMap::<String,Value>::new();
    let mut reader = tokio::fs::read_dir(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    loop {
        let entry=match reader.next_entry().await {
            Ok(Some(entry))=>entry,
            Ok(None)=>break,
            Err(error)=>return Err(format!("{}: {error}",path.display())),
        };
        let name=entry.file_name().to_string_lossy().into_owned();
        if cursor.is_some_and(|cursor|name.as_str()<=cursor){continue;}
        let metadata = entry.metadata().await.ok();
        page.insert(name.clone(),json!({
            "name": name,
            "path": entry.path().to_string_lossy(),
            "type": if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) { "dir" } else { "file" },
            "size": metadata.as_ref().map(|m| m.len()).unwrap_or(0),
        }));
        if page.len()>limit+1 {
            if let Some(last)=page.keys().next_back().cloned(){page.remove(&last);}
        }
    }
    let truncated=page.len()>limit;
    let entries=page.values().take(limit).cloned().collect::<Vec<_>>();
    let next_cursor=truncated.then(||entries.last().and_then(|item|item["name"].as_str()).unwrap_or(""));
    Ok(json!({"path":path.to_string_lossy(),"entries":entries,
        "limit":limit,"truncated":truncated,"nextCursor":next_cursor}))
}

async fn read_text(path: &Path) -> Result<Value, String> {
    let text = read_source_text(path).await?;
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
    let text = read_source_text(path).await?;
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
    let source = read_source_text(path).await?;
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
    let Some(policy) = policy else {
        // Even without the opt-in guard, a caller that supplies a frozen
        // version gets a compare-before-write under the same per-file lock.
        let Some(expected) = expected_sha256 else {
            return write_text(path, content).await;
        };
        let canonical = canonical.map_err(|error| error.to_string())?;
        let observed = match sha256_file(&canonical).await {
            Ok(digest) => digest,
            Err(error) => {
                return Ok(json!({
                    "error": format!("파일을 다시 읽지 못했습니다: {error}"),
                    "staleVersion": true,
                    "expectedSha256": expected,
                }))
            }
        };
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
            "postSha256":format!("{:x}",Sha256::digest(content.as_bytes())),
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
                "postSha256":format!("{:x}",Sha256::digest(content.as_bytes()))}),
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
        "postSha256":format!("{:x}",Sha256::digest(content.as_bytes()))}),
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
        "written": true,
        "postSha256":format!("{:x}",Sha256::digest(content.as_bytes()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_account_connection_controls_tool_discovery_without_leaking_tokens(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!("dominion-connected-tools-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.clone(),
            workspace:root.clone()};
        let state=crate::state::AppState::new(config,
            Store::open(&root.join("db.sqlite")).unwrap(),Vault::open(&root.join("key")));
        let visible=|state:&crate::state::AppState|definitions_for(state).into_iter()
            .filter_map(|tool|tool["function"]["name"].as_str().map(str::to_string))
            .collect::<std::collections::HashSet<_>>();
        assert!(!visible(&state).contains("github_work_read"));
        assert!(!visible(&state).contains("google_workspace_read"));
        state.store.put("oauth_config",&json!({"id":"github","connected":true})).unwrap();
        state.save_secret("oauth:github:token",&json!({"accessToken":"internal-only"}).to_string());
        assert!(visible(&state).contains("github_work_read"));
        state.store.put("oauth_config",&json!({"id":"google","connected":true,
            "scopes":"openid email profile"})).unwrap();
        state.save_secret("oauth:google:token",&json!({"accessToken":"internal-only"}).to_string());
        assert!(!visible(&state).contains("google_workspace_read"));
        state.store.put("oauth_config",&json!({"id":"google","connected":true,
            "scopes":"openid email profile https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send https://www.googleapis.com/auth/drive https://www.googleapis.com/auth/documents https://www.googleapis.com/auth/calendar.events"})).unwrap();
        assert!(visible(&state).contains("google_workspace_read"));
        assert!(!definitions_for(&state).iter().any(|tool|tool.to_string().contains("internal-only")));
        state.store.remove_secret("oauth:github:token");
        assert!(!visible(&state).contains("github_work_read"));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn connected_account_tool_contracts_separate_reads_from_approved_mutations(){
        let names=definitions().into_iter()
            .filter_map(|item|item["function"]["name"].as_str().map(str::to_string))
            .collect::<std::collections::HashSet<_>>();
        for name in ["github_work_read","github_work_write",
            "google_workspace_read","google_workspace_write"] {
            assert!(names.contains(name),"missing tool schema: {name}");
        }
        for read in ["github_work_read","google_workspace_read"] {
            assert!(!requires_approval(read));
            assert!(dominion_agent_sdk::runtime::builtin_is_read_only(read));
        }
        for write in ["github_work_write","google_workspace_write"] {
            assert!(requires_approval(write));
            assert!(!dominion_agent_sdk::runtime::builtin_is_read_only(write));
        }
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
        crate::test_support::cleanup_dir(dir);
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
                    "id":"idx-test:src/engine/example.rs",
                    "indexId":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),
                    "path":"src/engine/example.rs",
                    "symbols":[
                        {"name":"run_loop","kind":"fn","line":120},
                        {"name":"record_example_lesson","kind":"fn","line":900}
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
        assert_eq!(result["matches"][0]["path"], "src/engine/example.rs");
        assert_eq!(result["matches"][0]["line"], 120);
        state.store.put("code_index",&json!({"id":"foreign","sourceDir":"/another/repository","generatedAt":"2099-01-01","files":1}));
        state.store.put("code_index_file",&json!({"id":"foreign-file","indexId":"foreign","path":"private.rs","symbols":[{"name":"run_loop","kind":"fn","line":999}]}));
        let multi=search_code_index(&ctx,"fn run_loop record_example_lesson",10).unwrap();
        assert_eq!(multi["matchCount"],2,"{multi}");
        assert!(multi["matches"].as_array().unwrap().iter().all(|row|row["path"]!="private.rs"));
        let path_result = search_code_index(&ctx, "example.rs", 10).unwrap();
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
    fn http_response_capture_stops_at_byte_limit() {
        let mut bytes=Vec::new();
        assert!(!append_http_chunk(&mut bytes,&vec![b'a';HTTP_FETCH_LIMIT-4]));
        assert!(append_http_chunk(&mut bytes,b"abcdefghij"));
        assert_eq!(bytes.len(),HTTP_FETCH_LIMIT);
        assert_eq!(&bytes[HTTP_FETCH_LIMIT-4..],b"abcd");
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
            "skill_search",
            "skill_read",
            "skill_resource_read",
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
        crate::test_support::cleanup_dir(dir);
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
        crate::test_support::cleanup_dir(dir);
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
