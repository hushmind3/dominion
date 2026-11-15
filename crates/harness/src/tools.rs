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
        if Path::new(path).components().any(|part| matches!(part, std::path::Component::ParentDir)) {
            return Err("상위 경로 이동은 허용되지 않습니다.".into());
        }
        let candidate = self.root.join(path);
        let parent = candidate.parent().unwrap_or(&self.root);
        let parent = parent.canonicalize().unwrap_or_else(|_| self.root.clone());
        if !parent.starts_with(&self.root) {
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
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.workspace.root)
            .output()
            .map_err(|error| format!("실행 실패: {error}"))?;
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        let text = text.chars().take(20_000).collect::<String>();
        Ok(json!({
            "exit": output.status.code(),
            "seconds": started.elapsed().as_secs_f64(),
            "output": text,
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
    fn escaping_the_workspace_is_rejected() {
        let dir = temp_workspace();
        let mut write = WriteFile(Workspace::new(&dir).unwrap());
        assert!(write.invoke(&json!({"path":"../escape.txt","content":"x"})).is_err());
        assert!(write.invoke(&json!({"path":"/tmp/abs.txt","content":"x"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
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
