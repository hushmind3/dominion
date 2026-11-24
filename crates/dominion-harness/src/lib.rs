//! Execution orchestration only. Decisions, transport, storage and effect
//! permissions remain behind core Host contracts.
pub mod engine;
pub use engine::Engine;
