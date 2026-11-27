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
#[derive(Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
pub struct UpdateRequest{repository:String,commit:String,manifest_digest:String,expected_image_id:String,confirmed:bool}
fn repository_ok(repo:&str)->bool{
    repo.len()<=140&&repo.split('/').count()==2&&repo.split('/').all(|p|
        !p.is_empty()&&p!="."&&p!=".."&&p.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.')))
}
fn hex(s:&str,n:usize)->bool{s.len()==n&&s.bytes().all(|b|b.is_ascii_hexdigit())}
fn text_ok(s:&str,max:usize)->bool{!s.trim().is_empty()&&s.len()<=max&&!s.chars().any(char::is_control)}
fn parse(repo:&str,value:Value)->Result<Manifest,String>{
    if !repository_ok(repo){return Err("Invalid repository".into())}
    let m:Manifest=serde_json::from_value(value).map_err(|_|"Unsupported package manifest")?;
    let prefix=format!("ghcr.io/{}",repo.to_ascii_lowercase());
    let valid_path=m.image.split_once("@sha256:").is_some_and(|(image,hash)|{
        let same_repository=image.strip_prefix(&prefix).is_some_and(|suffix|suffix.is_empty()||
            suffix.strip_prefix('/').is_some_and(|leaf|!leaf.is_empty()&&leaf!="."&&leaf!=".."&&leaf.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||matches!(b,b'-'|b'_'|b'.'))));
        // GitHub's repository rename did not move the public GHCR artifact.
        // Only the existing connector gets this exact alias; other packages
        // must still match their own source repository and immutable digest.
        let renamed_connector=repo.eq_ignore_ascii_case("hushmind3/dominion")
            && image=="ghcr.io/hushmind3/dominion-rust/mcp-connector";
        hex(hash,64)&&(same_repository||renamed_connector)
    });
    if m.schema_version!=1||!text_ok(&m.name,120)||!text_ok(&m.description,1500)||!text_ok(&m.version,80)
        ||!valid_path||!valid_image(&m.image)
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
fn package_id(repo:&str,image:&str)->String{
    let prefix=format!("ghcr.io/{}",repo.to_ascii_lowercase());
    let path=image.split('@').next().unwrap_or("");
    if path==prefix{install_id(repo)}else{install_id(path.trim_start_matches("ghcr.io/"))}
}
fn ref_ok(value:&str)->bool{!value.is_empty()&&value.len()<=120&&!value.contains("..")&&value.split('/').all(|p|!p.is_empty()&&!p.starts_with('.')&&p.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.')))}
// Catalog entries are discovery metadata only. Installation always fetches
// and validates the immutable package manifest, never trusting this index.
const INDEX_URL:&str="https://raw.githubusercontent.com/hushmind3/dominion/branch3/extensions/catalog.json";
const INDEX_SEED:&str=include_str!("../../../extensions/catalog.json");
fn index_cards(index:&Value,q:&str)->Result<Vec<Value>,String>{
    if index["schemaVersion"]!=1{return Err("Unsupported extension index".into())}
    let rows=index["packages"].as_array().ok_or("Invalid extension index")?;
    if rows.len()>200{return Err("Extension index too large".into())}
    let terms=q.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    let mut seen=std::collections::HashSet::new();
    let mut items=Vec::new();
    for p in rows{
        let Some(repo)=p["repository"].as_str().filter(|s|repository_ok(s))else{continue};
        let Some(source_ref)=p["sourceRef"].as_str().filter(|s|ref_ok(s))else{continue};
        let Some(name)=p["name"].as_str().filter(|s|text_ok(s,120))else{continue};
        let Some(description)=p["description"].as_str().filter(|s|text_ok(s,1500))else{continue};
        let keywords=p["keywords"].as_array().into_iter().flatten().take(32).filter_map(Value::as_str).filter(|s|text_ok(s,120)).collect::<Vec<_>>().join(" ");
        let haystack=format!("{name} {description} {repo} {keywords}").to_lowercase();
        if !terms.iter().all(|term|haystack.contains(term))||!seen.insert((repo.to_ascii_lowercase(),source_ref.to_owned())){continue}
        items.push(json!({"repository":repo,"sourceRef":source_ref,"name":name,"description":description,
            "url":format!("https://github.com/{repo}"),"indexed":true,"detailsLoaded":false,"manifest":null}));
    }
    Ok(items)
}
async fn indexed(client:&reqwest::Client,q:&str)->(Vec<Value>,bool){
    let live=async{
        let url=reqwest::Url::parse(INDEX_URL).map_err(|e|e.to_string())?;
        index_cards(&read(client,url,256*1024).await?,q)
    };
    if let Ok(Ok(items))=tokio::time::timeout(Duration::from_secs(4),live).await{return (items,false)}
    let seed:Value=serde_json::from_str(INDEX_SEED).expect("bundled index JSON");
    (index_cards(&seed,q).unwrap_or_default(),true)
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
fn card(row:&Value)->Option<Value>{
    let repo=row["full_name"].as_str().filter(|s|repository_ok(s))?;
    if row["private"]!=false{return None}
    Some(json!({"repository":repo,"description":row["description"].as_str().unwrap_or("").chars().take(1000).collect::<String>(),
        "stars":row["stargazers_count"].as_u64().unwrap_or(0),"url":format!("https://github.com/{repo}"),
        "manifest":null,"detailsLoaded":false}))
}
fn release_asset(repo:&str,asset:&Value)->Option<Value>{
    let name=asset["name"].as_str().filter(|s|text_ok(s,250))?;
    let url=reqwest::Url::parse(asset["browser_download_url"].as_str()?).ok()?;
    if url.scheme()!="https"||url.host_str()!=Some("github.com")||!url.username().is_empty()
        ||url.password().is_some()||url.port().is_some()||!url.path().to_ascii_lowercase().starts_with(&format!("/{}/releases/download/",repo.to_ascii_lowercase())){return None}
    Some(json!({"name":name,"url":url.as_str(),"size":asset["size"].as_u64()}))
}
async fn resolve(client:&reqwest::Client,repo:&str,requested_ref:Option<&str>)->Result<Value,String>{
    if !repository_ok(repo){return Err("Invalid repository".into())}
    let row=read(client,github_url(&format!("repos/{repo}"))?,512*1024).await?;
    if row["full_name"].as_str().is_none_or(|s|!s.eq_ignore_ascii_case(repo)){return Err("Repository identity changed".into())}
    let mut item=card(&row).ok_or("Only public repositories are supported")?;
    if requested_ref.is_some_and(|r|!ref_ok(r)){return Err("Invalid package source ref".into())}
    let branch=requested_ref.or_else(||row["default_branch"].as_str()).filter(|s|!s.is_empty()&&s.len()<200).ok_or("No default branch")?;
    item["sourceRef"]=json!(branch);
    let mut url=github_url(&format!("repos/{repo}/commits/"))?;
    url.path_segments_mut().map_err(|_|"Invalid branch URL")?.pop_if_empty().push(branch);
    let commit=read(client,url,4*1024*1024).await?["sha"].as_str().filter(|s|hex(s,40)).ok_or("Invalid commit")?.to_owned();
    item["commit"]=json!(commit);
    item["sourceDownload"]=json!(format!("https://github.com/{repo}/archive/{commit}.zip"));
    item["detailsLoaded"]=json!(true);
    match load(client,repo,&commit).await{
        Ok(m)=>{item["manifestDigest"]=json!(digest(&m));item["installId"]=json!(package_id(repo,&m.image));item["manifest"]=json!(m); }
        Err(e)=>{item["manifestError"]=json!(e);}
    }
    if requested_ref.is_some_and(|r|Some(r)!=row["default_branch"].as_str()){item["assets"]=json!([]);return Ok(item)}
    match read(client,github_url(&format!("repos/{repo}/releases/latest"))?,2*1024*1024).await{
        Ok(r)=>{
            item["releaseTag"]=r["tag_name"].clone();
            item["assets"]=json!(r["assets"].as_array().into_iter().flatten().filter_map(|a|release_asset(repo,a)).take(40).collect::<Vec<_>>());
        }
        Err(e)=>{item["assets"]=json!([]);if !e.contains("HTTP 404"){item["releaseError"]=json!(e);}}
    }
    Ok(item)
}
#[derive(Deserialize)]
pub struct Details{repository:String,#[serde(rename="sourceRef")] source_ref:Option<String>}
pub async fn details(Query(input):Query<Details>)->Response{
    let result=async{resolve(&client()?,input.repository.trim(),input.source_ref.as_deref()).await}.await;
    match result{Ok(v)=>Json(v).into_response(),Err(e)=>error(StatusCode::BAD_GATEWAY,&e)}
}
pub async fn search(Query(input):Query<Search>)->Response{
    let result:Result<Value,String>=async{
        let q=input.q.as_deref().unwrap_or("").trim();
        if q.len()>160||q.chars().any(char::is_control){return Err("Invalid search".into())}
        let client=client()?;
        let repo=q.strip_prefix("https://github.com/").unwrap_or(q).trim_end_matches('/');
        if let Some((name,source_ref))=repo.split_once('@'){
            if !repository_ok(name)||!ref_ok(source_ref){return Err("Invalid repository or branch".into())}
            return Ok(json!({"items":[resolve(&client,name,Some(source_ref)).await?],"errors":[]}))
        }
        if repository_ok(repo){return Ok(json!({"items":[resolve(&client,repo,None).await?],"errors":[]}))}
        let safe=q.replace(['"','\\',':']," ");
        let words=safe.split_whitespace().map(|w|format!("\"{w}\"")).collect::<Vec<_>>().join(" ");
        let query=if words.is_empty(){"topic:dominion-plugin archived:false".into()}else{format!("archived:false {words}")};
        let mut url=github_url("search/repositories")?;
        url.query_pairs_mut().append_pair("q",&query).append_pair("per_page","30").append_pair("sort","stars").append_pair("order","desc");
        let ((mut items,index_cached),remote)=tokio::join!(indexed(&client,q),read(&client,url,4*1024*1024));
        let mut errors=Vec::new();
        match remote{
            Ok(rows)=>match rows["items"].as_array(){
                Some(rows)=>{
                    // A repository may publish several packages on distinct refs.
                    // Keep indexed packages and suppress only generic duplicate repo cards.
                    let known=items.iter().filter_map(|p|p["repository"].as_str().map(str::to_lowercase)).collect::<std::collections::HashSet<_>>();
                    for item in &mut items {
                        if let Some(row)=rows.iter().find(|r|r["full_name"].as_str().zip(item["repository"].as_str()).is_some_and(|(a,b)|a.eq_ignore_ascii_case(b))){
                            item["stars"]=row["stargazers_count"].clone();
                        }
                    }
                    items.extend(rows.iter().filter_map(card).filter(|p|!known.contains(&p["repository"].as_str().unwrap_or("").to_lowercase())));
                    items.sort_by(|a,b| (b["indexed"]==true).cmp(&(a["indexed"]==true)).then_with(||b["stars"].as_u64().cmp(&a["stars"].as_u64())));
                }
                None=>errors.push(json!({"repository":"GitHub","message":"Invalid search response"})),
            },
            Err(e)=>errors.push(json!({"repository":"GitHub","message":e})),
        }
        Ok(json!({"items":items,"errors":errors,"indexCached":index_cached}))
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
    super::install(State(state),Json(Install{id:package_id(&input.repository,&m.image),name:m.name,image:m.image,network:m.network,port:m.port,confirmed:true})).await
}
pub async fn update(State(state):State<AppState>,Json(input):Json<UpdateRequest>)->Response{
    if !input.confirmed||!repository_ok(&input.repository)||!hex(&input.commit,40)||!hex(&input.manifest_digest,64)||!super::versions::immutable(&input.expected_image_id){
        return error(StatusCode::BAD_REQUEST,"업데이트할 패키지와 현재 버전을 확인하세요.")
    }
    let m=match async{load(&client()?,&input.repository,&input.commit).await}.await{
        Ok(m)=>m,Err(e)=>return error(StatusCode::BAD_GATEWAY,&e)
    };
    if digest(&m)!=input.manifest_digest{return error(StatusCode::CONFLICT,"검토한 패키지 내용이 변경되었습니다.")}
    let next=Install{id:package_id(&input.repository,&m.image),name:m.name,image:m.image,network:m.network,port:m.port,confirmed:true};
    super::versions::update_market(state,next,&input.expected_image_id).await
}
#[cfg(test)]
mod tests{
    use super::*;
    fn sample()->Value{json!({"schemaVersion":1,"name":"Example","description":"Example independent app","version":"1.0.0",
        "image":format!("ghcr.io/example/app@sha256:{}","a".repeat(64)),"network":true,"port":8080})}
    #[test]
    fn catalog_search_uses_names_and_keywords_without_trusting_install_metadata(){
        let index=json!({"schemaVersion":1,"packages":[
            {"name":"Example connector","description":"Independent provider","repository":"example/app","sourceRef":"plugins","keywords":["도구","MCP"],"image":"untrusted"},
            {"name":"Example assistant","description":"Conversation","repository":"example/app","sourceRef":"voice","keywords":["음성"]},
            {"name":"Bad","description":"Invalid","repository":"example/../bad","sourceRef":"main"},
            {"name":"Duplicate","description":"Duplicate","repository":"EXAMPLE/APP","sourceRef":"plugins"}
        ]});
        let all=index_cards(&index,"").unwrap();assert_eq!(all.len(),2);
        let found=index_cards(&index,"mcp 도구").unwrap();assert_eq!(found.len(),1);
        assert_eq!(found[0]["sourceRef"],"plugins");assert_eq!(found[0]["manifest"],Value::Null);
        assert!(found[0].get("image").is_none());
        assert!(index_cards(&index,"unrelated").unwrap().is_empty());
    }
    #[test]
    fn repository_rename_preserves_only_the_existing_digest_pinned_connector(){
        let mut manifest=sample();
        manifest["image"]=json!(format!("ghcr.io/hushmind3/dominion-rust/mcp-connector@sha256:{}","a".repeat(64)));
        assert!(parse("hushmind3/dominion",manifest.clone()).is_ok());
        assert!(parse("other/dominion",manifest.clone()).is_err());
        assert!(parse("hushmind3/another-project",manifest.clone()).is_err());
        for image in ["ghcr.io/hushmind3/dominion-rust/mcp-connector-evil", "ghcr.io/hushmind3/dominion-rust/other"]{
            manifest["image"]=json!(format!("{image}@sha256:{}","a".repeat(64)));
            assert!(parse("hushmind3/dominion",manifest.clone()).is_err());
        }
        manifest["image"]=json!("ghcr.io/hushmind3/dominion-rust/mcp-connector:latest");
        assert!(parse("hushmind3/dominion",manifest).is_err());
    }
    #[test]
    fn only_same_repository_digest_pinned_declarative_package(){
        let m=parse("Example/App",sample()).unwrap();assert_eq!(digest(&m).len(),64);
        assert_eq!(install_id("Example/App"),install_id("example/app"));
        assert!(parse("other/app",sample()).is_err());
        let mut child=sample();child["image"]=json!(format!("ghcr.io/example/app/connector@sha256:{}","a".repeat(64)));
        let child=parse("example/app",child).unwrap();assert_ne!(package_id("example/app",&child.image),install_id("example/app"));
        let mut impostor=sample();impostor["image"]=json!(format!("ghcr.io/example/app-other@sha256:{}","a".repeat(64)));assert!(parse("example/app",impostor).is_err());
        assert!(ref_ok("release/channel"));assert!(!ref_ok("../other"));
        for (field,value) in [("image",json!("ghcr.io/example/app:latest")),("schemaVersion",json!(2)),("network",json!(false)),("command",json!(["shell"]))]{
            let mut v=sample();v[field]=value;assert!(parse("example/app",v).is_err());
        }
        assert!(!repository_ok("a/../b"));assert!(!repository_ok("a/b?ref=x"));
    }
}
