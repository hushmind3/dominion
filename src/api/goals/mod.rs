//! HTTP handlers for durable goal lifecycle, safe owner recovery and
//! isolated/shared workspace changes. Implementation split by responsibility.
use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "recovery.rs"]
mod recovery;
#[path = "worktree.rs"]
mod worktree;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use lifecycle::{list_goals,get_goal,delete_goal,create_goal,cancel_goal};
pub use recovery::{resume_goal,steer_goal};
pub use worktree::{diff_goal,export_goal_patch,rollback_goal,apply_goal,shared_changes,shared_undo};
