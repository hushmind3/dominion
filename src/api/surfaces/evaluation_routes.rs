//! Product-domain HTTP endpoints, outside the minimal turn API.
//! Shared root router applies auth, CORS, maintenance and fallback.
use super::super::*;
pub(super) fn routes()->Router<AppState>{
    Router::<AppState>::new()
        .route("/api/algorithms",get(algorithm_controls::list)
            .post(algorithm_controls::register))
        .route("/api/algorithms/:id/activate",
            post(algorithm_controls::activate))
        .route("/api/synergy",get(algorithm_controls::statistics))
        .route("/api/benchmark/strategies", post(benchmark::seed_strategy))
        .route("/api/benchmark/experiments", post(benchmark::record_experiment))
        .route("/api/benchmark/policies", post(benchmark::register_policy))
        .route(
            "/api/benchmark/policies/experiment",
            post(benchmark::policy_experiment),
        )
        .route(
            "/api/benchmark/policies/state",
            post(benchmark::policy_state),
        )
        .route("/api/benchmark/policy-search", post(benchmark::policy_search))
        .route("/api/benchmark/policy-patches", post(benchmark::register_patch))
        .route(
            "/api/benchmark/policy-patches/experiment",
            post(benchmark::patch_experiment),
        )
        .route("/api/benchmark/identity", get(system::benchmark_identity))
        .route("/api/evaluation/summary",get(system::evaluation_summary))
        .route("/api/experiment-requests", get(system::experiment_requests))
        .route("/api/experiment-requests/claim", post(experiments::claim))
        .route(
            "/api/experiment-requests/release-runner",
            post(experiments::release_runner),
        )
        .route("/api/experiment-requests/:id/complete", post(experiments::complete))
        .route("/api/experiment-requests/:id/release", post(experiments::release))
        .route(
            "/api/benchmark/experiment-requests",
            post(experiments::enqueue),
        )
        .route("/api/policies", get(system::policies))
        .route("/api/policy-searches", get(system::policy_searches))
        .route("/api/policy-patches", get(system::policy_patches))
        .route("/api/policy", get(approvals::get_policy).post(approvals::save_policy))
        .route("/api/improvements", get(improvements::list))
        .route("/api/experience", get(experience::list))
        .route("/api/transfer", get(experience::transfer))
        .route("/api/improvements/:id/:action", post(improvements::decide))
}
