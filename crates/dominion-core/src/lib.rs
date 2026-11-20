//! Native Rust execution harness. The product supplies SQLite, model and tool adapters.
pub mod domain;
pub mod journal;
pub mod tool_schema;
pub mod policy;
pub mod hosted;
pub mod runtime_port;
pub mod strategy;
pub mod budget;
mod mcp {
    pub fn request_identity(run:uuid::Uuid,call:&crate::domain::ToolCall)->String {
        use sha2::{Digest,Sha256};
        format!("dominion-{:x}",Sha256::digest(format!("{run}:{}:{}:{}",call.id,call.name,call.arguments)))
    }
}

pub mod delegation;
