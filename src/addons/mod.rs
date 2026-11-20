//! Optional product systems. The native engine owns only execution; these
//! adapters provide knowledge, optimization, integrations and operations.
//! `crate::engine` re-exports are compatibility paths, not ownership.

// knowledge
#[path = "knowledge/agent_growth.rs"]
pub mod agent_growth;
#[path = "knowledge/distill.rs"]
pub mod distill;
#[path = "knowledge/experience.rs"]
pub mod experience;
#[path = "knowledge/learning.rs"]
pub mod learning;
#[path = "knowledge/memory.rs"]
pub mod memory;
#[path = "knowledge/principle.rs"]
pub mod principle;
#[path = "knowledge/reflect.rs"]
pub mod reflect;
#[path = "knowledge/room_knowledge.rs"]
pub mod room_knowledge;
#[path = "knowledge/skills.rs"]
pub mod skills;

// strategies
#[path = "strategies/adaptation/mod.rs"]
pub mod adaptation;
#[path = "strategies/adaptation_loop.rs"]
pub mod adaptation_loop;
#[path = "strategies/algorithm_eval.rs"]
pub mod algorithm_eval;
#[path = "strategies/algorithms.rs"]
pub mod algorithms;
#[path = "strategies/debate.rs"]
pub mod debate;
#[path = "strategies/decompose.rs"]
pub mod decompose;
#[path = "strategies/episode.rs"]
pub mod episode;
#[path = "strategies/improve.rs"]
pub mod improve;
#[path = "strategies/plan_quality.rs"]
pub mod plan_quality;
#[path = "strategies/policy_patch.rs"]
pub mod policy_patch;
#[path = "strategies/preflight_fixture.rs"]
pub mod preflight_fixture;
#[path = "strategies/procedure.rs"]
pub mod procedure;

// integrations
#[path = "integrations/attachments.rs"]
pub mod attachments;
#[path = "integrations/bridge.rs"]
pub mod bridge;
#[path = "integrations/browser.rs"]
pub mod browser;
#[path = "integrations/channel_adapter.rs"]
pub mod channel_adapter;
#[path = "integrations/cli_model.rs"]
pub mod cli_model;
#[path = "integrations/code_index.rs"]
pub mod code_index;
#[path = "integrations/codex_oauth.rs"]
pub mod codex_oauth;
#[path = "integrations/dev.rs"]
pub mod dev;
#[path = "integrations/discord.rs"]
pub mod discord;
#[path = "integrations/extensions/mod.rs"]
pub mod extensions;
#[path = "integrations/gateway.rs"]
pub mod gateway;
#[path = "integrations/mcp.rs"]
pub mod mcp;
#[path = "integrations/models.rs"]
pub mod models;
#[path = "integrations/oauth.rs"]
pub mod oauth;
#[path = "integrations/personas.rs"]
pub mod personas;
#[path = "integrations/room_state.rs"]
pub mod room_state;
#[path = "integrations/rooms.rs"]
pub mod rooms;
#[path = "integrations/telegram.rs"]
pub mod telegram;

// operations
#[path = "operations/backup_keys.rs"]
pub mod backup_keys;
#[path = "operations/maintenance.rs"]
pub mod maintenance;
#[path = "operations/remote_backup.rs"]
pub mod remote_backup;
#[path = "operations/vault_rotation.rs"]
pub mod vault_rotation;

