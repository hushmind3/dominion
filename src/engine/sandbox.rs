//! Enforced OS process isolation for explicitly isolated workers. Unsupported
//! required backends fail closed; existing owner sessions retain their policy.
use std::path::Path;
use tokio::process::Command;

pub fn required(state: &crate::state::AppState, session: &str) -> bool {
    state
        .store
        .get("session", session)
        .is_some_and(|s| s["shellSandbox"] == "required")
        || std::env::var("DOMINION_SHELL_SANDBOX").is_ok_and(|v| v == "required")
}
pub fn script(workspace: &Path, script: &str) -> Result<Command, String> {
    if script.is_empty() || script.len() > 65536 {
        return Err("유효하지 않은 셸 명령 길이입니다.".into());
    }
    command(workspace, "/bin/sh", &["-c".into(), script.into()])
}
pub fn command(workspace: &Path, program: &str, args: &[String]) -> Result<Command, String> {
    let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
    sandboxed(&workspace, program, args)
}
#[cfg(target_os = "macos")]
fn sandboxed(workspace: &Path, program: &str, args: &[String]) -> Result<Command, String> {
    if !Path::new("/usr/bin/sandbox-exec").is_file() {
        return Err("필수 macOS 셸 격리 백엔드가 없습니다.".into());
    }
    let path = workspace
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let profile = format!(
        r#"(version 1)
(deny default)
(allow process*)
(allow file-read-metadata)
(allow file-read* (literal "/") (literal "/private/var/select/sh") (literal "/dev/null") (subpath "/usr") (subpath "/bin") (subpath "/System") (subpath "/Library") (subpath "{path}"))
(allow file-write* (literal "/dev/null") (subpath "{path}") (subpath "/private/tmp"))
(deny network*)"#
    );
    let mut c = Command::new("/usr/bin/sandbox-exec");
    c.arg("-p")
        .arg(profile)
        .arg(program)
        .args(args)
        .current_dir(workspace)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .env("HOME", workspace);
    Ok(c)
}
#[cfg(target_os = "linux")]
fn sandboxed(workspace: &Path, program: &str, args: &[String]) -> Result<Command, String> {
    let backend = ["/usr/bin/bwrap", "/bin/bwrap"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("필수 Linux 셸 격리에는 bubblewrap이 필요합니다.")?;
    let mut c = Command::new(backend);
    c.args([
        "--die-with-parent",
        "--new-session",
        "--unshare-pid",
        "--unshare-net",
        "--unshare-ipc",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
    ]);
    for dir in [
        "/usr",
        "/bin",
        "/lib",
        "/lib64",
        "/etc/ssl",
        "/etc/alternatives",
    ] {
        if Path::new(dir).exists() {
            c.arg("--ro-bind").arg(dir).arg(dir);
        }
    }
    c.arg("--bind")
        .arg(workspace)
        .arg(workspace)
        .arg("--chdir")
        .arg(workspace)
        .arg("--setenv")
        .arg("HOME")
        .arg(workspace)
        .args(["--setenv", "PATH", "/usr/local/bin:/usr/bin:/bin", "--"])
        .arg(program)
        .args(args)
        .current_dir(workspace)
        .env_clear();
    Ok(c)
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn sandboxed(_workspace: &Path, _program: &str, _args: &[String]) -> Result<Command, String> {
    Err("이 운영체제에는 필수 프로세스 격리 백엔드가 없습니다.".into())
}
