//! Native goal harness: stable entry points with separated execution,
//! completion, role resolution, source navigation, and failure diagnosis.
use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_quiet_outcome};
pub use crate::engine::verification::{
    check_acceptance, prepare_acceptance_checks, validate_criteria,
};
use crate::engine::verification::{
    acceptance_gaps, baseline_checks, infer_criteria, string, successful_evidence,
};
#[cfg(test)]
use std::path::PathBuf;
use crate::state::AppState;
use serde_json::{json, Value};

#[path="goals/progress.rs"]
mod progress;
#[path="goals/diagnosis.rs"]
mod diagnosis;
#[path="goals/roles.rs"]
mod roles;
#[path="goals/completion.rs"]
mod completion;
#[path="goals/runner.rs"]
mod runner;
#[cfg(test)]
#[path="goals/tests.rs"]
mod tests;

use roles::{role_model,optional,tail,resolve_executor};
use completion::{patch_goal,finish};
pub use completion::parse_review;
pub use runner::run;
