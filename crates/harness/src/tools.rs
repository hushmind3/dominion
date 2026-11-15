use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Permission, Tool, ToolSpec};

/// Workspace-scoped filesystem tools. Every path is resolved inside the root;
/// `..` escapes and absolute paths are rejected before touching the disk.
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        let root = root.canonicalize().map_err(|error| format!("작업 폴더 접근 실패: {error}"))?;
        Ok(Self { root })
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        if path.is_empty() || Path::new(path).is_absolute() {
            return Err("상대 경로만 허용됩니다.".into());
        }
        if Path::new(path).components().any(|part| matches!(part,
            std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_))) {
            return Err("상위 경로 이동은 허용되지 않습니다.".into());
        }
        let candidate = self.root.join(path);
        // Check the nearest existing ancestor, including the target itself.
        // A symlink pointing outside the workspace must never become a read
        // or a write target. Do not fall back to the root on canonicalize error.
        let mut existing = candidate.as_path();
        while !existing.exists() && !existing.is_symlink() {
            existing = existing.parent().ok_or("경로를 확인할 수 없습니다.")?;
        }
        let resolved = existing.canonicalize().map_err(|error| format!("경로 확인 실패: {error}"))?;
        if !resolved.starts_with(&self.root) {
            return Err("작업 폴더 밖의 경로입니다.".into());
        }
        Ok(candidate)
    }
}

pub struct ReadFile(pub Workspace);
impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs_read".into(),
            description: "Read a UTF-8 text file inside the workspace.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            permission: Permission::Read,
        }
    }
    fn invoke(&mut self, arguments: &Value) -> Result<Value, String> {
        let path = arguments["path"].as_str().ok_or("path가 필요합니다.")?;
        let path = self.0.resolve(path)?;
        let text = std::fs::read_to_string(&path).map_err(|error| format!("읽기 실패: {error}"))?;
        Ok(json!({"path": path.file_name().and_then(|n| n.to_str()).unwrap_or(""), "chars": text.chars().count(), "content": text}))
    }
}

pub struct WriteFile(pub Workspace);
impl Tool for WriteFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs_write".into(),
            description: "Write a UTF-8 text file inside the workspace.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            permission: Permission::Write,
        }
    }
    fn invoke(&mut self, arguments: &Value) -> Result<Value, String> {
        let path = arguments["path"].as_str().ok_or("path가 필요합니다.")?;
        let content = arguments["content"].as_str().ok_or("content가 필요합니다.")?;
        let target = self.0.resolve(path)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&target, content).map_err(|error| format!("쓰기 실패: {error}"))?;
        Ok(json!({"path": path, "bytes": content.len()}))
    }
}

/// Replace one exact span of a source file, refusing ambiguous or stale edits.
/// A patch costs fewer model tokens than rewriting a whole file.
pub struct PatchFile(pub Workspace);
impl Tool for PatchFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs_patch".into(),
            description: "Replace exactly one matching text span in a workspace UTF-8 file. Refuses missing or ambiguous matches.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "old":{"type":"string"},
                    "new":{"type":"string"}
                },
                "required":["path","old","new"]
            }),
            permission: Permission::Write,
        }
    }
    fn invoke(&mut self, arguments: &Value) -> Result<Value, String> {
        let path = arguments["path"].as_str().ok_or("path가 필요합니다.")?;
        let old = arguments["old"].as_str().filter(|part| !part.is_empty())
            .ok_or("old는 비어 있을 수 없습니다.")?;
        let new = arguments["new"].as_str().ok_or("new가 필요합니다.")?;
        let target = self.0.resolve(path)?;
        let previous = std::fs::read_to_string(&target)
            .map_err(|error| format!("수정할 파일 읽기 실패: {error}"))?;
        let hits = previous.match_indices(old).count();
        if hits != 1 {
            return Err(format!("코드 패치 거부: old 텍스트가 {hits}개 일치합니다 (정확히 1개 필요)."));
        }
        let updated = previous.replacen(old, new, 1);
        // Resolve the final path a second time; a symlink may have changed
        // while the model was preparing its requested patch.
        let verified = self.0.resolve(path)?;
        if verified != target { return Err("패치 대상 경로가 변경됐습니다.".into()); }
        std::fs::write(&target, updated.as_bytes())
            .map_err(|error| format!("패치 기록 실패: {error}"))?;
        Ok(json!({
            "path":path, "changed":true, "oldBytes":old.len(), "newBytes":new.len()
        }))
    }
}

pub struct ListDir(pub Workspace);
impl Tool for ListDir {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs_list".into(),
            description: "List files and folders inside the workspace.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
            permission: Permission::Read,
        }
    }
    fn invoke(&mut self, arguments: &Value) -> Result<Value, String> {
        let path = arguments["path"].as_str().unwrap_or(".");
        let path = self.0.resolve(path)?;
        let mut rows = Vec::new();
        for entry in std::fs::read_dir(&path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            rows.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "dir": entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false),
            }));
        }
        rows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(json!({"entries": rows}))
    }
}

/// Shell execution inside the workspace. Output is capped; a timeout is
/// enforced by the caller's process policy, not silently here.
pub struct Shell {
    pub workspace: Workspace,
    pub timeout_secs: u64,
}

