use super::super::{algorithms, benchmark, experiments, system};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/benchmark/strategies", post(benchmark::seed_strategy))
        .route(
            "/api/benchmark/experiments",
            post(benchmark::record_experiment),
        )
        .route("/api/benchmark/policies", post(benchmark::register_policy))
        .route(
            "/api/benchmark/policies/experiment",
            post(benchmark::policy_experiment),
        )
        .route(
            "/api/benchmark/policies/state",
            post(benchmark::policy_state),
        )
        .route(
            "/api/benchmark/policy-search",
            post(benchmark::policy_search),
        )
        .route(
            "/api/benchmark/policy-patches",
            post(benchmark::register_patch),
        )
        .route(
            "/api/benchmark/policy-patches/experiment",
            post(benchmark::patch_experiment),
        )
        .route("/api/benchmark/identity", get(system::benchmark_identity))
        .route("/api/experiment-requests", get(system::experiment_requests))
        .route(
            "/api/algorithms",
            get(algorithms::list).post(algorithms::register),
        )
        .route("/api/algorithms/evaluate", get(algorithms::evaluate))
        .route("/api/algorithms/evaluations", get(algorithms::evaluations))
        .route("/api/algorithms/generate", post(algorithms::generate))
        .route("/api/algorithms/:id/activate", post(algorithms::activate))
        .route("/api/algorithms/:id/disable", post(algorithms::disable))
        .route("/api/experiment-requests/claim", post(experiments::claim))
        .route(
            "/api/experiment-requests/release-runner",
            post(experiments::release_runner),
        )
        .route(
            "/api/experiment-requests/:id/complete",
            post(experiments::complete),
        )
        .route(
            "/api/experiment-requests/:id/release",
            post(experiments::release),
        )
        .route(
            "/api/benchmark/experiment-requests",
            post(experiments::enqueue),
        )
        .route("/api/policies", get(system::policies))
        .route("/api/scheduler-runs", get(system::scheduler_runs))
        .route("/api/policy-searches", get(system::policy_searches))
        .route("/api/policy-patches", get(system::policy_patches))
}
