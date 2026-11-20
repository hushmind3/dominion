//! External connectors and delivery adapters, never a parallel agent kernel.
//! They may provide tools or channel transport, but the native dispatcher
//! retains permission checks, effects, cancellation and execution receipts.
pub mod browser;
pub mod channel_adapter;
pub mod discord;
pub mod github_work;
pub mod google_work;
pub mod mcp;
pub mod telegram;
