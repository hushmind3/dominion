//! Product-supplied optional work around a native language -> model -> tool
//! -> observation turn. The native runner may call this boundary, but it does
//! not own memory, strategy selection or after-turn learning.
use crate::{state::AppState,engine::chat::TurnRequest};
use serde_json::Value;
use super::{algorithms,improve,memory,extensions};
use extensions::Extension;

pub fn strategies_enabled(
    state:&AppState,session_id:&str,agent_id:Option<&str>,
)->bool {
    extensions::enabled(state,session_id,agent_id,Extension::Strategies)
}

fn enabled_for_record(state:&AppState,record:&Value,extension:Extension)->bool{
    let Some(session_id)=record["sessionId"].as_str() else {return false;};
    extensions::enabled(state,session_id,record["agentId"].as_str(),extension)
}

pub fn strategies_enabled_for_execution(state:&AppState,record:&Value)->bool {
    enabled_for_record(state,record,Extension::Strategies)
}
pub fn memory_enabled_for_execution(state:&AppState,record:&Value)->bool {
    enabled_for_record(state,record,Extension::Memory)
}

fn enabled_for_run(state:&AppState,execution_id:Option<&str>)->bool {
    match execution_id.and_then(|id|state.store.get("execution",id)){
        Some(run)=>strategies_enabled_for_execution(state,&run),
        None=>extensions::globally_enabled(state,Extension::Strategies),
    }
}

/// A disabled strategy add-on cannot scan improvement history, inject a
/// strategy or create an algorithm-usage record for an ordinary native turn.
pub fn prepare_turn(
    state:&AppState,session_id:&str,agent_id:Option<&str>,execution_id:&str,
)->(Vec<String>,Vec<Value>) {
    if !strategies_enabled(state,session_id,agent_id){
        return (Vec::new(),Vec::new());
    }
    let scope=memory::MemoryScope::for_session(&state.store,session_id,agent_id);
    (improve::injected_ids_with_execution(state,&scope,Some(execution_id)),
     improve::active_values_with_execution(state,&scope,Some(execution_id)))
}

pub fn choose_for_agent(
    state:&AppState,kind:&str,agent_id:Option<&str>,execution_id:Option<&str>,
)->Option<Value> {
    if !enabled_for_run(state,execution_id){return None;}
    algorithms::choose_for_agent(state,kind,agent_id,execution_id)
}

pub fn choose_decomposition_for_agent(
    state:&AppState,agent_id:Option<&str>,execution_id:Option<&str>,request:&str,
)->Option<Value> {
    if !enabled_for_run(state,execution_id){return None;}
    algorithms::choose_decomposition_for_agent(state,agent_id,execution_id,request)
}

pub fn mark_signal(state:&AppState,execution_id:&str,kind:&str,ok:bool){
    if enabled_for_run(state,Some(execution_id)){
        algorithms::mark_signal(state,execution_id,kind,ok);
    }
}

pub fn mark_unrated(state:&AppState,execution_id:&str,kind:&str,reason:&str){
    if enabled_for_run(state,Some(execution_id)){
        algorithms::mark_unrated(state,execution_id,kind,reason);
    }
}

pub fn execution_updates(state:&AppState,execution:&Value)->Result<Vec<Value>,String>{
    if !strategies_enabled_for_execution(state,execution){
        return Ok(Vec::new());
    }
    improve::execution_updates(state,execution)
}

pub fn after_turn(
    state:&AppState,request:&TurnRequest,session_id:&str,execution_id:&str,
    result:&Result<Option<Value>,String>,
){
    extensions::knowledge::after_turn(state,request,session_id,execution_id,result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    use serde_json::json;

    #[test]
    fn optional_strategy_denial_preserves_native_execution_and_transcript(){
        let root=std::env::temp_dir().join(format!(
            "dominion-minimal-kernel-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"native",
            "extensions":{"strategies":false,"memory":false,"skills":false,"mcp":false}
        })).unwrap();
        state.store.put("execution",&json!({"id":"run","sessionId":"native",
            "status":"running"})).unwrap();
        assert!(!strategies_enabled(&state,"native",None));
        assert_eq!(prepare_turn(&state,"native",None,"run"),(vec![],vec![]));
        assert!(choose_for_agent(&state,"execution.budget",None,Some("run")).is_none());
        assert!(choose_decomposition_for_agent(&state,None,Some("run"),"test").is_none());
        assert!(execution_updates(&state,&json!({"id":"run","sessionId":"native"}))
            .unwrap().is_empty());
        assert!(crate::engine::tools::definitions().iter().any(|definition|
            definition["function"]["name"]=="workspace_read"));
        assert!(crate::engine::tools::definitions().iter().any(|definition|
            definition["function"]["name"]=="workspace_write"));
        assert!(state.store.get("execution","run").is_some());
        assert_eq!(state.store.all("algorithm_usage").len(),0);
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn strategy_denial_attentuates_per_agent_in_shared_room(){
        let root=std::env::temp_dir().join(format!(
            "dominion-strategy-scope-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("room",&json!({"id":"room"})).unwrap();
        state.store.put("agent",&json!({"id":"restricted","extensions":{"strategies":false}})).unwrap();
        state.store.put("agent",&json!({"id":"allowed"})).unwrap();
        state.store.put("session",&json!({"id":"room:room","roomId":"room"})).unwrap();
        assert!(!strategies_enabled(&state,"room:room",Some("restricted")));
        assert!(strategies_enabled(&state,"room:room",Some("allowed")));
        state.store.put("extension_policy",&json!({"id":"default",
            "extensions":{"strategies":false}})).unwrap();
        assert!(!strategies_enabled(&state,"room:room",Some("allowed")));
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
}
