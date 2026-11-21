//! Minimal natural-language execution kernel with external product services.
//! Only core/ owns the conversational execution path. All other Rust
//! capabilities are source-isolated; historical engine:: paths stay stable.
pub mod addons;
pub use dominion_turn_core as core;
pub use addons::{browser,bridge,channel_adapter,discord,github_work,google_work,mcp,telegram};
pub use core::TurnStats;
#[path = "services/automation/turn_quiet.rs"]
mod turn_quiet;
pub use turn_quiet::{run_quiet_outcome,run_quiet_with_stats,run_quiet};
// Optional services preserve the public engine API without living in core.
#[path = "services/automation/turn_resume.rs"]
mod turn_resume;
pub use turn_resume::run_quiet_resume;
#[path = "services/conversation/room_worker.rs"]
mod room_worker;
pub use room_worker::run_room_quiet;
#[cfg(test)]
use core::collect_tool_event;

// Natural language, execution admission and evidence
#[path = "services/operations/execution_scope.rs"]
pub mod execution_scope;
pub use dominion_agent_kernel as agent_kernel;
#[path = "services/tools/workspace_scope.rs"]
pub mod workspace_scope;
#[path = "services/operations/effect.rs"]
pub mod effect;
#[path = "services/conversation/chat/mod.rs"]
pub mod chat;
#[path = "services/decision/verification.rs"]
pub mod verification;
#[path = "services/operations/resource_lock.rs"]
pub mod resource_lock;
#[path = "services/operations/run_state.rs"]
pub mod run_state;

// External service: providers
#[path = "services/providers/gateway.rs"]
pub mod gateway;
#[path = "services/providers/cli_accounts.rs"]
pub mod cli_accounts;
#[path = "services/providers/model_cost.rs"]
pub mod model_cost;
#[path = "services/providers/cli_model.rs"]
pub mod cli_model;
#[path = "services/providers/llm.rs"]
pub mod llm;
#[path = "services/providers/models.rs"]
pub mod models;
#[path = "services/providers/codex_oauth.rs"]
pub mod codex_oauth;
#[path = "services/providers/oauth.rs"]
pub mod oauth;
#[path = "services/providers/router.rs"]
pub mod router;
#[path = "services/providers/route_calibration.rs"]
pub mod route_calibration;

// External service: conversation
#[path = "services/conversation/episode.rs"]
pub mod episode;
#[path = "services/conversation/experience.rs"]
pub mod experience;
#[path = "services/conversation/memory.rs"]
pub mod memory;
#[path = "services/conversation/personas.rs"]
pub mod personas;
#[path = "services/conversation/rooms.rs"]
pub mod rooms;
#[path = "services/conversation/room_knowledge.rs"]
pub mod room_knowledge;
#[path = "services/conversation/room_state.rs"]
pub mod room_state;
#[path = "services/conversation/room_workboard.rs"]
pub mod room_workboard;
#[path = "services/conversation/skills.rs"]
pub mod skills;

// External service: automation
#[path = "services/automation/autonomy.rs"]
pub mod autonomy;
#[path = "services/automation/recovery.rs"]
pub mod recovery;
#[path = "services/automation/jobs.rs"]
pub mod jobs;
#[path = "services/automation/lease.rs"]
pub mod lease;
#[path = "services/automation/maintenance.rs"]
pub mod maintenance;
#[path = "services/automation/wake.rs"]
pub mod wake;

// External service: tools
#[path = "services/tools/attachments.rs"]
pub mod attachments;
#[path = "services/tools/code_index.rs"]
pub mod code_index;
#[path = "services/tools/tools.rs"]
pub mod tools;

// External service: decision
#[path = "services/decision/adaptation_loop.rs"]
pub mod adaptation_loop;
#[path = "services/decision/distill.rs"]
pub mod distill;
#[path = "services/decision/agent_growth.rs"]
pub mod agent_growth;
#[path = "services/decision/algorithms.rs"]
pub mod algorithms;
#[path = "services/decision/turn_policy.rs"]
pub mod turn_policy;
#[path = "services/decision/decompose.rs"]
pub mod decompose;
#[path = "services/decision/debate.rs"]
pub mod debate;
#[path = "services/decision/adaptation/mod.rs"]
pub mod adaptation;
#[path = "services/decision/procedure.rs"]
pub mod procedure;
#[path = "services/decision/program.rs"]
pub mod program;
#[path = "services/decision/task_team_work.rs"]
pub mod task_team_work;
#[path = "services/decision/reflect.rs"]
pub mod reflect;
#[path = "services/decision/evaluation.rs"]
pub mod evaluation;
#[path = "services/decision/improve.rs"]
pub mod improve;
#[path = "services/decision/reasoning.rs"]
pub mod reasoning;
#[path = "services/decision/learning.rs"]
pub mod learning;
#[path = "services/decision/plan_quality.rs"]
pub mod plan_quality;
#[path = "services/decision/policy.rs"]
pub mod policy;
#[path = "services/decision/policy_patch.rs"]
pub mod policy_patch;
#[path = "services/decision/preflight_fixture.rs"]
pub mod preflight_fixture;
#[path = "services/decision/principle.rs"]
pub mod principle;

// External service: operations
#[path = "services/operations/backup_keys.rs"]
pub mod backup_keys;
#[path = "services/operations/compact.rs"]
pub mod compact;
#[path = "services/operations/dev.rs"]
pub mod dev;
#[path = "services/operations/remote_backup.rs"]
pub mod remote_backup;
#[path = "services/operations/unsafe_audit.rs"]
pub mod unsafe_audit;
#[path = "services/operations/vault_rotation.rs"]
pub mod vault_rotation;

#[cfg(test)]
#[path = "tests.rs"]
mod outcome_tests;
