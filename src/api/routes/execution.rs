use super::super::{
    approvals, executions, jobs, remote_artifacts, remote_registry, remote_workers, sdk, tasks,
};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/sdk/runs", get(sdk::list).post(sdk::start))
        .route("/api/sdk/runs/source/:kind", get(sdk::by_source))
        .route("/api/sdk/runs/:id", get(sdk::one))
        .route("/api/sdk/runs/:id/resume", post(sdk::resume))
        .route("/api/sdk/runs/:id/approve", post(sdk::approve))
        .route("/api/sdk/runs/:id/reconcile", post(sdk::reconcile))
        .route("/api/sdk/runs/:id/cancel", post(sdk::cancel))
        .route("/api/sdk/runs/:id/steer", post(sdk::steer))
        .route(
            "/api/remote/jobs",
            get(remote_workers::list).post(remote_workers::create),
        )
        .route("/api/remote/jobs/:id", get(remote_workers::one))
        .route("/api/remote/jobs/:id/review", post(remote_workers::review))
        .route("/api/remote/jobs/:id/cancel", post(remote_workers::cancel))
        .route("/api/remote/jobs/:id/artifact", get(remote_artifacts::get))
        .route(
            "/api/remote/jobs/:id/artifact/upload",
            post(remote_artifacts::upload),
        )
        .route(
            "/api/remote/jobs/:id/artifact/review",
            post(remote_artifacts::accept),
        )
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
        .route("/api/remote/claim", post(remote_workers::claim))
        .route("/api/remote/reconcile", post(remote_workers::reconcile))
        .route(
            "/api/remote/jobs/:id/heartbeat",
            post(remote_workers::heartbeat),
        )
        .route("/api/remote/jobs/:id/finish", post(remote_workers::finish))
        .route("/api/jobs", get(jobs::list).post(jobs::create))
        .route("/api/jobs/:id", get(jobs::one))
        .route("/api/jobs/:id/team-steps", get(jobs::team_steps))
        .route("/api/jobs/:id/cancel", post(jobs::cancel))
        .route("/api/jobs/:id/steer", post(jobs::steer))
        .route("/api/jobs/:id/resume-external", post(jobs::resume_external))
        .route("/api/tasks", get(tasks::list))
        .route("/api/tasks/:id", get(tasks::one))
        .route(
            "/api/policy",
            get(approvals::get_policy).post(approvals::save_policy),
        )
        .route(
            "/api/executions/:id/operations",
            get(executions::execution_operations),
        )
        .route(
            "/api/approvals",
            get(approvals::list_approvals).post(approvals::resolve_approval),
        )
        .route("/api/executions", get(executions::list))
        .route("/api/executions/active", get(executions::active_executions))
        .route("/api/executions/:id", get(executions::one))
        .route("/api/executions/:id/events", get(executions::run_events))
        .route(
            "/api/executions/:id/budget/reconcile",
            post(executions::reconcile_budget),
        )
        .route("/api/executions/:id/cancel", post(executions::cancel))
}
