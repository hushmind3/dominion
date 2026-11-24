//! Optional app adapter for the independent extension package manager.
use crate::state::AppState;
use dominion_agent_sdk::plugin_protocol::{Manifest,PREFIX};
use dominion_extension_manager as packages;
use serde_json::{json,Value};
use std::path::PathBuf;
static LIFECYCLE:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());
const KIND:&str="extension_package";
const SOURCES:&str="extension_registry";
fn root(state:&AppState)->PathBuf{state.config.data_dir.join("extensions")}
fn manifest(row:&Value)->Option<Manifest>{
    let m:Manifest=serde_json::from_value(row["manifest"].clone()).ok()?;
    m.validate().ok()?;Some(m)
}
pub fn sources(state:&AppState)->Vec<String>{
    let mut urls=state.store.get(SOURCES,"catalog").and_then(|v|serde_json::from_value::<Vec<String>>(v["urls"].clone()).ok()).unwrap_or_default();
    if let Ok(url)=std::env::var("DOMINION_EXTENSION_REGISTRY"){
        if packages::validate_url(&url).is_ok()&&!urls.contains(&url){urls.push(url)}
    }
    urls
}
pub async fn save_sources(state:&AppState,urls:Vec<String>)->Result<Value,String>{
    let _guard=LIFECYCLE.lock().await;
    if urls.len()>5{return Err("maximum 5 extension registries".into());}
    let mut unique=Vec::new();
    for url in urls{packages::validate_url(&url)?;if !unique.contains(&url){unique.push(url)}}
    state.store.put(SOURCES,&json!({"id":"catalog","urls":unique})).ok_or("cannot save registries")?;
    Ok(json!({"urls":sources(state)}))
}
pub fn list(state:&AppState)->Vec<Value>{state.store.all(KIND)}
pub async fn search(state:&AppState,q:&str)->Value{
    let q=q.trim().to_lowercase();let mut items=Vec::new();let mut errors=Vec::new();
    // Separate bounded requests, no installer or process is launched on search.
    let urls=sources(state);
    let results=futures_util::future::join_all(urls.iter().map(|source|packages::catalog(source))).await;
    for (source,result) in urls.iter().zip(results){
        match result{
            Ok(rows)=>for m in rows{
                if q.is_empty()||format!("{} {} {} {}",m.id,m.name,m.description,m.tools.iter().map(|t|t.description.as_str()).collect::<Vec<_>>().join(" ")).to_lowercase().contains(&q){
                    if items.len()<200{items.push(json!({"manifest":m,"source":source,"revision":packages::revision(&m),"compatible":m.compatible()}));}
                }
            },
            Err(error)=>errors.push(json!({"source":source,"error":error}))
        }
    }
    json!({"items":items,"errors":errors})
}
pub async fn install(state:&AppState,source:&str,requested:Manifest)->Result<Value,String>{
    let _guard=LIFECYCLE.lock().await;
    requested.validate()?;
    if !sources(state).iter().any(|s|s==source){return Err("extension registry not configured".into());}
    if !packages::catalog(source).await?.iter().any(|m|m==&requested){return Err("catalog changed; search and review again".into());}
    let old=state.store.get(KIND,&requested.id);
    if let Some(row)=&old{
        if row["source"].as_str()!=Some(source){return Err("extension identity belongs to a different registry".into());}
        if row["manifest"]==json!(requested){
            packages::check(&root(state),&requested).await?;
            return Ok(row.clone());
        }
    }
    packages::install(&root(state),&requested).await?;
    let saved=state.store.put(KIND,&json!({"id":requested.id,"manifest":requested,
        "source":source,"enabled":false,"revision":packages::revision(&requested)})).ok_or("cannot persist installed extension")?;
    if let Some(previous)=old.as_ref().and_then(manifest){
        if packages::revision(&previous)!=packages::revision(&requested){let _=packages::remove(&root(state),&previous).await;}
    }
    state.store.audit("extension.install",json!({"id":requested.id,"version":requested.version}));
    Ok(saved)
}
pub async fn enable(state:&AppState,id:&str,enabled:bool)->Result<Value,String>{
    let _guard=LIFECYCLE.lock().await;
    let mut row=state.store.get(KIND,id).ok_or("extension not installed")?;
    if enabled{packages::check(&root(state),&manifest(&row).ok_or("invalid manifest")?).await?;}
    row["enabled"]=json!(enabled);
    let saved=state.store.put(KIND,&row).ok_or("cannot save extension state")?;
    state.store.audit("extension.enable",json!({"id":id,"enabled":enabled}));
    Ok(saved)
}
pub async fn remove(state:&AppState,id:&str)->Result<Value,String>{
    let _guard=LIFECYCLE.lock().await;
    let mut row=state.store.get(KIND,id).ok_or("extension not installed")?;
    let m=manifest(&row).ok_or("invalid installed manifest")?;
    row["enabled"]=json!(false);state.store.put(KIND,&row).ok_or("cannot disable extension")?;
    packages::remove(&root(state),&m).await?;
    if !state.store.remove(KIND,id){return Err("cannot remove extension record".into());}
    state.store.audit("extension.remove",json!({"id":id}));
    Ok(json!({"removed":true}))
}
pub fn definitions(state:&AppState)->Vec<Value>{
    let mut out=Vec::new();
    for row in list(state).into_iter().filter(|v|v["enabled"]==true){
        let Some(m)=manifest(&row) else{continue};
        if !m.compatible(){continue;}
        let revision=packages::revision(&m);
        for tool in &m.tools{
            out.push(json!({"type":"function","function":{"name":m.qualified(tool,&revision),
                "description":format!("[{}] {}",m.name,tool.description),"parameters":tool.parameters}}));
        }
    }
    out
}
pub async fn execute(state:&AppState,name:&str,args:&Value)->Result<Value,String>{
    if !name.starts_with(PREFIX){return Err("invalid extension tool name".into());}
    // A lifecycle operation cannot replace/remove an executable mid-call.
    let _guard=LIFECYCLE.lock().await;
    for row in list(state).into_iter().filter(|v|v["enabled"]==true){
        let Some(m)=manifest(&row)else{continue};
        let revision=packages::revision(&m);
        if let Some(tool)=m.tools.iter().find(|t|m.qualified(t,&revision)==name){
            return packages::invoke(&root(state),&m,tool,args).await;
        }
    }
    Err("extension tool disabled, removed or changed; rediscover it".into())
}
