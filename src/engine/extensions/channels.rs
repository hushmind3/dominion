//! Messaging channel adapters are independent add-ons. They share the
//! persisted conversation/room journal but never own model execution.
use super::{globally_enabled,Extension};
use crate::{engine::{channel_adapter,discord,telegram},state::AppState};

/// Starts the adapter supervisors, not any external channel connection.
/// Every driver consults the policy on each poll/flush; switching the
/// extension off stops external traffic without restarting the Rust core.
pub fn start_inbound(state:AppState){
    discord::start(state.clone());
    telegram::start(state);
}
pub fn start_outbound(state:AppState){
    channel_adapter::start(state);
}
pub fn enabled(state:&AppState)->bool{
    globally_enabled(state,Extension::Channels)
}
