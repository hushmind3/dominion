//! Source-install updates.
//!
//! The packaged desktop app applies signed releases. This module only
//! rebuilds and restarts a macOS source install that the LaunchAgent owns.
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::process::Command;

const CONFIG_KIND: &str = "dev_config";
const CONFIG_ID: &str = "default";

static UPDATING: AtomicBool = AtomicBool::new(false);

pub fn installation_running() -> bool {
    UPDATING.load(Ordering::Acquire)
}

pub(super) fn toolchain_path() -> Option<std::ffi::OsString> {
    let mut paths = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
    {
        paths.insert(0, home.join(".cargo/bin"));
        paths.insert(0, home.join(".local/bin"));
        paths.insert(0, home.join(".bun/bin"));
    }
    if let Some(value) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&value));
    }
    std::env::join_paths(paths).ok()
}

/// Existing installs keep their source folder in `dev_config`; a checkout
/// launched from its own root does not need it configured again.
fn source_dir(state: &AppState) -> Option<PathBuf> {
    let configured = state
        .store
        .get(CONFIG_KIND, CONFIG_ID)
        .and_then(|value| value.get("sourceDir").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    if configured.trim().is_empty() {
        let cwd = std::env::current_dir().ok()?;
        if cwd.join(".git").exists()
            && cwd.join("Cargo.toml").is_file()
            && cwd.join("src/engine/mod.rs").is_file()
        {
            return Some(cwd);
        }
        return None;
    }
    let expanded = if let Some(rest) = configured.strip_prefix("~/") {
        std::env::var("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|_| PathBuf::from(&configured))
    } else {
        PathBuf::from(&configured)
    };
    Some(expanded)
}

async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Only source installs managed by the local macOS LaunchAgent can safely
/// rebuild and restart the running process. Packaged apps need signed releases.
fn live_source_root(state: &AppState) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("소스 자동 업데이트는 현재 macOS LaunchAgent 설치에서 지원합니다. 설치형 앱은 릴리스 업데이트를 사용하세요.".into());
    }
    let dir = source_dir(state).ok_or("DOMINION 소스 설치 폴더를 찾지 못했습니다.")?;
    let actual = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|e| e.to_string())?;
    let expected = dir
        .join("target/release/dominion-rust")
        .canonicalize()
        .map_err(|_| "현재 실행 중인 서버가 소스 빌드인지 확인하지 못했습니다.")?;
    if actual != expected {
        return Err("이 서버는 소스 설치형 Rust 엔진이 아닙니다. 설치형 앱의 배포 업데이트를 사용하세요.".into());
    }
    if !dir.join(".git").exists() {
        return Err("Git 저장소가 없어 소스 갱신을 진행할 수 없습니다.".into());
    }
    if !dir.join("scripts/install-launch-agent.sh").exists() {
        return Err("macOS 자동 재시작 스크립트가 없습니다.".into());
    }
    Ok(dir)
}

pub async fn update_status(state: &AppState) -> Value {
    let dir = match live_source_root(state) {
        Ok(dir) => dir,
        Err(message) => return json!({"supported":false,"available":false,"reason":message}),
    };
    let branch = git(&dir, &["branch", "--show-current"]).await.unwrap_or_default();
    if branch.trim() != "main" {
        return json!({"supported":false,"available":false,"reason":"main 브랜치에서 실행한 경우에만 업데이트할 수 있습니다."});
    }
    let local = git(&dir, &["rev-parse", "HEAD"]).await.unwrap_or_default().trim().to_string();
    let dirty = git(&dir, &["status", "--porcelain"]).await.unwrap_or_default();
    if !dirty.trim().is_empty() {
        return json!({"supported":false,"available":false,"local":local,"reason":"수정된 소스 파일이 있어 업데이트를 중단했습니다. 변경 내용을 보존한 채 개발 작업에서 정리하세요."});
    }
    let remote = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new("git")
            .args(["ls-remote", "--exit-code", "origin", "refs/heads/main"])
            .current_dir(&dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output(),
    )
    .await;
    match remote {
        Ok(Ok(output)) if output.status.success() => {
            let sha = String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if sha.len() != 40 {
                return json!({"supported":true,"available":false,"local":local,"reason":"원격 버전 정보를 읽지 못했습니다."});
            }
            json!({"supported":true,"available":sha!=local,"local":local,"remote":sha,"running":crate::build_identity::running().commit,"installMode":"source"})
        }
        _ => json!({"supported":true,"available":false,"local":local,"reason":"업데이트 서버에 연결할 수 없습니다. 인터넷 연결을 확인하세요."}),
    }
}

