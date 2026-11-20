//! Optional long-term memory and reusable skill knowledge. Core session
//! messages and the execution journal are intentionally not routed here.
use super::{enabled,Extension};
use crate::{engine::memory,state::AppState};
use serde_json::Value;

pub fn memory_context(
    state:&AppState,session_id:&str,agent_id:Option<&str>,
    query:&str,execution_id:Option<&str>,
)->(Option<String>,Vec<Value>){
    if !enabled(state,session_id,agent_id,Extension::Memory){
        return (None,Vec::new());
    }
    let scope=memory::MemoryScope::for_session(&state.store,session_id,agent_id);
    memory::context_block_with_manifest_at(state,&scope,query,execution_id)
}

pub fn memories_for_room(
    state:&AppState,session_id:&str,agent_id:Option<&str>,query:&str,limit:usize
)->Vec<Value>{
    if !enabled(state,session_id,agent_id,Extension::Memory){
        return Vec::new();
    }
    let scope=memory::MemoryScope::for_session(&state.store,session_id,agent_id);
    memory::retrieve(&state.store,&scope,query,limit)
}

pub fn visible_skills(state:&AppState,session_id:&str,agent_id:Option<&str>)->Vec<Value>{
    if !enabled(state,session_id,agent_id,Extension::Skills){
        return Vec::new();
    }
    let scope=memory::MemoryScope::for_session(&state.store,session_id,agent_id);
    state.store.all("skill").into_iter().filter(|record|
        record["enabled"]!=false && record["status"]!="draft"
            && memory::visible(record,&scope)
    ).collect()
}
pub fn has_visible_skills(state:&AppState,session_id:&str,agent_id:Option<&str>)->bool{
    !visible_skills(state,session_id,agent_id).is_empty()
}
pub fn record_context_use(state:&AppState,session_id:&str,agent_id:Option<&str>,items:&[Value]){
    if enabled(state,session_id,agent_id,Extension::Memory){
        memory::record_context_use(&state.store,items);
    }
}
