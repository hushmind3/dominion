//! Public, language-neutral plugin package discovery. A repository is never
//! executed. Only a reviewed v1 manifest pinned to an immutable commit may
//! install a digest-pinned OCI image through the existing managed lifecycle.
use super::{Install,valid_image};
use crate::{state::AppState,api::response::error};
use axum::{extract::{Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::time::Duration;

#[derive(Debug,Deserialize,Serialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
struct Manifest{
    schema_version:u32,name:String,description:String,version:String,
    image:String,network:bool,port:Option<u16>,
}
#[derive(Deserialize)]
pub struct Search{q:Option<String>}
#[derive(Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
pub struct Request{repository:String,commit:String,manifest_digest:String,confirmed:bool}
fn repository_ok(repo:&str)->bool{
    repo.len()<=140&&repo.split('/').count()==2&&repo.split('/').all(|p|
        !p.is_empty()&&p!="."&&p!=".."&&p.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.')))
}
fn hex(s:&str,n:usize)->bool{s.len()==n&&s.bytes().all(|b|b.is_ascii_hexdigit())}
fn text_ok(s:&str,max:usize)->bool{!s.trim().is_empty()&&s.len()<=max&&!s.chars().any(char::is_control)}
fn parse(repo:&str,value:Value)->Result<Manifest,String>{
    if !repository_ok(repo){return Err("Invalid repository".into())}
    let m:Manifest=serde_json::from_value(value).map_err(|_|"Unsupported package manifest")?;
    let prefix=format!("ghcr.io/{}@",repo.to_ascii_lowercase());
    if m.schema_version!=1||!text_ok(&m.name,120)||!text_ok(&m.description,1500)||!text_ok(&m.version,80)
        ||!m.image.starts_with(&prefix)||!valid_image(&m.image)
        ||!m.image.strip_prefix(&prefix).is_some_and(|s|s.strip_prefix("sha256:").is_some_and(|d|hex(d,64)))
        ||m.port==Some(0)||m.port.is_some()&&!m.network{
        return Err("Unsupported manifest or image identity".into())
    }
    Ok(m)
}
fn digest(m:&Manifest)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(m).expect("manifest serialization")))}
fn install_id(repo:&str)->String{
    let hash=Sha256::digest(format!("dominion-market-v1:{}",repo.to_ascii_lowercase()).as_bytes());
    let mut bytes=[0u8;16];bytes.copy_from_slice(&hash[..16]);bytes[6]=(bytes[6]&0x0f)|0x50;bytes[8]=(bytes[8]&0x3f)|0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}
