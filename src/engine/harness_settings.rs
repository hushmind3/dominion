//! Owner-configured, engine-enforced harness budgets. This is an execution
//! control plane, not another model runtime. The user may constrain schema and
//! discovery spending; permission/approval and execution ownership stay native.
use crate::state::AppState;
use serde::{Deserialize,Serialize};
use serde_json::Value;

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",default)]
pub struct HarnessSettings {
    pub id:String,
    pub dynamic_tool_discovery:bool,
    pub tool_schema_budget_bytes:usize,
    pub max_discovery_searches:usize,
    pub max_revealed_tools:usize,
}
impl Default for HarnessSettings {
    fn default()->Self {
        Self {
            id:"global".into(),
            dynamic_tool_discovery:true,
            tool_schema_budget_bytes:60_000,
            max_discovery_searches:4,
            max_revealed_tools:20,
        }
    }
}
impl HarnessSettings {
    /// Reject ambiguous owner inputs rather than silently clamp them.
    pub fn parse_owner(value:&Value)->Result<Self,String>{
        let object=value.as_object().ok_or("Harness settings must be a JSON object")?;
        for key in object.keys(){
            if !matches!(key.as_str(),"id"|"dynamicToolDiscovery"|
                "toolSchemaBudgetBytes"|"maxDiscoverySearches"|"maxRevealedTools"){
                return Err(format!("Unknown harness setting: {key}"));
            }
        }
        let settings:Self=serde_json::from_value(value.clone())
            .map_err(|error|format!("Invalid harness setting: {error}"))?;
        settings.validate()?;
        Ok(settings)
    }
    pub fn validate(&self)->Result<(),String>{
        if self.id!="global" {return Err("Harness settings can only target global".into());}
        if !(4_000..=60_000).contains(&self.tool_schema_budget_bytes){
            return Err("toolSchemaBudgetBytes must be between 4000 and 60000".into());
        }
        if !(1..=12).contains(&self.max_discovery_searches){
            return Err("maxDiscoverySearches must be between 1 and 12".into());
        }
        if !(1..=20).contains(&self.max_revealed_tools){
            return Err("maxRevealedTools must be between 1 and 20".into());
        }
        Ok(())
    }
    pub fn schema_budget(&self,provider_budget:usize)->usize{
        provider_budget.min(self.tool_schema_budget_bytes)
    }
}
/// Active turns load once; an owner change affects the next turn, never a
/// half-finished tool batch. Invalid persisted values fall back to defaults.
pub fn current(state:&AppState)->HarnessSettings{
    state.store.get("harness_settings","global")
        .and_then(|row|serde_json::from_value::<HarnessSettings>(row).ok())
        .filter(|settings|settings.validate().is_ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn owner_can_constrain_discovery_without_granting_tool_permissions(){
        let config=HarnessSettings::parse_owner(&json!({
            "dynamicToolDiscovery":false,
            "toolSchemaBudgetBytes":8000,
            "maxDiscoverySearches":2,
            "maxRevealedTools":3
        })).unwrap();
        assert!(!config.dynamic_tool_discovery);
        assert_eq!(config.schema_budget(32_000),8000);
        assert_eq!(config.schema_budget(5_000),5_000);
        assert_eq!(config.max_discovery_searches,2);
        assert_eq!(config.id,"global");
        let roundtrip=serde_json::to_value(config).unwrap();
        assert_eq!(roundtrip["maxRevealedTools"],3);
    }

    #[test]
    fn invalid_or_foreign_settings_never_disable_the_budget_limit(){
        for value in [
            json!({"toolSchemaBudgetBytes":100000}),
            json!({"maxRevealedTools":0}),
            json!({"maxDiscoverySearches":0}),
            json!({"id":"another-user"}),
            json!({"dynamicToolDiscovery":"yes"}),
            json!({"ignoreApproval":true}),
            json!(null)
        ]{
            assert!(HarnessSettings::parse_owner(&value).is_err(),"{value}");
        }
        assert_eq!(HarnessSettings::default().schema_budget(90_000),60_000);
    }
}
