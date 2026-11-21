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
        .route("/api/activity/teams", get(executions::team_activity))
        .route("/api/activity/teams/start", post(team_controls::start))
        .route("/api/activity/teams/:id/stop", post(team_controls::stop))
        .route("/api/activity/teams/:id/resume", post(team_controls::resume))
        .route("/api/activity/teams/:id/artifacts/:task_id", get(team_controls::preview))
        .route("/api/activity/teams/:id/artifacts/:task_id/apply", post(team_controls::approve_apply))
        .route("/api/activity/teams/:id/artifacts/:task_id/reject", post(team_controls::reject))
        .route("/api/executions", get(executions::list))
        .route("/api/executions/active", get(executions::active_executions))
        .route("/api/executions/:id", get(executions::one))
        .route("/api/executions/:id/cancel", post(executions::cancel))
}