fn client()->Result<reqwest::Client,String>{
    reqwest::Client::builder().user_agent("DOMINION-package-market/1")
        .timeout(Duration::from_secs(12)).redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e|e.to_string())
}
async fn read(client:&reqwest::Client,url:reqwest::Url,limit:usize)->Result<Value,String>{
    let response=client.get(url).send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){return Err(format!("GitHub HTTP {}",response.status()))}
    crate::integrations::network::read_bounded_json(response,limit,"plugin package").await
}
fn github_url(path:&str)->Result<reqwest::Url,String>{
    reqwest::Url::parse(&format!("https://api.github.com/{path}")).map_err(|e|e.to_string())
}
async fn load(client:&reqwest::Client,repo:&str,commit:&str)->Result<Manifest,String>{
    if !repository_ok(repo)||!hex(commit,40){return Err("Invalid package source".into())}
    let url=reqwest::Url::parse(&format!("https://raw.githubusercontent.com/{repo}/{commit}/dominion-plugin.json")).map_err(|e|e.to_string())?;
    parse(repo,read(client,url,16384).await?)
}
async fn resolve(client:&reqwest::Client,repo:&str)->Result<Value,String>{
    if !repository_ok(repo){return Err("Invalid repository".into())}
    let row=read(client,github_url(&format!("repos/{repo}"))?,512*1024).await?;
    if row["private"]!=false||row["full_name"].as_str().is_none_or(|s|!s.eq_ignore_ascii_case(repo)){
        return Err("Only public repositories are supported".into())
    }
    let branch=row["default_branch"].as_str().filter(|s|!s.is_empty()&&s.len()<200).ok_or("No default branch")?;
    let mut url=github_url(&format!("repos/{repo}/commits/"))?;
    url.path_segments_mut().map_err(|_|"Invalid branch URL")?.pop_if_empty().push(branch);
    let commit=read(client,url,4*1024*1024).await?["sha"].as_str().filter(|s|hex(s,40)).ok_or("Invalid commit")?.to_owned();
    let m=load(client,repo,&commit).await?;
    Ok(json!({"repository":repo,"commit":commit,"manifestDigest":digest(&m),"manifest":m,
        "installId":install_id(repo),"url":format!("https://github.com/{repo}")}))
}
pub async fn search(Query(input):Query<Search>)->Response{
    let result:Result<Value,String>=async{
        let q=input.q.as_deref().unwrap_or("").trim();
        if q.len()>160||q.chars().any(char::is_control){return Err("Invalid search".into())}
        let client=client()?;
        if repository_ok(q){
            return Ok(json!({"items":[resolve(&client,q).await?],"errors":[]}))
        }
        // Quote free-form words so no user-supplied qualifier can override the topic.
        let safe=q.replace(['"','\\',':']," ");
        let query=format!("topic:dominion-plugin archived:false {}",safe.split_whitespace().map(|w|format!("\"{w}\"")).collect::<Vec<_>>().join(" "));
        let mut url=github_url("search/repositories")?;
        url.query_pairs_mut().append_pair("q",&query).append_pair("per_page","8");
        let rows=read(&client,url,2*1024*1024).await?;
        let repos=rows["items"].as_array().ok_or("Invalid GitHub search response")?.iter()
            .filter(|r|r["private"]==false).filter_map(|r|r["full_name"].as_str()).filter(|s|repository_ok(s)).take(8).collect::<Vec<_>>();
        let results=futures_util::future::join_all(repos.iter().map(|repo|{let client=&client;async move{(*repo,resolve(client,repo).await)}})).await;
        let mut items=vec![];let mut errors=vec![];
        for (repo,r) in results{match r{Ok(v)=>items.push(v),Err(e)=>errors.push(json!({"repository":repo,"message":e}))}}
        Ok(json!({"items":items,"errors":errors}))
    }.await;
    match result{Ok(v)=>Json(v).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
pub async fn install(State(state):State<AppState>,Json(input):Json<Request>)->Response{
    if !input.confirmed||!repository_ok(&input.repository)||!hex(&input.commit,40)||!hex(&input.manifest_digest,64){
        return error(StatusCode::BAD_REQUEST,"설치할 패키지와 권한을 확인하세요.")
    }
    let m=match async{load(&client()?,&input.repository,&input.commit).await}.await{
        Ok(m)=>m,Err(e)=>return error(StatusCode::BAD_GATEWAY,&e)
    };
    if digest(&m)!=input.manifest_digest{return error(StatusCode::CONFLICT,"검토한 패키지 내용이 변경되었습니다.")}
    super::install(State(state),Json(Install{id:install_id(&input.repository),name:m.name,image:m.image,network:m.network,port:m.port,confirmed:true})).await
}
#[cfg(test)]
mod tests{
    use super::*;
    fn sample()->Value{json!({"schemaVersion":1,"name":"Example","description":"Example independent app","version":"1.0.0",
        "image":format!("ghcr.io/example/app@sha256:{}","a".repeat(64)),"network":true,"port":8080})}
    #[test]
    fn only_same_repository_digest_pinned_declarative_package(){
        let m=parse("Example/App",sample()).unwrap();assert_eq!(digest(&m).len(),64);
        assert_eq!(install_id("Example/App"),install_id("example/app"));
        assert!(parse("other/app",sample()).is_err());
        for (field,value) in [("image",json!("ghcr.io/example/app:latest")),("schemaVersion",json!(2)),("network",json!(false)),("command",json!(["shell"]))]{
            let mut v=sample();v[field]=value;assert!(parse("example/app",v).is_err());
        }
        assert!(!repository_ok("a/../b"));assert!(!repository_ok("a/b?ref=x"));
    }
}
