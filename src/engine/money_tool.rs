//! Money is a detachable domain tool, not a second runtime or agent engine.
//! It shares the native job worker, model runner, HTTP tools, and SQLite Store.
use crate::{engine::jobs::JobContext, state::AppState};
use serde_json::Value;

/// The shared job lifecycle needs one callback, not Money-specific timers.
pub fn schedule(state: &AppState) {
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
    fn handles_only_money_owned_job_kinds() {
        for kind in ["money_discover", "money_ingest", "money_catalog", "money_fold"] {
            assert!(handles(kind));
        }
        for kind in ["prompt", "resume", "video", "learn", "room", ""] {
            assert!(!handles(kind));
        }
    }
}
