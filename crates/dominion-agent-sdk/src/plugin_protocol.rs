//! Language-neutral extension package and invocation contract. No MCP or app dependencies.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::collections::HashSet;
pub const PROTOCOL:&str="dominion.extension.v1";
pub const PREFIX:&str="ext__";

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Manifest {
    pub sdk_version:u32,pub id:String,pub name:String,pub version:String,pub description:String,
    pub runtime:Runtime,#[serde(default)] pub targets:Vec<String>,pub artifact:Artifact,pub tools:Vec<Tool>,
}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="lowercase")]
pub enum Runtime { Node,Python,Native }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {pub url:String,pub sha256:String}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tool {pub name:String,pub description:String,pub parameters:Value}
pub fn segment(s:&str)->bool{
    !s.is_empty()&&s.len()<=20&&s.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_')
    &&!s.starts_with('_')&&!s.ends_with('_')&&!s.contains("__")
}
fn digest(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn version(s:&str)->bool{s.len()<=64&&!s.is_empty()&&s.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'-'|b'+'))&&s.split('.').next().is_some_and(|x|!x.is_empty()&&x.bytes().all(|b|b.is_ascii_digit()))}
impl Manifest{
    pub fn validate(&self)->Result<(),String>{
        if self.sdk_version!=1||!segment(&self.id)||!version(&self.version)||self.name.trim().is_empty()
            ||self.name.len()>160||self.description.len()>4000||!digest(&self.artifact.sha256)
            ||self.artifact.url.len()>2048||self.tools.is_empty()||self.tools.len()>32||self.targets.len()>16{
            return Err("invalid extension manifest or unsupported SDK version".into());
        }
        if self.runtime==Runtime::Native&&self.targets.is_empty(){return Err("native artifact requires explicit platform targets".into());}
        if self.targets.iter().any(|x|x.len()>40||x.is_empty()||!x.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')){return Err("invalid extension target".into());}
        let mut seen=HashSet::new();
        for tool in &self.tools{
            if !segment(&tool.name)||!seen.insert(&tool.name)||tool.description.trim().is_empty()
            ||tool.description.len()>2000||tool.parameters["type"]!="object"||tool.parameters.to_string().len()>16384{
                return Err("invalid or duplicate extension tool definition".into());
            }
        }
        Ok(())
    }
    pub fn compatible(&self)->bool{self.targets.is_empty()||self.targets.contains(&format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH))}
    /// Names are revision-bound so updates cannot inherit stale approvals.
    pub fn qualified(&self,tool:&Tool,revision:&str)->String{format!("{PREFIX}{}__{}__{}",self.id,tool.name,revision)}
}
pub fn request(id:&str,tool:&str,arguments:&Value)->Result<Value,String>{
    if id.is_empty()||!segment(tool)||!arguments.is_object(){return Err("invalid extension invocation".into());}
    Ok(json!({"protocol":PROTOCOL,"id":id,"tool":tool,"arguments":arguments}))
}
pub fn response(id:&str,value:Value)->Result<Value,String>{
    if value["protocol"]!=PROTOCOL||value["id"].as_str()!=Some(id){return Err("extension response identity mismatch".into());}
    match value["ok"].as_bool(){
        Some(true) if value.get("result").is_some()&&value.get("error").is_none()=>Ok(json!({"success":true,"result":value["result"]})),
        Some(false)=>Err(value["error"].as_str().unwrap_or("extension returned failure").chars().take(1000).collect()),
        _=>Err("invalid extension response".into())
    }
}
#[cfg(test)]
mod tests{use super::*;
 #[test]fn verifies_response_identity(){
    assert!(response("x",json!({"protocol":PROTOCOL,"id":"y","ok":true,"result":1})).is_err());
    assert!(request("x","../bad",&json!({})).is_err());
    assert_eq!(response("x",json!({"protocol":PROTOCOL,"id":"x","ok":true,"result":1})).unwrap()["result"],1);
 }}
