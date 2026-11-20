//! Legacy names for internal product code during transition.
//! This module does NOT load optional features into the SDK kernel.
pub(crate) use crate::host::execution::*;
pub(crate) use crate::features::cognition::{adaptation_loop, distill, agent_growth, algorithms, compact, decompose, episode, experience, procedure, reflect, improve, learning, memory, plan_quality, principle, adaptation};
pub(crate) use crate::features::collaboration::{debate, personas, rooms, room_knowledge, room_state};
pub(crate) use crate::features::providers::{native_model, native_codex_ws, native_cache, native_routing, native_stream, gateway, cli_model, llm, models, codex_oauth, router, reasoning};
pub(crate) use crate::features::platform::{attachments, backup_keys, dev, code_index, remote_backup, maintenance, policy, preflight_fixture, vault_rotation, wake};
pub(crate) use crate::features::forensics::{native_replay, native_adjudication};
pub(crate) use crate::conversation::chat;
pub(crate) use crate::integrations::local_tools as tools;
