//! Optional capability boundary for the DOMINION product host.
//!
//! The Rust execution kernel, durable transcript, model routing and approval
//! journal have no dependency on these extensions. The application host alone
//! decides which add-ons contribute tools, evidence or background transports.
//! Default-on maintains existing configurations; any explicit false at global,
//! room, agent or session level is an attenuation, never an elevation.
use crate::state::AppState;
use serde_json::{json,Value};

pub mod channels;
pub mod knowledge;
pub mod tools;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Extension {
    Mcp,
    Memory,
    Skills,
    Channels,
}
impl Extension {
    pub const ALL: [Self;4]=[Self::Mcp,Self::Memory,Self::Skills,Self::Channels];
    pub const fn key(self)->&'static str {
        match self {
            Self::Mcp=>"mcp", Self::Memory=>"memory",
            Self::Skills=>"skills",Self::Channels=>"channels",
        }
    }
    pub fn from_tool(name:&str)->Option<Self>{
        if name.starts_with("mcp__"){return Some(Self::Mcp)}
        match name {
            "memory_search"|"memory_store"=>Some(Self::Memory),
            "skill_view"|"skill_propose"=>Some(Self::Skills),
            _=>None,
        }
    }
}

/// Validate an entire replaceable extension policy. Unknown keys and non-bool
/// values are rejected before persistence; omitted keys inherit enabled.
pub fn validate_policy(policy:&Value)->Result<Value,String>{
    let object=policy.as_object().ok_or("부가기능 설정은 JSON 객체여야 합니다.")?;
    let mut normalized=serde_json::Map::new();
    for (name,value) in object {
        if !Extension::ALL.iter().any(|ext|ext.key()==name) {
            return Err(format!("등록되지 않은 부가기능입니다: {name}"));
        }
        if !value.is_boolean(){
            return Err(format!("부가기능 {name} 설정은 true/false여야 합니다."));
        }
        normalized.insert(name.clone(),value.clone());
    }
    Ok(Value::Object(normalized))
}

fn admits(record:&Value,extension:Extension)->bool{
    record["extensions"][extension.key()]!=false
}

/// The absence of configuration is backwards-compatible, but an explicit
/// denial at any layer cannot be overridden by a more local "true".
pub fn globally_enabled(state:&AppState,extension:Extension)->bool{
    state.store.get("extension_policy","default")
        .is_none_or(|record|admits(&record,extension))
}
pub fn enabled(
    state:&AppState,session_id:&str,agent_id:Option<&str>,extension:Extension,
)->bool{
    if !globally_enabled(state,extension){return false;}
    let session=state.store.get("session",session_id).unwrap_or(Value::Null);
    if !admits(&session,extension){return false;}
    if let Some(room_id)=session["roomId"].as_str(){
        if state.store.get("room",room_id)
            .is_some_and(|record|!admits(&record,extension)) {return false}
    }
    let agent_id=agent_id.or_else(||session["agentId"].as_str());
    if let Some(agent_id)=agent_id {
        if state.store.get("agent",agent_id)
            .is_some_and(|record|!admits(&record,extension)){return false}
    }
    true
}
pub fn tool_allowed(
    state:&AppState,session_id:&str,agent_id:Option<&str>,name:&str,
)->bool{
    Extension::from_tool(name)
        .is_none_or(|extension|enabled(state,session_id,agent_id,extension))
}
pub fn status(state:&AppState,session_id:Option<&str>,agent_id:Option<&str>)->Value{
    let mut policy=serde_json::Map::new();
    for extension in Extension::ALL {
        policy.insert(extension.key().into(),
            json!(session_id.map_or_else(
                ||globally_enabled(state,extension),
                |id|enabled(state,id,agent_id,extension)
            )));
    }
    Value::Object(policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn optional_extensions_cannot_escalate_disabled_global_or_room_authority(){
        let root=std::env::temp_dir().join(format!("dominion-addon-boundary-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("agent",&json!({"id":"a","extensions":{"skills":false}})).unwrap();
        state.store.put("room",&json!({"id":"r","extensions":{"memory":false}})).unwrap();
        state.store.put("session",&json!({"id":"s","agentId":"a","roomId":"r",
            "extensions":{"mcp":false,"memory":true,"skills":true}})).unwrap();
        assert!(!enabled(&state,"s",Some("a"),Extension::Mcp));
        assert!(!enabled(&state,"s",Some("a"),Extension::Memory));
        assert!(!enabled(&state,"s",Some("a"),Extension::Skills));
        assert!(enabled(&state,"s",Some("a"),Extension::Channels));
        assert!(!tool_allowed(&state,"s",Some("a"),"mcp__external__call"));
        assert!(!tool_allowed(&state,"s",Some("a"),"skill_view"));
        assert!(!tool_allowed(&state,"s",Some("a"),"memory_search"));
        assert!(tool_allowed(&state,"s",Some("a"),"workspace_read"));
        state.store.put("extension_policy",&json!({"id":"default",
            "extensions":{"channels":false}})).unwrap();
        assert!(!globally_enabled(&state,Extension::Channels));
        assert!(!enabled(&state,"s",Some("a"),Extension::Channels));
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn policy_rejects_unknown_or_untyped_capability_keys(){
        assert_eq!(validate_policy(&json!({"mcp":false,"memory":false})).unwrap(),
            json!({"mcp":false,"memory":false}));
        assert!(validate_policy(&json!({"mcp":"disabled"})).is_err());
        assert!(validate_policy(&json!({"unsupported":false})).is_err());
        assert!(validate_policy(&json!([])).is_err());
    }
}
