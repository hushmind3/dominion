//! Optional source isolation for delegated coding jobs. Child changes remain
//! reviewable until the parent explicitly integrates the exact frozen patch.
use crate::{engine::tools::ToolContext, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{io::AsyncWriteExt, process::Command};

fn git_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return format!("//{}", rest.replace('\\', "/"));
        }
        return text
            .strip_prefix(r"\\?\")
            .unwrap_or(&text)
            .replace('\\', "/");
    }
    #[cfg(not(windows))]
    {
        text.into_owned()
    }
}

async fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        Command::new("git")
            .args([
                "-c",
                "core.hooksPath=",
                "-c",
                "core.fsmonitor=false",
                "--no-optional-locks",
            ])
            .args(args)
            .current_dir(root)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Git 작업공간 명령 시간 초과")?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(2400)
            .collect());
    }
    Ok(output.stdout)
}
async fn apply(root: &Path, patch: &[u8], check: bool) -> Result<(), String> {
    let mut command = Command::new("git");
    command.arg("apply").arg("--binary");
    if check {
        command.arg("--check");
    }
    let mut child = command
        .arg("--")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().ok_or("Git 패치 입력이 없습니다.")?;
    let output = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        input.write_all(patch).await.map_err(|e| e.to_string())?;
        drop(input);
        child.wait_with_output().await.map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Git 패치 적용 시간 초과")??;
    if !output.status.success() {
        return Err(format!(
            "패치 충돌: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}
pub async fn prepare(state: &AppState, source: &Path, job: &str) -> Result<Value, String> {
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let head = String::from_utf8(git(&source, &["rev-parse", "HEAD"]).await?)
        .map_err(|e| e.to_string())?;
    let patch = git(
        &source,
        &["diff", "--binary", "--no-ext-diff", "HEAD", "--"],
    )
    .await?;
    if !patch.is_empty() {
        return Err("격리 위임은 커밋된 작업공간에서 시작하세요. 현재 변경은 보존했습니다.".into());
    }
    let status = git(
        &source,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    if !status.is_empty() {
        return Err(
            "격리 위임 전 추적되지 않은 소스를 커밋하거나 공유 작업공간 모드를 사용하세요.".into(),
        );
    }
    let root = state.config.data_dir.join("delegation-workspaces");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let target = root.canonicalize().map_err(|e| e.to_string())?.join(job);
    let target_text = git_path(&target);
    git(
        &source,
        &["worktree", "add", "--detach", &target_text, head.trim()],
    )
    .await?;
    Ok(json!({"path":target,"source":source,"baseCommit":head.trim(),"jobId":job}))
}
pub async fn discard_prepared(workspace: &Value) -> Result<(), String> {
    let source = Path::new(
        workspace["source"]
            .as_str()
            .ok_or("원본 경로가 없습니다.")?,
    );
    let path = workspace["path"].as_str().ok_or("격리 경로가 없습니다.")?;
    git(source, &["worktree", "remove", "--force", &path]).await?;
    Ok(())
}
pub fn scoped_workspace(state: &AppState, session: &Value) -> Result<Option<PathBuf>, String> {
    let owned = &session["delegationWorkspace"];
    let Some(raw) = owned["path"].as_str() else {
        return Ok(None);
    };
    let job = owned["jobId"]
        .as_str()
        .ok_or("격리 작업 식별자가 없습니다.")?;
    let expected = state
        .config
        .data_dir
        .join("delegation-workspaces")
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join(job);
    let actual = Path::new(raw).canonicalize().map_err(|e| e.to_string())?;
    if actual != expected {
        return Err("격리 작업공간 경로가 저장된 위임과 일치하지 않습니다.".into());
    }
    Ok(Some(actual))
}
pub async fn freeze_result(state: &AppState, job: &Value) -> Result<(), String> {
    let Some(session) = job["input"]["sessionId"]
        .as_str()
        .and_then(|id| state.store.get("session", id))
    else {
        return Ok(());
    };
    let Some(child) = scoped_workspace(state, &session)? else {
        return Ok(());
    };
    git(&child, &["add", "-N", "--", "."]).await?;
    let patch = git(&child, &["diff", "--binary", "--no-ext-diff", "HEAD", "--"]).await?;
    let hash = format!("{:x}", Sha256::digest(&patch));
    let id = job["id"].as_str().ok_or("위임 작업 식별자가 없습니다.")?;
    state
        .store
        .patch_if(
            "job",
            id,
            "status",
            &json!("completed"),
            &json!({"delegationPatchSha256":hash,
        "delegationPatchBytes":patch.len(),"delegationWorkspace":session["delegationWorkspace"]}),
        )
        .ok_or("완료된 위임 패치 증거를 고정할 수 없습니다.")?;
    Ok(())
}

pub async fn inspect(workspace: &Path, diff: bool) -> Result<Value, String> {
    let args = if diff {
        vec![
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
        ]
    } else {
        vec!["status", "--short", "--untracked-files=normal"]
    };
    let bytes = git(workspace, &args).await?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(
        json!({"content":text.chars().take(32000).collect::<String>(),"truncated":text.chars().count()>32000}),
    )
}

pub async fn integrate(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let id = args["jobId"].as_str().ok_or("작업 식별자가 필요합니다.")?;
    let job = ctx.store.get("job", id).ok_or("위임 작업이 없습니다.")?;
    if job["input"]["parentSessionId"] != ctx.session_id || job["status"] != "completed" {
        return Err("이 세션이 위임한 완료 작업만 통합할 수 있습니다.".into());
    }
    if job["integratedAt"].is_string() {
        return Err("이미 통합한 위임 작업입니다.".into());
    }
    let session = ctx
        .store
        .get(
            "session",
            job["input"]["sessionId"]
                .as_str()
                .ok_or("자식 세션이 없습니다.")?,
        )
        .ok_or("자식 세션이 없습니다.")?;
    let child = scoped_workspace(ctx.state, &session)?.ok_or("격리 Git 작업공간이 아닙니다.")?;
    let source = Path::new(
        session["delegationWorkspace"]["source"]
            .as_str()
            .ok_or("원본 경로가 없습니다.")?,
    )
    .canonicalize()
    .map_err(|e| e.to_string())?;
    if source != ctx.workspace.canonicalize().map_err(|e| e.to_string())? {
        return Err("현재 부모 작업공간이 원본과 다릅니다.".into());
    }
    let base = session["delegationWorkspace"]["baseCommit"]
        .as_str()
        .ok_or("기준 커밋이 없습니다.")?;
    for path in [&source, &child] {
        if String::from_utf8_lossy(&git(path, &["rev-parse", "HEAD"]).await?).trim() != base {
            return Err("부모 또는 자식의 기준 커밋이 변경되었습니다.".into());
        }
    }
    git(&child, &["add", "-N", "--", "."]).await?;
    let patch = git(&child, &["diff", "--binary", "--no-ext-diff", "HEAD", "--"]).await?;
    if job["delegationPatchSha256"].as_str()
        != Some(format!("{:x}", Sha256::digest(&patch)).as_str())
    {
        return Err(
            "위임 완료 시 고정된 패치와 현재 파일이 다릅니다. 새 변경을 확인하세요.".into(),
        );
    }
    if patch.len() > 524288 {
        return Err("위임 패치가 512KB를 초과합니다.".into());
    }
    if patch.is_empty() {
        return Ok(json!({"jobId":id,"changed":false}));
    }
    // --numstat -z reports the exact target paths for shared mutation locks.
    let names = git(&child, &["diff", "--numstat", "-z", "HEAD", "--"]).await?;
    let mut paths = names
        .split(|b| *b == 0)
        .filter(|v| !v.is_empty())
        .map(|item| {
            let text = std::str::from_utf8(item).map_err(|e| e.to_string())?;
            let path = text
                .splitn(3, '\t')
                .nth(2)
                .filter(|p| !p.is_empty())
                .ok_or("이름 변경 패치는 수동 검토가 필요합니다.")?;
            crate::engine::tools::resolve_workspace(&source, path)
        })
        .collect::<Result<Vec<_>, String>>()?;
    paths.sort();
    paths.dedup();
    let mut guards = Vec::new();
    for path in paths {
        guards.push(
            crate::engine::tools::mutation_lock(ctx.state, &path)
                .lock_owned()
                .await,
        );
    }
    apply(&source, &patch, true).await?;
    if let Err(error) = apply(&source, &patch, false).await {
        return Ok(
            json!({"jobId":id,"error":error,"outcomeUnknown":true,"patchSha256":job["delegationPatchSha256"]}),
        );
    }
    let saved = match ctx.store.patch_if(
        "job",
        id,
        "integratedAt",
        &Value::Null,
        &json!({"integratedAt":chrono::Utc::now().to_rfc3339()}),
    ) {
        Some(saved) => saved,
        None => {
            return Ok(
                json!({"jobId":id,"error":"통합 결과 기록에 실패했습니다. 현재 파일을 확인하세요.","outcomeUnknown":true,"patchSha256":job["delegationPatchSha256"]}),
            )
        }
    };
    ctx.state.bus.record("job", &saved);
    Ok(json!({"jobId":id,"changed":true,"patchBytes":patch.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    async fn fixture() -> (AppState, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("dominion-child-worktree-{}", uuid::Uuid::new_v4()));
        let source = root.join("project");
        std::fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-q"]).await.unwrap();
        git(&source, &["config", "user.name", "Fixture"])
            .await
            .unwrap();
        git(&source, &["config", "user.email", "fixture@example.test"])
            .await
            .unwrap();
        std::fs::write(source.join("target.txt"), "original\n").unwrap();
        git(&source, &["add", "."]).await.unwrap();
        git(&source, &["commit", "-qm", "baseline"]).await.unwrap();
        let config = Config {
            port: 0,
            data_dir: root.join("data"),
            web_dir: root.join("web"),
            workspace: source.clone(),
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state = AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        (state, source)
    }
    #[tokio::test]
    async fn child_patch_stays_isolated_until_approved_integration_and_cannot_replay() {
        let (state, source) = fixture().await;
        let workspace = prepare(&state, &source, "child-job").await.unwrap();
        let path = Path::new(workspace["path"].as_str().unwrap());
        std::fs::write(path.join("target.txt"), "child edit\n").unwrap();
        std::fs::write(path.join("new.txt"), "new child file\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(source.join("target.txt")).unwrap(),
            "original\n"
        );
        state.store.put(
            "session",
            &json!({"id":"child","delegationWorkspace":workspace}),
        );
        let job = json!({"id":"child-job","status":"completed","input":{"sessionId":"child","parentSessionId":"parent"}});
        state.store.put("job", &job);
        freeze_result(&state, &job).await.unwrap();
        let ctx = ToolContext {
            workspace: &source,
            store: &state.store,
            state: &state,
            session_id: "parent",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(crate::engine::tools::requires_approval("task_integrate"));
        let result = integrate(&ctx, &json!({"jobId":"child-job"}))
            .await
            .unwrap();
        assert_eq!(result["changed"], true);
        assert_eq!(
            std::fs::read_to_string(source.join("target.txt")).unwrap(),
            "child edit\n"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("new.txt")).unwrap(),
            "new child file\n"
        );
        assert!(integrate(&ctx, &json!({"jobId":"child-job"}))
            .await
            .is_err());
    }
    #[tokio::test]
    async fn changed_child_after_frozen_completion_is_rejected() {
        let (state, source) = fixture().await;
        let workspace = prepare(&state, &source, "child-job").await.unwrap();
        let path = Path::new(workspace["path"].as_str().unwrap());
        std::fs::write(path.join("target.txt"), "first\n").unwrap();
        state.store.put(
            "session",
            &json!({"id":"child","delegationWorkspace":workspace}),
        );
        let job = json!({"id":"child-job","status":"completed","input":{"sessionId":"child","parentSessionId":"parent"}});
        state.store.put("job", &job);
        freeze_result(&state, &job).await.unwrap();
        std::fs::write(path.join("target.txt"), "unapproved second edit\n").unwrap();
        let ctx = ToolContext {
            workspace: &source,
            store: &state.store,
            state: &state,
            session_id: "parent",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(integrate(&ctx, &json!({"jobId":"child-job"}))
            .await
            .unwrap_err()
            .contains("고정된 패치"));
        assert_eq!(
            std::fs::read_to_string(source.join("target.txt")).unwrap(),
            "original\n"
        );
    }

    #[tokio::test]
    async fn readonly_worker_cannot_write_even_with_a_forged_tool_call() {
        let (state, source) = fixture().await;
        state
            .store
            .put("session", &json!({"id":"readonly","readOnly":true}));
        let ctx = ToolContext {
            workspace: &source,
            store: &state.store,
            state: &state,
            session_id: "readonly",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(crate::engine::tools::execute(
            &ctx,
            "workspace_write",
            &json!({"path":"target.txt","content":"forged"}),
            "r"
        )
        .await
        .is_err());
        assert_eq!(
            std::fs::read_to_string(source.join("target.txt")).unwrap(),
            "original\n"
        );
        assert!(crate::engine::tools::execute(
            &ctx,
            "workspace_read",
            &json!({"path":"target.txt"}),
            "r"
        )
        .await
        .is_ok());
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn enforced_shell_allows_workspace_output_and_denies_parent_write() {
        let (_state, source) = fixture().await;
        std::fs::write(
            source.parent().unwrap().join("secret.txt"),
            "OUTSIDE_SECRET",
        )
        .unwrap();
        let mut command = crate::engine::sandbox::script(
            &source,
            "printf allowed > inside.txt; cat ../secret.txt; printf denied > ../outside.txt",
        )
        .unwrap();
        let output = command.output().await.unwrap();
        assert!(!output.status.success());
        assert!(
            source.join("inside.txt").exists(),
            "sandbox output: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(source.join("inside.txt")).unwrap(),
            "allowed"
        );
        assert!(!source.parent().unwrap().join("outside.txt").exists());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("OUTSIDE_SECRET"));
    }
}
