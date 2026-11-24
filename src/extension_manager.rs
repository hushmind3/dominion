//! Optional app adapter for the independent extension package manager.
use crate::state::AppState;
use dominion_agent_sdk::plugin_protocol::{Manifest,PREFIX};
use dominion_extension_manager as packages;
use serde_json::{json,Value};
use std::path::PathBuf;
static SOURCES_LOCK:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());
async fn package_lock(state:&AppState,id:&str)->Result<tokio::sync::OwnedMutexGuard<()>,String>{
    if !dominion_agent_sdk::plugin_protocol::segment(id){return Err("invalid extension id".into());}
    let gate={
        let mut gates=state.extension_gates.lock().map_err(|_|"extension lock unavailable")?;
        gates.retain(|_,gate|gate.strong_count()>0);
        if let Some(gate)=gates.get(id).and_then(std::sync::Weak::upgrade){gate}
        else{
            let gate=std::sync::Arc::new(tokio::sync::Mutex::new(()));
            gates.insert(id.to_string(),std::sync::Arc::downgrade(&gate));gate
        }
    };
    Ok(gate.lock_owned().await)
}
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
pub fn github_enabled(state:&AppState)->bool{
    state.store.get(SOURCES,"catalog").and_then(|v|v["githubDiscovery"].as_bool()).unwrap_or(true)
}
pub fn settings(state:&AppState)->Value{
    json!({"urls":sources(state),"githubDiscovery":github_enabled(state)})
}
pub async fn save_sources(state:&AppState,urls:Vec<String>,github_discovery:Option<bool>)->Result<Value,String>{
    let _guard=SOURCES_LOCK.lock().await;
    if urls.len()>5{return Err("maximum 5 extension registries".into());}
    let mut unique=Vec::new();
    for url in urls{packages::validate_url(&url)?;if !unique.contains(&url){unique.push(url)}}
    let enabled=github_discovery.unwrap_or_else(||github_enabled(state));
    state.store.put(SOURCES,&json!({"id":"catalog","urls":unique,"githubDiscovery":enabled})).ok_or("cannot save registries")?;
    Ok(settings(state))
}
pub fn list(state:&AppState)->Vec<Value>{state.store.all(KIND)}
pub async fn search(state:&AppState,q:&str)->Value{
    let q=q.trim().to_lowercase();let mut items=Vec::new();let mut errors=Vec::new();
    // Separate bounded requests, no installer or process is launched on search.
    let urls=sources(state);
    let (results,public)=tokio::join!(
        futures_util::future::join_all(urls.iter().map(|source|packages::catalog(source))),
        async{if github_enabled(state){Some(packages::github_catalog::search(&q).await)}else{None}}
    );
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
    if let Some(public)=public{
        match public{
            Ok(rows)=>for (source,result) in rows{
                match result{
                    Ok(m) if items.len()<200=>items.push(json!({"manifest":m,"source":source,"revision":packages::revision(&m),"compatible":m.compatible()})),
                    Ok(_)=>{},
                    Err(error)=>errors.push(json!({"source":source,"error":error})),
                }
            },
            Err(error)=>errors.push(json!({"source":"GitHub","error":error}))
        }
    }
    // A catalog may duplicate a package; never silently merge distinct publishers.
    let mut seen=std::collections::HashSet::new();
    items.retain(|row|seen.insert((row["source"].to_string(),row["revision"].to_string())));
    json!({"items":items,"errors":errors})
}
pub async fn install(state:&AppState,source:&str,requested:Manifest)->Result<Value,String>{
    requested.validate()?;
    let _guard=package_lock(state,&requested.id).await?;
    let unchanged=if packages::github_catalog::is_source(source){
        if !github_enabled(state){return Err("public repository discovery disabled".into());}
        packages::github_catalog::resolve(source).await?==requested
    }else{
        if !sources(state).iter().any(|s|s==source){return Err("extension registry not configured".into());}
        packages::catalog(source).await?.iter().any(|m|m==&requested)
    };
    if !unchanged{return Err("catalog changed; search and review again".into());}
    persist_install(state,source,requested,None).await
}
/// A package file is an independent distribution channel. It is never fetched
/// or executed during import, and cannot overwrite a different publisher.
pub async fn install_local(state:&AppState,requested:Manifest,bytes:&[u8])->Result<Value,String>{
    requested.validate()?;
    let _guard=package_lock(state,&requested.id).await?;
    packages::validate_url(&requested.artifact.url)?;
    if bytes.is_empty()||bytes.len()>8*1024*1024{return Err("local extension artifact exceeds 8 MiB or is empty".into());}
    let source=format!("local:{}",requested.id);
    persist_install(state,&source,requested,Some(bytes)).await
}
async fn persist_install(state:&AppState,source:&str,requested:Manifest,bytes:Option<&[u8]>)->Result<Value,String>{
    let old=state.store.get(KIND,&requested.id);
    if let Some(row)=&old{
        if row["source"].as_str()!=Some(source){return Err("extension identity belongs to a different registry".into());}
        if bytes.is_none()&&row["manifest"]==json!(requested)&&packages::check(&root(state),&requested).await.is_ok(){
            return Ok(row.clone());
        }
    }
    match bytes{
        Some(data)=>packages::install_bytes(&root(state),&requested,data).await?,
        None=>packages::install(&root(state),&requested).await?,
    }
    let saved=state.store.put(KIND,&json!({"id":requested.id,"manifest":requested,
        "source":source,"enabled":false,"revision":packages::revision(&requested)})).ok_or("cannot persist installed extension")?;
    if let Some(previous)=old.as_ref().and_then(manifest){
        if packages::revision(&previous)!=packages::revision(&requested){let _=packages::remove(&root(state),&previous).await;}
    }
    state.store.audit("extension.install",json!({"id":requested.id,"version":requested.version}));
    Ok(saved)
}
pub async fn enable(state:&AppState,id:&str,enabled:bool)->Result<Value,String>{
    let _guard=package_lock(state,id).await?;
    let mut row=state.store.get(KIND,id).ok_or("extension not installed")?;
    if enabled{
        let m=manifest(&row).ok_or("invalid manifest")?;
        packages::check(&root(state),&m).await?;
        packages::check_runtime(&m)?;
    }
    row["enabled"]=json!(enabled);
    let saved=state.store.put(KIND,&row).ok_or("cannot save extension state")?;
    state.store.audit("extension.enable",json!({"id":id,"enabled":enabled}));
    Ok(saved)
}
pub async fn remove(state:&AppState,id:&str)->Result<Value,String>{
    let _guard=package_lock(state,id).await?;
    let mut row=state.store.get(KIND,id).ok_or("extension not installed")?;
    let m=manifest(&row).ok_or("invalid installed manifest")?;
    row["enabled"]=json!(false);state.store.put(KIND,&row).ok_or("cannot disable extension")?;
    packages::remove(&root(state),&m).await?;
    packages::remove_data(&root(state),&m).await?;
    if !state.store.remove(KIND,id){return Err("cannot remove extension record".into());}
    state.store.audit("extension.remove",json!({"id":id}));
    Ok(json!({"removed":true}))
}
pub async fn diagnose(state:&AppState,id:&str)->Result<Value,String>{
    let _guard=package_lock(state,id).await?;
    let row=state.store.get(KIND,id).ok_or("extension not installed")?;
    let m=manifest(&row).ok_or("invalid manifest")?;
    let integrity=packages::check(&root(state),&m).await;
    let runtime=packages::check_runtime(&m);
    Ok(json!({"id":id,"enabled":row["enabled"],"compatible":m.compatible(),
        "integrityOk":integrity.is_ok(),"runtimeOk":runtime.is_ok(),
        "integrityError":integrity.err(),"runtimeError":runtime.err()}))
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
    let id=name.strip_prefix(PREFIX).and_then(|s|s.split_once("__")).map(|(id,_)|id)
        .ok_or("invalid extension tool name")?;
    // Only this package is locked during execution. A stale approval cannot
    // invoke a different revision after an update or disable.
    let _guard=package_lock(state,id).await?;
    let row=state.store.get(KIND,id).filter(|v|v["enabled"]==true)
        .ok_or("extension tool disabled or removed; rediscover it")?;
    let m=manifest(&row).ok_or("invalid manifest")?;
    let revision=packages::revision(&m);
    let tool=m.tools.iter().find(|t|m.qualified(t,&revision)==name)
        .ok_or("extension tool changed; rediscover it")?;
    packages::invoke(&root(state),&m,tool,args).await
}

