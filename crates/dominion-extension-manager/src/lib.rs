//! Independent registry, artifact installation and process invocation.
//! No dependency on application, MCP, Hugging Face or a particular provider.
use dominion_agent_sdk::plugin_protocol::{self as protocol,Manifest,Runtime,Tool};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::io::{AsyncReadExt,AsyncWriteExt};
pub mod github_catalog;
const MAX_ARTIFACT:usize=64*1024*1024;
const MAX_INDEX:usize=2*1024*1024;
const MAX_OUTPUT:usize=1024*1024;
pub fn revision(m:&Manifest)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(m).expect("serializable manifest")))[..12].to_string()}
pub fn validate_url(raw:&str)->Result<reqwest::Url,String>{
    let url=reqwest::Url::parse(raw).map_err(|_|"invalid extension URL")?;
    if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some()||url.fragment().is_some(){return Err("extension URLs require HTTPS without embedded credentials".into());}
    Ok(url)
}
async fn fetch(raw:&str,limit:usize)->Result<Vec<u8>,String>{
    let client=reqwest::Client::builder().user_agent("DOMINION-Extension-Manager/1").timeout(Duration::from_secs(25))
      .redirect(reqwest::redirect::Policy::custom(|a|{if a.previous().len()>=5||a.url().scheme()!="https"||!a.url().username().is_empty()||a.url().password().is_some(){a.error("invalid redirect")}else{a.follow()}}))
      .build().map_err(|_|"extension network client unavailable")?;
    let mut response=client.get(validate_url(raw)?).send().await.map_err(|_|"extension download unavailable")?.error_for_status().map_err(|_|"extension HTTP error")?;
    if response.content_length().is_some_and(|n|n>limit as u64){return Err("extension download exceeds limit".into());}
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|_|"extension download interrupted")?{
      if bytes.len().saturating_add(chunk.len())>limit{return Err("extension download exceeds limit".into());}
      bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub async fn catalog(url:&str)->Result<Vec<Manifest>,String>{
    parse_catalog(&fetch(url,MAX_INDEX).await?)
}
fn parse_catalog(bytes:&[u8])->Result<Vec<Manifest>,String>{
    let data:Value=serde_json::from_slice(bytes).map_err(|_|"invalid registry JSON")?;
    if data["schemaVersion"]!=1{return Err("unsupported extension registry version".into());}
    let items=data["items"].as_array().ok_or("registry items missing")?;
    if items.len()>1000{return Err("registry exceeds 1000 entries".into());}
    let mut valid=Vec::new();
    for entry in items{
        let m:Manifest=serde_json::from_value(entry.clone()).map_err(|_|"invalid registry manifest")?;
        m.validate()?;validate_url(&m.artifact.url)?;valid.push(m);
    }
    Ok(valid)
}
fn entry_path(root:&Path,m:&Manifest)->PathBuf{
    let name=match m.runtime{Runtime::Node=>"entry.mjs",Runtime::Python=>"entry.py",Runtime::Native if cfg!(windows)=>"entry.exe",Runtime::Native=>"entry"};
    root.join(&m.id).join(revision(m)).join(name)
}
fn verified(m:&Manifest,bytes:&[u8])->bool{!bytes.is_empty()&&bytes.len()<=MAX_ARTIFACT&&format!("{:x}",Sha256::digest(bytes))==m.artifact.sha256}
pub async fn install(root:&Path,m:&Manifest)->Result<(),String>{
    m.validate()?;if !m.compatible(){return Err("extension platform mismatch".into());}
    install_bytes(root,m,&fetch(&m.artifact.url,MAX_ARTIFACT).await?).await
}
/// Install an explicitly supplied artifact using exactly the remote installer checks.
pub async fn install_bytes(root:&Path,m:&Manifest,bytes:&[u8])->Result<(),String>{
    m.validate()?;if !m.compatible(){return Err("extension platform mismatch".into());}
    if !verified(m,bytes){return Err("extension SHA-256 verification failed".into());}
    // Avoid replacing an existing verified executable (also works on Windows).
    if check(root,m).await.is_ok(){return Ok(());}
    let path=entry_path(root,m);let dir=path.parent().ok_or("invalid extension path")?;
    tokio::fs::create_dir_all(dir).await.map_err(|e|e.to_string())?;
    let temporary=dir.join(format!(".install-{}",uuid::Uuid::new_v4()));
    let result=async{
        let mut file=tokio::fs::OpenOptions::new().create_new(true).write(true).open(&temporary).await?;
        file.write_all(bytes).await?;file.sync_all().await?;drop(file);
        #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;tokio::fs::set_permissions(&temporary,std::fs::Permissions::from_mode(0o700)).await?;}
        #[cfg(windows)]
        if tokio::fs::symlink_metadata(&path).await.is_ok(){tokio::fs::remove_file(&path).await?;}
        tokio::fs::rename(&temporary,&path).await
    }.await;
    if result.is_err(){let _=tokio::fs::remove_file(&temporary).await;}
    result.map_err(|e|e.to_string())
}
pub async fn remove(root:&Path,m:&Manifest)->Result<(),String>{
    m.validate()?;let path=entry_path(root,m);let dir=path.parent().ok_or("invalid extension directory")?;
    match tokio::fs::remove_dir_all(dir).await{Ok(())=>Ok(()),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(e)=>Err(e.to_string())}
}
pub async fn check(root:&Path,m:&Manifest)->Result<(),String>{
    m.validate()?;if !m.compatible(){return Err("extension platform mismatch".into());}
    let path=entry_path(root,m);
    let meta=tokio::fs::symlink_metadata(&path).await.map_err(|_|"extension artifact missing")?;
    if !meta.is_file()||meta.len()>MAX_ARTIFACT as u64{return Err("invalid extension artifact".into());}
    let bytes=tokio::fs::read(path).await.map_err(|_|"extension artifact unreadable")?;
    if !verified(m,&bytes){return Err("extension artifact integrity check failed".into());}Ok(())
}
/// Runtime lookup does not start the interpreter or execute package code.
fn interpreter(runtime:Runtime)->Result<Option<PathBuf>,String>{
    let name=match runtime{Runtime::Node=>"node",Runtime::Python if cfg!(windows)=>"python",Runtime::Python=>"python3",Runtime::Native=>return Ok(None)};
    let path=std::env::var_os("PATH").ok_or_else(||format!("runtime missing: {name}"))?;
    for dir in std::env::split_paths(&path).filter(|dir|dir.is_absolute()){
        let candidate=dir.join(if cfg!(windows){format!("{name}.exe")}else{name.to_string()});
        if let Ok(meta)=std::fs::metadata(&candidate){
            if !meta.is_file(){continue;}
            #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;if meta.permissions().mode()&0o111==0{continue;}}
            if let Ok(path)=candidate.canonicalize(){return Ok(Some(path));}
        }
    }
    Err(format!("runtime missing: {name}; install it and restart DOMINION"))
}
pub fn check_runtime(m:&Manifest)->Result<(),String>{interpreter(m.runtime).map(|_|())}
/// Persistent package data is distinct from revision-bound executable files.
fn data_path(root:&Path,m:&Manifest)->PathBuf{root.join(&m.id).join("data")}
pub async fn remove_data(root:&Path,m:&Manifest)->Result<(),String>{
    m.validate()?;
    match tokio::fs::remove_dir_all(data_path(root,m)).await{
        Ok(())=>Ok(()),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(e)=>Err(e.to_string())
    }
}
fn unknown(error:String)->Value{json!({"success":false,"outcomeUnknown":true,"error":error})}
/// Child process is not a sandbox. Owner admission is required. No application
/// credentials are passed; after dispatch a missing receipt is uncertain.
pub async fn invoke(root:&Path,m:&Manifest,tool:&Tool,args:&Value)->Result<Value,String>{
    check(root,m).await?;if !m.tools.contains(tool){return Err("extension tool not declared".into());}
    let id=uuid::Uuid::new_v4().to_string();
    let mut request=serde_json::to_vec(&protocol::request(&id,&tool.name,args)?).map_err(|e|e.to_string())?;
    if request.len()>MAX_OUTPUT{return Err("extension input exceeds limit".into());}request.push(b'\n');
    let entry=entry_path(root,m).canonicalize().map_err(|_|"extension entry missing")?;
    let executable=interpreter(m.runtime)?;
    let mut command=match (m.runtime,executable){
        (Runtime::Node,Some(path))=>{let mut c=tokio::process::Command::new(path);c.arg(&entry);c},
        (Runtime::Python,Some(path))=>{let mut c=tokio::process::Command::new(path);c.arg("-I").arg(&entry);c},
        (Runtime::Native,None)=>tokio::process::Command::new(&entry),
        _=>return Err("invalid extension runtime".into()),
    };
    let data=data_path(root,m);
    tokio::fs::create_dir_all(&data).await.map_err(|e|e.to_string())?;
    let data=data.canonicalize().map_err(|_|"extension data directory unavailable")?;
    command.current_dir(entry.parent().ok_or("invalid extension entry")?).env_clear()
        .env("DOMINION_EXTENSION_DATA_DIR",data);
    if let Some(path)=std::env::var_os("PATH"){command.env("PATH",path);}
    #[cfg(windows)] if let Some(root)=std::env::var_os("SystemRoot"){command.env("SystemRoot",root);}
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut child=command.spawn().map_err(|e|format!("extension runtime unavailable: {e}"))?;
    let result=tokio::time::timeout(Duration::from_secs(45),async{
        let mut stdin=child.stdin.take().ok_or("extension stdin unavailable")?;
        let stdout=child.stdout.take().ok_or("extension stdout unavailable")?;
        let send=async{stdin.write_all(&request).await.map_err(|e|e.to_string())?;stdin.shutdown().await.map_err(|e|e.to_string())?;drop(stdin);Ok::<(),String>(())};
        let receive=async{let mut bytes=Vec::new();stdout.take((MAX_OUTPUT+1) as u64).read_to_end(&mut bytes).await.map_err(|e|e.to_string())?;
            if bytes.len()>MAX_OUTPUT{return Err("extension response too large".into());}Ok::<Vec<u8>,String>(bytes)};
        let (_,bytes)=tokio::try_join!(send,receive)?;
        if !child.wait().await.map_err(|e|e.to_string())?.success(){return Err("extension process exited unsuccessfully".into());}
        protocol::response(&id,serde_json::from_slice(&bytes).map_err(|_|"invalid extension JSON response")?)
    }).await;
    match result{Ok(Ok(value))=>Ok(value),Ok(Err(error))=>{let _=child.kill().await;Ok(unknown(error))},
        Err(_)=>{let _=child.kill().await;Ok(unknown("extension deadline exceeded; do not replay automatically".into()))}}
}
#[cfg(test)]
mod tests{use super::*;
 #[tokio::test]async fn verified_install_execute_remove_without_mcp(){
    let bytes=br#"let input='';for await(const c of process.stdin)input+=c;const r=JSON.parse(input);console.log(JSON.stringify({protocol:r.protocol,id:r.id,ok:true,result:r.arguments}));"#;
    let root=std::env::temp_dir().join(format!("dominion-plugin-test-{}",uuid::Uuid::new_v4()));
    let m:Manifest=serde_json::from_value(json!({"sdkVersion":1,"id":"fixture","name":"Fixture","version":"1.0.0","description":"fixture","runtime":"node",
      "artifact":{"url":"https://example.invalid/plugin.mjs","sha256":format!("{:x}",Sha256::digest(bytes))},"tools":[{"name":"echo","description":"Echo a message","parameters":{"type":"object"}}]})).unwrap();
    assert!(install_bytes(&root,&m,b"tampered").await.is_err());
    install_bytes(&root,&m,bytes).await.unwrap();
    assert_eq!(invoke(&root,&m,&m.tools[0],&json!({"text":"hello"})).await.unwrap()["result"]["text"],"hello");
    let mut updated=m.clone();updated.version="2.0.0".into();assert_ne!(revision(&m),revision(&updated));
    tokio::fs::write(entry_path(&root,&m),b"tampered").await.unwrap();assert!(invoke(&root,&m,&m.tools[0],&json!({})).await.is_err());
    remove(&root,&m).await.unwrap();assert!(check(&root,&m).await.is_err());let _=tokio::fs::remove_dir_all(root).await;
 }}
