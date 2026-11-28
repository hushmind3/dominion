//! Agent loop algorithm; the product gateway composes it with dominion-core.
pub mod engine;
pub mod context;

pub mod decompose;
pub mod plan_quality;
pub mod planning;
pub mod reasoning;

pub mod memory_rank;
pub mod experience_rank;
pub mod learning_contract;

/// Evidence-derived repair guidance ported from branch3.
pub mod failure_guidance;
pub mod tool_history;