#[cfg(test)]
mod tests{
    use super::*;
    use sha2::{Digest,Sha256};
    #[tokio::test]
    async fn local_extension_lifecycle_is_independent_of_mcp_and_model_catalogs(){
        let dir=std::env::temp_dir().join(format!("dominion-extension-host-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{port:0,data_dir:dir.clone(),workspace:dir.clone(),web_dir:dir.join("web")};
        let state=AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),crate::vault::Vault::open(&config.key_path()));
        assert!(definitions(&state).is_empty());
        for (runtime,bytes) in [
            ("node",b"let s='';for await(const c of process.stdin)s+=c;const r=JSON.parse(s);console.log(JSON.stringify({protocol:r.protocol,id:r.id,ok:true,result:r.arguments}));".as_slice()),
            ("python",b"import sys,json\nr=json.load(sys.stdin)\nprint(json.dumps(dict(protocol=r['protocol'],id=r['id'],ok=True,result=r['arguments'])))\n".as_slice())
        ]{
            let m:Manifest=serde_json::from_value(json!({"sdkVersion":1,"id":format!("test_{runtime}"),"name":"Test fixture","version":"1.0.0","description":"fixture",
                "runtime":runtime,"artifact":{"url":"https://example.invalid/entry","sha256":format!("{:x}",Sha256::digest(bytes))},
                "tools":[{"name":"echo","description":"Echo","parameters":{"type":"object"}}]})).unwrap();
            assert!(install_local(&state,m.clone(),b"wrong").await.is_err());
            let row=install_local(&state,m.clone(),bytes).await.unwrap();
            assert_eq!(row["enabled"],false);
            assert!(definitions(&state).is_empty());
            let name=m.qualified(&m.tools[0],&packages::revision(&m));
            assert!(execute(&state,&name,&json!({})).await.is_err());
            enable(&state,&m.id,true).await.unwrap();
            let status=diagnose(&state,&m.id).await.unwrap();
            assert_eq!(status["integrityOk"],true);assert_eq!(status["runtimeOk"],true);
            assert_eq!(definitions(&state)[0]["function"]["name"],name);
            assert!(crate::engine::tools::requires_approval(&name));
            assert!(!crate::engine::tools::parallel_read_only_tool(&name));
            assert!(!crate::engine::tools::ui_tool_allowed(&json!({"mcpEnabled":true}),&name));
            assert!(crate::engine::tools::ui_tool_allowed(&json!({"mcpEnabled":false,"extensionsEnabled":true}),&name));
            assert_eq!(execute(&state,&name,&json!({"text":"real child process"})).await.unwrap()["result"]["text"],"real child process");
            let data=root(&state).join(&m.id).join("data");
            std::fs::write(data.join("keep"),b"persistent").unwrap();
            let mut next=m.clone();next.version="2.0.0".into();
            install_local(&state,next.clone(),bytes).await.unwrap();
            assert_eq!(std::fs::read(data.join("keep")).unwrap(),b"persistent");
            assert!(definitions(&state).is_empty());
            enable(&state,&next.id,true).await.unwrap();
            assert!(execute(&state,&name,&json!({})).await.is_err());
            remove(&state,&next.id).await.unwrap();
            assert!(!data.exists());
            assert!(definitions(&state).is_empty());
        }
        // Unrelated package operations proceed even while another gate is held.
        let held=package_lock(&state,"first").await.unwrap();
        let independent=tokio::time::timeout(std::time::Duration::from_millis(100),package_lock(&state,"second")).await;
        assert!(independent.is_ok());
        assert!(tokio::time::timeout(std::time::Duration::from_millis(10),package_lock(&state,"first")).await.is_err());
        drop(held);
        assert!(list(&state).is_empty());
        assert!(state.store.all("mcp").is_empty());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
}
