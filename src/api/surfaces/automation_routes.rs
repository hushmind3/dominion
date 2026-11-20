//! Product-domain HTTP endpoints, outside the minimal turn API.
//! Shared root router applies auth, CORS, maintenance and fallback.
use super::super::*;
pub(super) fn routes()->Router<AppState>{
    Router::<AppState>::new()
        .route("/api/run-wakes", get(system::run_wakes))
        .route("/api/jobs",get(jobs::list).post(jobs::create))
        .route("/api/jobs/:id",get(jobs::one))
        .route("/api/jobs/:id/cancel",post(jobs::cancel))
        .route("/api/jobs/:id/steer",post(jobs::steer))
        .route("/api/tasks", get(tasks::list))
        .route("/api/tasks/:id", get(tasks::one))
        .route("/api/executions", get(executions::list))
        .route("/api/executions/active", get(executions::active_executions))
        .route("/api/executions/:id", get(executions::one))
        .route("/api/executions/:id/cancel", post(executions::cancel))
}
