//! Standalone transport context. No DOMINION host, harness, DB or model dependency.
use serde_json::Value;
use std::{collections::HashMap,path::{Path,PathBuf},sync::{Arc,RwLock}};
#[derive(Clone)]
pub struct Config{pub port:u16,pub data_dir:PathBuf,pub web_dir:PathBuf,pub workspace:PathBuf}
impl Config{pub fn db_path(&self)->PathBuf{self.data_dir.join("unused-db")}pub fn key_path(&self)->PathBuf{self.data_dir.join("unused-key")}}
#[derive(Clone,Default)]
pub struct Store(Arc<RwLock<HashMap<(String,String),Value>>>);
impl Store{
 pub fn open(_: &Path)->Result<Self,String>{Ok(Self::default())}
 pub fn get(&self,kind:&str,id:&str)->Option<Value>{self.0.read().unwrap().get(&(kind.into(),id.into())).cloned()}
 pub fn all(&self,kind:&str)->Vec<Value>{let mut rows=self.0.read().unwrap().iter().filter(|((k,_),_)|k==kind).map(|(_,v)|v.clone()).collect::<Vec<_>>();rows.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str()));rows}
 pub fn put(&self,kind:&str,value:&Value)->Option<Value>{let mut row=value.clone();if !row.is_object(){return None}let id=row["id"].as_str().filter(|s|!s.is_empty()).map(str::to_owned).unwrap_or_else(||uuid::Uuid::new_v4().to_string());row["id"]=serde_json::json!(id);self.0.write().unwrap().insert((kind.into(),id),row.clone());Some(row)}
 pub fn audit(&self,_:&str,_:Value){} // never log raw remote output or credentials
}
#[derive(Clone,Default)]
pub struct Vault(Arc<RwLock<HashMap<String,String>>>);
impl Vault{pub fn open(_: &Path)->Self{Self::default()}}
#[derive(Clone,Default)]
pub struct IntegrationState{pub(crate) mcp_sessions:Arc<crate::mcp::SessionPool>}
#[derive(Clone)]
pub struct AppState{pub config:Config,pub store:Store,pub instance_id:String,pub integrations:IntegrationState,vault:Vault}
impl AppState{
 pub fn new(config:Config,store:Store,vault:Vault)->Self{Self{config,store,instance_id:uuid::Uuid::new_v4().to_string(),integrations:IntegrationState::default(),vault}}
 pub fn read_secret(&self,key:&str)->Option<String>{self.vault.0.read().unwrap().get(key).cloned()}
 pub fn save_secret(&self,key:&str,value:&str){self.vault.0.write().unwrap().insert(key.into(),value.into());}
}
