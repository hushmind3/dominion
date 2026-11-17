//! Money is a detachable domain tool, not a second runtime or agent engine.
//! It shares the native job worker, model runner, HTTP tools, and SQLite Store.
use crate::{engine::jobs::JobContext, state::AppState};
use serde_json::Value;

/// The shared job lifecycle needs one callback, not Money-specific timers.
pub fn enabled(state:&AppState)->bool{
    state.store.get("money_module","global")
        .is_some_and(|row|row["enabled"]==true)
}
pub fn ensure_live(context:&JobContext)->Result<(),String>{
    if !enabled(&context.state){return Err("머니 기능 중단: 비활성화됨".into());}
    let Some(job)=context.state.store.get("job",&context.job_id)else{
        return Err("머니 작업이 사라졌습니다.".into());
    };
    if job["cancelRequested"]==true || job["status"]=="cancelled"{
        return Err("머니 기능 중단: 취소된 작업".into());
    }
    Ok(())
}
pub fn schedule(state: &AppState) {
    if !enabled(state){return;}
    super::money_health::sample(state);
    super::money_sources::schedule(state);
    super::money_catalog::schedule(state);
    super::money_folding::schedule(state);
    super::money::schedule_autopilot(state);
}

pub fn handles(kind: &str) -> bool {
    matches!(kind, "money_discover" | "money_ingest" | "money_catalog" | "money_fold")
}

/// Delegate the tool's work to domain code; execution, budgets, checkpoints,
/// cancellation and recovery remain owned by the shared job harness.
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    ensure_live(context)?;
    match job["kind"].as_str().unwrap_or("") {
        "money_discover" => super::money::run(context, job).await,
        "money_ingest" => super::money_sources::run(context, job).await,
        "money_catalog" => super::money_catalog::run(context, job).await,
        "money_fold" => super::money_folding::run(context, job).await,
        other => Err(format!("Money 도구가 지원하지 않는 작업 종류: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_money_does_not_queue_background_collection_or_model_work(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!("money-idle-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),web_dir:root.join("web"),
            workspace:root.clone()};
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        assert!(!enabled(&state));
        schedule(&state);
        assert_eq!(state.store.count_kind("job"),0);
        state.store.money_set_module_enabled(true).unwrap();
        schedule(&state);
        assert!(state.store.count_kind("job")>=2);
        let (settings,_,stopped)=state.store.money_set_module_enabled(false).unwrap();
        assert_eq!(settings["enabled"],false);
        assert!(stopped.len()>=2);
        schedule(&state);
        assert_eq!(state.store.count_kind("job"),stopped.len());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn handles_only_money_owned_job_kinds() {
        for kind in ["money_discover", "money_ingest", "money_catalog", "money_fold"] {
            assert!(handles(kind));
        }
        for kind in ["prompt", "resume", "video", "learn", "room", ""] {
            assert!(!handles(kind));
        }
    }
}
