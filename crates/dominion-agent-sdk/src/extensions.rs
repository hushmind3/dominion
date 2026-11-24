//! Runtime registration of trusted host adapters. No native binary loading,
//! package installation, filesystem discovery or ambient tool permissions.
use crate::{AgentLoop, ModelPort, PortRunResult, RunOptions, ToolPort, ToolRequest};
use serde_json::{json,Value};
use std::{collections::{BTreeMap,BTreeSet},future::Future,pin::Pin};

/// An extension supplies actual tool execution, not an alternate agent loop.
/// The host must enforce approvals, argument validation and effect receipts
/// appropriate to each concrete implementation.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output=Result<Value,String>> + 'a>>;
pub trait ExtensionTool {
    fn call<'a>(&'a mut self, arguments:&'a Value)->ToolFuture<'a>;
}

pub struct ToolRegistration {
    pub name:String,
    pub description:String,
    pub parameters:Value,
    pub handler:Box<dyn ExtensionTool>,
}

struct Entry {
    plugin:String,
    schema:Value,
    handler:Box<dyn ExtensionTool>,
}

#[derive(Default)]
pub struct ExtensionRegistry {
    entries:BTreeMap<String,Entry>,
    plugins:BTreeSet<String>,
}

fn valid_segment(name:&str)->bool {
    !name.is_empty() && name.len()<=28 &&
        name.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_')
        && !name.starts_with('_') && !name.ends_with('_') && !name.contains("__")
}
fn qualified(plugin:&str,tool:&str)->String { format!("ext__{plugin}__{tool}") }

impl ExtensionRegistry {
    pub fn new()->Self {Self::default()}

    /// Atomic registration. No partial catalog if one definition is invalid.
    /// Tool names are namespaced and stable; duplicate identities are refused.
    /// The host, not the model, controls registration and replacement.
    pub fn register(&mut self,plugin:&str,tools:Vec<ToolRegistration>)->Result<(),String>{
        if !valid_segment(plugin) || self.plugins.contains(plugin) || tools.is_empty() {
            return Err("invalid or duplicate extension".into());
        }
        if tools.len()>128 || self.entries.len().saturating_add(tools.len())>1024 {
            return Err("extension tool catalog limit exceeded".into());
        }
        let mut pending=BTreeMap::new();
        for tool in tools {
            if !valid_segment(&tool.name) || tool.description.len()>8192 ||
                !tool.parameters.is_object() || tool.parameters["type"]!="object"
                || tool.parameters.to_string().len()>65536 {
                return Err("invalid extension tool definition".into());
            }
            let name=qualified(plugin,&tool.name);
            if self.entries.contains_key(&name)||pending.contains_key(&name){
                return Err("duplicate extension tool name".into());
            }
            pending.insert(name.clone(),Entry{
                plugin:plugin.into(),
                schema:json!({"type":"function","function":{
                    "name":name,"description":tool.description,"parameters":tool.parameters
                }}),
                handler:tool.handler,
            });
        }
        self.entries.extend(pending);
        self.plugins.insert(plugin.into());
        Ok(())
    }
    pub fn unregister(&mut self,plugin:&str)->bool{
        if !self.plugins.remove(plugin){return false;}
        self.entries.retain(|_,entry|entry.plugin!=plugin);
        true
    }
    /// Host-visible names for an explicit owner/host permission selection.
    /// This is not the model's tool catalog.
    pub fn names(&self)->Vec<String>{self.entries.keys().cloned().collect()}

    /// Only exact currently registered names explicitly granted by the host
    /// are exposed. Unknown grants fail closed; no wildcard or implicit grants.
    /// A run borrows the registry mutably, so its tool identities and handlers
    /// cannot be swapped or removed mid-execution by another caller.
    pub async fn execute<A:AgentLoop,M:ModelPort>(
        &mut self, algorithm:&mut A, model:&mut M, history:&mut Vec<Value>,
        grants:&[String], options:RunOptions,
    )->Result<PortRunResult,String>{
        let allowed=grants.iter().cloned().collect::<BTreeSet<_>>();
        if allowed.iter().any(|name|!self.entries.contains_key(name)){
            return Err("extension grant refers to an unavailable tool".into());
        }
        let catalog=allowed.iter().filter_map(|name|self.entries.get(name))
            .map(|entry|entry.schema.clone()).collect::<Vec<_>>();
        let mut port=RegistryPort{registry:self,allowed};
        crate::execute(algorithm,model,&mut port,history,&catalog,options).await
    }
    /// Same admission and registry as execute, with optional shared context
    /// compaction. Tool effects and original receipts remain host-owned.
    pub async fn execute_compacted<A:AgentLoop,M:ModelPort>(
        &mut self, algorithm:&mut A, model:&mut M, history:&mut Vec<Value>,
        grants:&[String],options:RunOptions,tool_trace_bytes:usize,
    )->Result<PortRunResult,String>{
        let allowed=grants.iter().cloned().collect::<BTreeSet<_>>();
        if allowed.iter().any(|name|!self.entries.contains_key(name)){
            return Err("extension grant refers to an unavailable tool".into());
        }
        let catalog=allowed.iter().filter_map(|name|self.entries.get(name))
            .map(|entry|entry.schema.clone()).collect::<Vec<_>>();
        let mut port=RegistryPort{registry:self,allowed};
        crate::execute_compacted(algorithm,model,&mut port,history,&catalog,options,tool_trace_bytes).await
    }
}
struct RegistryPort<'a>{
    registry:&'a mut ExtensionRegistry,
    allowed:BTreeSet<String>,
}
impl ToolPort for RegistryPort<'_>{
    async fn dispatch(&mut self,request:&ToolRequest)->Result<Value,String>{
        if !self.allowed.contains(&request.name){
            return Err("extension tool not granted".into());
        }
        let entry=self.registry.entries.get_mut(&request.name)
            .ok_or_else(||"extension tool no longer registered".to_string())?;
        if !request.arguments.is_object(){
            return Ok(json!({"status":"not_dispatched","dispatched":false,
                "error":"extension arguments must be a JSON object"}));
        }
        entry.handler.call(&request.arguments).await
    }
}
