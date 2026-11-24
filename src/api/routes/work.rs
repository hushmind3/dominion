//! HTTP routes for work. Handler behavior is unchanged.
use crate::api::{
    executions, jobs, native_replay, remote_artifacts, remote_jobs, remote_registry,
    remote_workers, runs, sdk, sdk_runs, system, task_teams, tasks,
};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/run-wakes", get(system::run_wakes))
        .route("/api/jobs", get(jobs::list).post(jobs::create))
        .route("/api/jobs/:id", get(jobs::one))
        .route("/api/jobs/:id/cancel", post(jobs::cancel))
        .route("/api/jobs/:id/directive", post(jobs::directive))
        .route("/api/jobs/:id/steer", post(jobs::steer))
        .route("/api/jobs/:id/team-steps", get(jobs::team_steps))
        .route("/api/harness/runs", get(sdk::list).post(sdk::start))
        .route("/api/harness/runs/:id", get(sdk::one))
        .route("/api/harness/runs/:id/approve", post(sdk::approve))
        .route("/api/harness/runs/:id/cancel", post(sdk::cancel))
        .route("/api/harness/runs/:id/resume", post(sdk::resume))
        .route("/api/harness/runs/:id/reconcile", post(sdk::reconcile))
        .route("/api/sdk/runs", get(sdk_runs::list).post(sdk_runs::create))
        .route("/api/sdk/runs/:id", get(sdk_runs::one))
        .route("/api/sdk/runs/:id/events", get(sdk_runs::events))
        .route("/api/sdk/runs/:id/receipts", get(sdk_runs::receipts))
        .route(
            "/api/sdk/runs/:id/artifact-preview",
            post(sdk_runs::artifact_preview),
        )
        .route("/api/sdk/runs/:id/replay", get(native_replay::replay))
        .route("/api/sdk/runs/:id/compare", post(native_replay::compare))
        .route(
            "/api/sdk/runs/:id/adjudicate",
            post(native_replay::adjudicate),
        )
        .route("/api/jobs/:id/adjudicate", post(native_replay::adjudicate))
        .route("/api/jobs/:id/replay", get(native_replay::replay))
        .route("/api/jobs/:id/compare", post(native_replay::compare))
        .route("/api/sdk/runs/:id/approve", post(sdk_runs::approve))
        .route("/api/sdk/runs/:id/deny", post(sdk_runs::deny))
        .route("/api/sdk/runs/:id/cancel", post(sdk_runs::cancel))
        .route("/api/sdk/runs/:id/resume", post(sdk_runs::resume))
        .route("/api/sdk/runs/:id/reconcile", post(sdk_runs::reconcile))
        .route("/api/sdk/runs/:id/steer", post(sdk_runs::steer))
        .route("/api/tasks", get(tasks::list))
        .route("/api/tasks/:id", get(tasks::one))
        .route("/api/runs/:id", get(runs::one))
        .route(
            "/api/remote/workers",
            get(remote_registry::list).post(remote_registry::enroll),
        )
        .route(
            "/api/remote/workers/:id/state",
            post(remote_registry::state),
        )
        .route(
            "/api/remote/workers/:id/rotate",
            post(remote_registry::rotate),
        )
        .route(
            "/api/remote/jobs",
            get(remote_jobs::list).post(remote_jobs::create),
        )
        .route("/api/remote/jobs/:id", get(remote_jobs::one))
        .route("/api/remote/jobs/:id/review", post(remote_jobs::review))
        .route("/api/remote/jobs/:id/cancel", post(remote_jobs::cancel))
        .route("/api/remote/jobs/:id/artifact", get(remote_artifacts::get))
        .route(
            "/api/remote/jobs/:id/artifact/upload",
            post(remote_artifacts::upload),
        )
        .route(
            "/api/remote/jobs/:id/artifact/review",
            post(remote_artifacts::accept),
        )
        .route("/api/remote/claim", post(remote_jobs::claim))
        .route("/api/remote/reconcile", post(remote_jobs::reconcile))
        .route(
            "/api/remote/jobs/:id/heartbeat",
            post(remote_jobs::heartbeat),
        )
        .route("/api/remote/jobs/:id/finish", post(remote_jobs::finish))
        .route(
            "/api/remote/tasks",
            get(remote_workers::owner_list).post(remote_workers::owner_create),
        )
        .route("/api/remote/tasks/:id", get(remote_workers::owner_one))
        .route(
            "/api/remote/tasks/:id/cancel",
            post(remote_workers::owner_cancel),
        )
        .route(
            "/api/task-teams",
            get(task_teams::list).post(task_teams::create),
        )
        .route("/api/task-teams/:id", get(task_teams::one))
        .route("/api/task-teams/:id/cancel", post(task_teams::cancel))
        .route("/api/task-teams/bridges", get(task_teams::bridges))
        .route("/api/task-teams/:id/retry", post(task_teams::retry_team))
        .route(
            "/api/task-teams/:id/worktrees/:task/preview",
            get(task_teams::worktree_preview),
        )
        .route(
            "/api/task-teams/:id/worktrees/:task/promote",
            post(task_teams::worktree_promote),
        )
        .route(
            "/api/executions/:id/operations",
            get(executions::execution_operations),
        )
        .route("/api/executions", get(executions::list))
        .route("/api/executions/active", get(executions::active_executions))
        .route("/api/executions/:id/events", get(executions::events))
        .route("/api/executions/:id", get(executions::one))
        .route("/api/executions/:id/cancel", post(executions::cancel))
}
