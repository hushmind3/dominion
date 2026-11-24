//! Owner-approved installation of standalone SDK executables.
//! A native executable runs as the OS user; this is NOT a sandbox.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::{Path as FsPath,PathBuf},process::Stdio,time::Duration};
use tokio::io::AsyncReadExt;
use super::{extension_registry as registry,response::error};

pub(super) const PACKAGE_KIND:&str="sdk_extension_package";
const MAX_BYTES:usize=64*1024*1024;
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Install{
    version:String,source:String,sha256:String,trust_executable:bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start{token:String,trust_executable:bool}
fn digest(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn valid_digest(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit())}
fn release_url(url:&reqwest::Url)->bool{
    url.scheme()=="https"&&url.host_str()==Some("github.com")&&url.port().is_none()
        &&url.username().is_empty()&&url.password().is_none()&&url.query().is_none()
        &&url.fragment().is_none()
        &&url.path_segments().is_some_and(|p|{
            let parts=p.collect::<Vec<_>>();
            parts.len()==6&&parts[2]=="releases"&&parts[3]=="download"
                &&parts.iter().all(|part|!part.is_empty())
        })
}
fn redirect_allowed(url:&reqwest::Url)->bool{
    url.scheme()=="https"&&url.port().is_none()&&url.username().is_empty()&&url.password().is_none()
        &&(release_url(url)||matches!(url.host_str(),Some("release-assets.githubusercontent.com"|"objects.githubusercontent.com")))
}
async fn read_file(path:&FsPath)->Result<Vec<u8>,String>{
    let metadata=tokio::fs::symlink_metadata(path).await.map_err(|e|e.to_string())?;
    if !metadata.is_file()||metadata.len()>MAX_BYTES as u64{return Err("Expected a regular executable no larger than 64 MiB".into())}
    let mut bytes=Vec::new();
    tokio::fs::File::open(path).await.map_err(|e|e.to_string())?
        .take((MAX_BYTES+1) as u64).read_to_end(&mut bytes).await.map_err(|e|e.to_string())?;
    if bytes.is_empty()||bytes.len()>MAX_BYTES{return Err("Invalid executable size".into())}
    Ok(bytes)
}
async fn fetch_package(source:&str)->Result<Vec<u8>,String>{
    if source.starts_with("https://"){
        let url=reqwest::Url::parse(source).map_err(|_|"Invalid release URL".to_string())?;
        if !release_url(&url){return Err("Use an exact public GitHub release asset URL".into())}
        let client=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::custom(|a|{
                if a.previous().len()<5&&redirect_allowed(a.url()){a.follow()}else{a.error("Untrusted release redirect")}
            })).build().map_err(|_|"Cannot initialize release download".to_string())?;
        let mut res=client.get(url).send().await.map_err(|_|"Release download failed".to_string())?
            .error_for_status().map_err(|_|"Release download failed".to_string())?;
        if res.content_length().is_some_and(|n|n>MAX_BYTES as u64){return Err("Release exceeds 64 MiB".into())}
        let mut bytes=Vec::new();
        while let Some(chunk)=res.chunk().await.map_err(|_|"Release download interrupted".to_string())?{
            if bytes.len().saturating_add(chunk.len())>MAX_BYTES{return Err("Release exceeds 64 MiB".into())}
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty(){return Err("Empty release".into())}
        Ok(bytes)
    }else{
        let path=FsPath::new(source);
        if !path.is_absolute(){return Err("Use an absolute local executable path or GitHub release URL".into())}
        read_file(path).await
    }
}
fn artifact(state:&AppState,row:&Value)->Option<PathBuf>{
    let id=uuid::Uuid::parse_str(row["artifact"].as_str()?).ok()?;
    Some(state.config.data_dir.join("managed-apps").join(id.to_string())
        .join(if cfg!(windows){"app.exe"}else{"app"}))
}
async fn cleanup(state:&AppState,row:&Value){
    if let Some(path)=artifact(state,row){
        if let Err(e)=tokio::fs::remove_file(&path).await{
            if e.kind()!=std::io::ErrorKind::NotFound{
                state.store.audit("sdk.extension_package_cleanup_failed",json!({"id":row["id"],"error":e.to_string()}));
                return
            }
        }
        if let Some(parent)=path.parent(){let _=tokio::fs::remove_dir(parent).await;}
    }
}
fn change(state:&AppState,id:&str){state.bus.record("sdk_extension",&json!({"id":id,"packageChanged":true}));}
async fn snapshot(state:&AppState,id:&str)->Result<Value,Response>{
    let mut children=state.managed_apps.lock().await;
    let mut running=false;let mut exit_code=None;
    if let Some(child)=children.get_mut(id){
        match child.try_wait(){
            Ok(None)=>running=true,
            Ok(Some(exit))=>exit_code=exit.code(),
            Err(_)=>return Err(error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot inspect app process")),
        }
    }
    let package=state.store.get(PACKAGE_KIND,id).filter(|r|r["artifact"].is_string());
    let present=package.as_ref().and_then(|p|artifact(state,p)).is_some_and(|p|p.is_file());
    Ok(json!({"installed":package.is_some(),"filePresent":present,
        "version":package.as_ref().map(|p|p["version"].clone()),
        "sha256":package.as_ref().map(|p|p["sha256"].clone()),
        "running":running,"exitCode":exit_code,"platform":std::env::consts::OS,"arch":std::env::consts::ARCH}))
}
pub async fn status(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if let Err(r)=registry::load(&state,&id){return r}
    match snapshot(&state,&id).await{Ok(row)=>Json(row).into_response(),Err(r)=>r}
}
pub(super) async fn stop_child(state:&AppState,id:&str)->Result<(),Response>{
    let mut children=state.managed_apps.lock().await;
    if let Some(child)=children.get_mut(id){
        if child.try_wait().map_err(|_|error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot inspect app process"))?.is_none(){
            child.start_kill().map_err(|_|error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot stop app process"))?;
            tokio::time::timeout(Duration::from_secs(5),child.wait()).await
                .map_err(|_|error(StatusCode::SERVICE_UNAVAILABLE,"App is still stopping"))?
                .map_err(|_|error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot reap app process"))?;
        }
    }
    children.remove(id);
    Ok(())
}
pub(crate) async fn stop_all(state:&AppState){
    let mut children=state.managed_apps.lock().await;
    for (_,child) in children.iter_mut(){let _=child.start_kill();}
    for (_,child) in children.iter_mut(){let _=tokio::time::timeout(Duration::from_secs(2),child.wait()).await;}
    children.clear();
}
pub async fn install(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Install>)->Response{
    let _guard=match registry::lock(&state,&id){Ok(g)=>g,Err(r)=>return r};
    let mut registration=match registry::load(&state,&id){Ok(r)=>r,Err(r)=>return r};
    if registration.enabled{return error(StatusCode::CONFLICT,"Disable the app before installing or updating")}
    if !input.trust_executable||!valid_digest(&input.sha256)||input.source.len()>4096{
        return error(StatusCode::BAD_REQUEST,"Explicit trust and a SHA-256 digest are required")
    }
    registration.manifest.version=input.version.clone();
    if let Err(e)=registration.manifest.validate(){return error(StatusCode::BAD_REQUEST,&e)}
    if let Err(r)=stop_child(&state,&id).await{return r}
    let bytes=match fetch_package(&input.source).await{Ok(b)=>b,Err(e)=>return error(StatusCode::BAD_REQUEST,&e)};
    let expected=input.sha256.to_ascii_lowercase();
    if digest(&bytes)!=expected{return error(StatusCode::BAD_REQUEST,"Executable SHA-256 does not match")}
    if state.maintenance_mode(){return error(StatusCode::SERVICE_UNAVAILABLE,"Maintenance mode")}
    let row=json!({"id":id,"artifact":uuid::Uuid::new_v4().to_string(),"version":input.version,
        "sha256":expected,"bytes":bytes.len(),"platform":std::env::consts::OS,"arch":std::env::consts::ARCH});
    let path=artifact(&state,&row).unwrap();
    let written=async{
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        let mut file=tokio::fs::OpenOptions::new().write(true).create_new(true).open(&path).await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        #[cfg(unix)]{
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o700)).await?;
        }
        Ok::<(),std::io::Error>(())
    }.await;
    if written.is_err(){cleanup(&state,&row).await;return error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot stage executable")}
    let previous=state.store.get(PACKAGE_KIND,&id);
    if state.store.transaction(&[(PACKAGE_KIND.into(),row.clone()),("sdk_extension".into(),json!(registration))]).is_err(){
        cleanup(&state,&row).await;
        return error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot commit package; previous version retained")
    }
    if let Some(old)=previous{cleanup(&state,&old).await;}
    change(&state,&id);
    match snapshot(&state,&id).await{Ok(r)=>Json(r).into_response(),Err(r)=>r}
}
pub async fn uninstall(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let _guard=match registry::lock(&state,&id){Ok(g)=>g,Err(r)=>return r};
    let registration=match registry::load(&state,&id){Ok(r)=>r,Err(r)=>return r};
    if registration.enabled{return error(StatusCode::CONFLICT,"Disable the app before uninstalling")}
    if let Err(r)=stop_child(&state,&id).await{return r}
    if let Some(old)=state.store.get(PACKAGE_KIND,&id){
        if !state.store.remove(PACKAGE_KIND,&id){return error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot remove package registration")}
        cleanup(&state,&old).await;
    }
    change(&state,&id);
    Json(json!({"installed":false,"running":false})).into_response()
}
pub async fn start(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<Start>)->Response{
    let _guard=match registry::lock(&state,&id){Ok(g)=>g,Err(r)=>return r};
    let registration=match registry::load(&state,&id){Ok(r)=>r,Err(r)=>return r};
    if !registration.enabled||!input.trust_executable{return error(StatusCode::CONFLICT,"Enable the app and explicitly trust its executable")}
    let Some(grant)=state.store.get("sdk_extension_grant",&id)else{return error(StatusCode::UNAUTHORIZED,"Issue an app credential first")};
    let expected=grant["tokenSha256"].as_str().unwrap_or("");
    let actual=digest(input.token.as_bytes());
    if input.token.len()!=64||expected.len()!=64||actual.bytes().zip(expected.bytes()).fold(0u8,|d,(a,b)|d|(a^b))!=0{
        return error(StatusCode::UNAUTHORIZED,"Invalid app credential")
    }
    let Some(row)=state.store.get(PACKAGE_KIND,&id)else{return error(StatusCode::NOT_FOUND,"Install an executable first")};
    let Some(path)=artifact(&state,&row)else{return error(StatusCode::CONFLICT,"Invalid package record")};
    if row["platform"]!=std::env::consts::OS||row["arch"]!=std::env::consts::ARCH{return error(StatusCode::CONFLICT,"Package belongs to another platform")}
    let bytes=match read_file(&path).await{Ok(b)=>b,Err(_)=>return error(StatusCode::CONFLICT,"Installed executable missing or unreadable")};
    if row["sha256"]!=digest(&bytes){return error(StatusCode::CONFLICT,"Installed executable changed; reinstall it")}
    let path=match tokio::fs::canonicalize(path).await{Ok(p)=>p,Err(_)=>return error(StatusCode::CONFLICT,"Cannot resolve executable")};
    let mut children=state.managed_apps.lock().await;
    if let Some(child)=children.get_mut(&id){
        match child.try_wait(){Ok(None)=>return error(StatusCode::CONFLICT,"App is already running"),Ok(Some(_))=>{},Err(_)=>return error(StatusCode::INTERNAL_SERVER_ERROR,"Cannot inspect app process")}
    }
    if state.maintenance_mode(){return error(StatusCode::SERVICE_UNAVAILABLE,"Maintenance mode")}
    let mut command=tokio::process::Command::new(&path);
    command.current_dir(path.parent().unwrap()).env_clear()
        .env("DOMINION_BASE_URL",format!("http://127.0.0.1:{}",state.config.port))
        .env("DOMINION_EXTENSION_ID",&id).env("DOMINION_EXTENSION_TOKEN",&input.token)
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)] if let Some(root)=std::env::var_os("SystemRoot"){command.env("SystemRoot",root);}
    let child=match command.spawn(){Ok(c)=>c,Err(_)=>return error(StatusCode::BAD_REQUEST,"Executable could not start on this platform")};
    children.insert(id.clone(),child);
    drop(children);
    change(&state,&id);
    match snapshot(&state,&id).await{Ok(r)=>Json(r).into_response(),Err(r)=>r}
}
pub async fn stop(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let _guard=match registry::lock(&state,&id){Ok(g)=>g,Err(r)=>return r};
    if let Err(r)=registry::load(&state,&id){return r}
    if let Err(r)=stop_child(&state,&id).await{return r}
    change(&state,&id);
    match snapshot(&state,&id).await{Ok(r)=>Json(r).into_response(),Err(r)=>r}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn release_urls_are_pinned_and_redirects_do_not_allow_arbitrary_hosts(){
        assert!(release_url(&reqwest::Url::parse("https://github.com/a/b/releases/download/v1/app").unwrap()));
        for url in ["http://github.com/a/b/releases/download/v1/app","https://github.com/a/b/releases/latest/download/app","https://github.com.evil/a/b/releases/download/v1/app","https://127.0.0.1/app"]{
            assert!(!release_url(&reqwest::Url::parse(url).unwrap()));
        }
        assert!(!redirect_allowed(&reqwest::Url::parse("https://127.0.0.1/app").unwrap()));
    }
    #[tokio::test]
    async fn local_package_is_bounded_and_digest_checked(){
        let dir=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path=dir.join("fixture");
        tokio::fs::write(&path,b"fixture").await.unwrap();
        let bytes=fetch_package(path.to_str().unwrap()).await.unwrap();
        assert_eq!(digest(&bytes),digest(b"fixture"));
        assert!(valid_digest(&digest(&bytes)));
        assert!(fetch_package("relative/path").await.is_err());
        assert!(read_file(&dir).await.is_err());
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
