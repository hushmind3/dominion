//! External applications are independent of the product and use the SDK.
//! This contract contains no model, tool, network, database or process runner.
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION:u32=1;
pub const SDK_SOURCE_KIND:&str="extension";

pub fn valid_id(id:&str)->bool {
    !id.is_empty() && id.len()<=64 && id.as_bytes()[0].is_ascii_lowercase()
        && id.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'||b==b'.')
        && !id.ends_with('.') && !id.contains("..")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Manifest {
    pub schema_version:u32,
    pub id:String,
    pub name:String,
    pub version:String,
    #[serde(default)] pub description:String,
}
impl Manifest {
    pub fn validate(&self)->Result<(),String>{
        if self.schema_version!=SCHEMA_VERSION{return Err("Unsupported extension schema version".into())}
        if !valid_id(&self.id){return Err("Invalid extension ID (lowercase letters, digits, dots, hyphens; max 64)".into())}
        let plain=|s:&str,max:usize|!s.trim().is_empty()&&s.chars().count()<=max&&!s.chars().any(char::is_control);
        if !plain(&self.name,120){return Err("Invalid extension name".into())}
        if self.description.chars().count()>2000||self.description.chars().any(|c|c.is_control()&&c!='\n'){return Err("Invalid extension description".into())}
        if self.version.len()>64 || !self.version.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'.'|b'-'|b'+'))
            ||self.version.split(['-','+']).next().is_none_or(|v|{
                let parts=v.split('.').collect::<Vec<_>>();
                parts.len()!=3||parts.iter().any(|s|s.is_empty()||!s.bytes().all(|b|b.is_ascii_digit()))
            }) {return Err("Version must use major.minor.patch with optional suffix".into())}
        Ok(())
    }
}

#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Registration {
    pub id:String,
    pub manifest:Manifest,
    pub enabled:bool,
}
impl Registration {
    pub fn new(manifest:Manifest)->Result<Self,String>{
        manifest.validate()?;
        Ok(Self{id:manifest.id.clone(),manifest,enabled:false})
    }
    /// Registration is not authentication. The host still enforces owner
    /// admission, budgets, approvals and the existing execution journal.
    pub fn admit(&self,id:&str)->Result<(),String>{
        self.manifest.validate()?;
        if self.id!=id||self.manifest.id!=id||!self.enabled{return Err("Extension is disabled or registration is invalid".into())}
        Ok(())
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn manifest()->Manifest{Manifest{schema_version:1,id:"example.assistant".into(),name:"Assistant".into(),version:"1.0.0".into(),description:String::new()}}
    #[test]fn explicit_activation_and_identity(){
        let mut row=Registration::new(manifest()).unwrap();
        assert!(!row.enabled);
        assert!(row.admit("example.assistant").is_err());
        row.enabled=true;
        assert!(row.admit("example.assistant").is_ok());
        assert!(row.admit("other").is_err());
    }
    #[test]fn reject_invalid_schema_names_and_versions(){
        for id in ["","../x","A","a/b","a..b","a."]{
            let mut m=manifest();m.id=id.into();assert!(m.validate().is_err(),"{id}");
        }
        let mut m=manifest();m.schema_version=2;assert!(m.validate().is_err());
        let mut m=manifest();m.version="latest".into();assert!(m.validate().is_err());
    }
}
