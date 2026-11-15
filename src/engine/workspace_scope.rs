//! Per-job workspace routing. A coding task must edit its chosen project,
//! not whichever repository happened to be the server's startup directory.
//! Keep the same Rust tool runtime, file-mutation locks and SQLite history.
use crate::state::AppState;
use std::path::{Path,PathBuf};

/// Validate and canonicalize before enqueuing and again when claiming a job.
/// Relative paths are never resolved against the daemon's working directory;
/// symlinks are resolved to one stable root, including across restarts.
pub fn resolve(state:&AppState,requested:&str)->Result<PathBuf,String>{
    let raw=requested.trim();
    if raw.is_empty() || raw.len()>4096 {
        return Err("작업 폴더 경로는 1~4096자여야 합니다.".into());
    }
    let path=Path::new(raw);
    if !path.is_absolute() {
        return Err("작업 폴더는 절대경로로 지정하세요.".into());
    }
    let directory=path.canonicalize()
        .map_err(|error|format!("작업 폴더에 접근할 수 없습니다: {error}"))?;
    if !directory.is_dir(){
        return Err("작업 폴더는 이미 존재하는 디렉터리여야 합니다.".into());
    }
    if directory.parent().is_none_or(|parent|parent==directory){
        return Err("전체 디스크 루트는 작업 폴더로 지정할 수 없습니다.".into());
    }
    if let Ok(data)=state.config.data_dir.canonicalize(){
        if directory.starts_with(&data){
            return Err("도미니온 내부 데이터 폴더는 작업 대상으로 사용할 수 없습니다.".into());
        }
    }
    Ok(directory)
}

/// Clone lightweight state/configuration only. Store, cancellation, approval,
/// event bus and file-mutation locks remain the native shared engine handles.
pub fn scope(state:&AppState,workspace:PathBuf)->AppState{
    let mut scoped=state.clone();
    scoped.config.workspace=workspace;
    scoped
}

