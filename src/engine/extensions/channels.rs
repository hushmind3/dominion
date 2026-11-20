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
/// A configured connection can be muted by its room/agent policy without
/// disrupting any other messenger or the core session ledger.
pub fn enabled_binding(state:&AppState,config:&serde_json::Value)->bool{
    if !enabled(state) || config["enabled"]!=true {return false;}
    if let Some(room)=config["roomId"].as_str(){
        if state.store.get("room",room).is_some_and(|record|
            record["extensions"]["channels"]==false) {return false;}
    }
    if let Some(agent)=config["agentId"].as_str(){
        if state.store.get("agent",agent).is_some_and(|record|
            record["extensions"]["channels"]==false) {return false;}
    }
    true
}