pub fn start_update_job(state: AppState) -> Result<Value, String> {
    if UPDATING.swap(true, Ordering::AcqRel) {
        return Err("이미 업데이트를 진행하고 있습니다.".into());
    }
    let record = json!({
        "kind": "update",
        "status": "running",
        "logs": [],
        "input": {},
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    let Some(job) = state.store.put("dev_job", &record) else {
        UPDATING.store(false, Ordering::Release);
        return Err("업데이트 작업을 시작하지 못했습니다.".into());
    };
    let Some(job_id) = job["id"].as_str().map(str::to_string) else {
        UPDATING.store(false, Ordering::Release);
        return Err("업데이트 작업을 시작하지 못했습니다.".into());
    };
    let state_clone = state.clone();
    tokio::spawn(async move {
        run_update_job(state_clone, job_id).await;
    });
    Ok(job)
}

async fn run_update_job(state: AppState, job_id: String) {
    let result = update_source(&state).await;
    let needs_restart = result
        .as_ref()
        .ok()
        .and_then(|(_, value)| value["restartRequired"].as_bool())
        .unwrap_or(false);
    let mut job = state.store.get("dev_job", &job_id).unwrap_or_else(|| json!({}));
    match result {
        Ok((logs, summary)) => {
            job["status"] = json!("completed");
            job["logs"] = json!(logs);
            job["result"] = summary;
        }
        Err((logs, error)) => {
            job["status"] = json!("failed");
            job["logs"] = json!(logs);
            job["error"] = json!(error);
        }
    }
    job["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("dev_job", &job) {
        state.bus.record("dev_job", &saved);
    }
    state.store.audit("dev.job", json!({ "jobId": job_id, "kind": "update" }));
    UPDATING.store(false, Ordering::Release);
    if needs_restart {
        // The platform installer registers (or refreshes) the LaunchAgent.
        // It executes after the successful job is committed to SQLite.
        if let Some(dir) = source_dir(&state) {
            let script = dir.join("scripts/install-launch-agent.sh");
            let _ = Command::new("/bin/zsh")
                .arg(script)
                .current_dir(dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
}

async fn update_source(state: &AppState) -> Result<(Vec<String>, Value), (Vec<String>, String)> {
    let dir = live_source_root(state).map_err(|e| (vec![], e))?;
    let branch = git(&dir, &["branch", "--show-current"]).await.map_err(|e| (vec![], e))?;
    if branch.trim() != "main" {
        return Err((vec![], "main 브랜치에서만 업데이트할 수 있습니다.".into()));
    }
    let dirty = git(&dir, &["status", "--porcelain"]).await.map_err(|e| (vec![], e))?;
    if !dirty.trim().is_empty() {
        return Err((vec![], "소스 수정 사항이 있어 작업을 중단했습니다. 파일은 변경하지 않았습니다.".into()));
    }
    let old = git(&dir, &["rev-parse", "HEAD"]).await.map_err(|e| (vec![], e))?.trim().to_string();
    let mut logs = vec![format!("현재 버전: {}", old.chars().take(12).collect::<String>())];
    let commands: Vec<(&str, Vec<&str>, u64)> = vec![
        ("git", vec!["fetch", "origin", "main"], 120),
        ("git", vec!["merge", "--ff-only", "FETCH_HEAD"], 30),
        ("npm", vec!["ci", "--prefix", "web"], 300),
        ("cargo", vec!["build", "--locked", "--release"], 900),
        ("npm", vec!["run", "build", "--prefix", "web"], 180),
    ];
    for (program, args, seconds) in commands {
        logs.push(format!("{} {}", program, args.join(" ")));
        let mut command = Command::new(program);
        command
            .args(&args)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .kill_on_drop(true);
        if let Some(path) = toolchain_path() {
            command.env("PATH", path);
        }
        let output = tokio::time::timeout(Duration::from_secs(seconds), command.output()).await;
        let output = match output {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err((logs, format!("{program} 실행 실패: {e}"))),
            Err(_) => return Err((logs, format!("{program} 실행 제한 시간을 초과했습니다."))),
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for output in [&stdout[..], &stderr[..]] {
            let mut recent = output.lines().rev().take(45).map(str::to_string).collect::<Vec<_>>();
            recent.reverse();
            logs.extend(recent);
        }
        if logs.len() > 180 {
            logs = logs.split_off(logs.len() - 180);
        }
        if !output.status.success() {
            return Err((
                logs,
                format!("{} 실패 (종료 코드 {:?}). 실행 중인 DOMINION은 유지됩니다.", program, output.status.code()),
            ));
        }
    }
    let new = git(&dir, &["rev-parse", "HEAD"]).await.unwrap_or_default().trim().to_string();
    logs.push(format!("새 버전 빌드 완료: {}", new.chars().take(12).collect::<String>()));
    Ok((logs, json!({"previous":old,"installed":new,"restartRequired":true})))
}