impl Tool for Shell {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell".into(),
            description: "Run a shell command in the workspace and return combined output.".into(),
            parameters: json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
            permission: Permission::Execute,
        }
    }
    fn invoke(&mut self, arguments: &Value) -> Result<Value, String> {
        let command = arguments["command"].as_str().ok_or("command가 필요합니다.")?;
        let started = std::time::Instant::now();
        let mut child = Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .arg(if cfg!(windows) { "/C" } else { "-c" })
            .arg(command)
            .current_dir(&self.workspace.root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("실행 실패: {error}"))?;

        // Drain both pipes while polling, otherwise a verbose test runner can
        // fill the OS pipe and deadlock before its deadline is reached.
        fn drain<R: std::io::Read + Send + 'static>(mut reader: R) -> std::thread::JoinHandle<Vec<u8>> {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let size = match reader.read(&mut chunk) { Ok(0) | Err(_) => break, Ok(n) => n };
                    let remaining = 20_000usize.saturating_sub(bytes.len());
                    bytes.extend_from_slice(&chunk[..size.min(remaining)]);
                }
                bytes
            })
        }
        let stdout = drain(child.stdout.take().ok_or("stdout unavailable")?);
        let stderr = drain(child.stderr.take().ok_or("stderr unavailable")?);
        let deadline = std::time::Duration::from_secs(self.timeout_secs);
        let (status, timed_out) = loop {
            match child.try_wait() {
                Ok(Some(status)) => break (status, false),
                Ok(None) if started.elapsed() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let status = child.wait().map_err(|error| error.to_string())?;
                    break (status, true);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("실행 상태 확인 실패: {error}"));
                }
            }
        };
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        let mut output = String::from_utf8_lossy(&stdout).to_string();
        output.push_str(&String::from_utf8_lossy(&stderr));
        let output: String = output.chars().take(20_000).collect();
        if timed_out {
            return Err(format!("shell timeout after {} seconds: {}", self.timeout_secs, output));
        }
        if !status.success() {
            return Err(format!("shell exited {:?}: {}", status.code(), output));
        }
        Ok(json!({
            "exit": status.code(),
            "seconds": started.elapsed().as_secs_f64(),
            "output": output,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "harness-fs-{}-{}-{}", std::process::id(), nanos, NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_then_read_round_trips_inside_the_workspace() {
        let dir = temp_workspace();
        let mut write = WriteFile(Workspace::new(&dir).unwrap());
        write.invoke(&json!({"path":"src/a.txt","content":"hello"})).unwrap();
        let mut read = ReadFile(Workspace::new(&dir).unwrap());
        let value = read.invoke(&json!({"path":"src/a.txt"})).unwrap();
        assert_eq!(value["content"], "hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patch_requires_exactly_one_match_and_preserves_other_content() {
        let dir = temp_workspace();
        std::fs::write(dir.join("main.rs"), "let a = 1;\\nlet b = 1;\\n".replace("\\n", "\n")).unwrap();
        let mut patch = PatchFile(Workspace::new(&dir).unwrap());
        assert!(patch.invoke(&json!({"path":"main.rs","old":"= 1","new":"= 2"})).is_err());
        let report = patch.invoke(&json!({"path":"main.rs","old":"let a = 1;","new":"let a = 2;"})).unwrap();
        assert_eq!(report["changed"], true);
        assert_eq!(std::fs::read_to_string(dir.join("main.rs")).unwrap(),
            "let a = 2;\\nlet b = 1;\\n".replace("\\n", "\n"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn escaping_the_workspace_is_rejected() {
        let dir = temp_workspace();
        let mut write = WriteFile(Workspace::new(&dir).unwrap());
        assert!(write.invoke(&json!({"path":"../escape.txt","content":"x"})).is_err());
        assert!(write.invoke(&json!({"path":"/tmp/abs.txt","content":"x"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_targets_outside_workspace_are_rejected() {
        let dir = temp_workspace();
        let outer = temp_workspace();
        std::os::unix::fs::symlink(&outer, dir.join("outside")).unwrap();
        let mut write = WriteFile(Workspace::new(&dir).unwrap());
        assert!(write.invoke(&json!({"path":"outside/leak.txt","content":"never"})).is_err());
        assert!(!outer.join("leak.txt").exists());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(outer);
    }
    #[cfg(unix)]
    #[test]
    fn shell_rejects_failed_commands_and_enforces_timeout() {
        let dir = temp_workspace();
        let mut shell = Shell { workspace: Workspace::new(&dir).unwrap(), timeout_secs: 1 };
        assert!(shell.invoke(&json!({"command":"exit 7"})).unwrap_err().contains("7"));
        assert!(shell.invoke(&json!({"command":"sleep 3"})).unwrap_err().contains("timeout"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shell_reports_the_exit_code_and_output() {
        let dir = temp_workspace();
        let mut shell = Shell { workspace: Workspace::new(&dir).unwrap(), timeout_secs: 10 };
        let value = shell.invoke(&json!({"command":"printf ok"})).unwrap();
        assert_eq!(value["exit"], 0);
        assert!(value["output"].as_str().unwrap().contains("ok"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
