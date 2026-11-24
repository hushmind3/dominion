//! Optional public repository discovery. GitHub is only a catalog source,
//! never the runtime, SDK contract, package format or authority provider.
use super::{fetch,validate_url,MAX_INDEX,Manifest};
use serde_json::Value;
use std::collections::BTreeSet;
const PREFIX:&str="github:";
const TOPIC:&str="dominion-extension";
fn valid_component(s:&str)->bool{
    !s.is_empty()&&s.len()<=100&&s!="."&&s!=".."&&s.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'))
}
fn repo_name(raw:&str)->Option<String>{
    let (owner,repo)=raw.split_once('/')?;
    (valid_component(owner)&&valid_component(repo)).then(||format!("{owner}/{repo}").to_ascii_lowercase())
}
pub fn is_source(source:&str)->bool{source.starts_with(PREFIX)}
fn source_repo(source:&str)->Result<String,String>{
    let raw=source.strip_prefix(PREFIX).ok_or("invalid repository source")?;
    let repo=repo_name(raw).ok_or("invalid extension repository")?;
    if source!=format!("{PREFIX}{repo}"){return Err("noncanonical extension repository source".into());}
    Ok(repo)
}
fn search_url(query:&str)->String{
    // Disallow injected GitHub search qualifiers. Preserve ordinary Unicode
    // words; the fixed topic filter is mandatory for all repository searches.
    let words=query.chars().take(120).map(|c|if c.is_alphanumeric()||c=='-'||c=='_'{c}else{' '}).collect::<String>();
    let q=format!("topic:{TOPIC} archived:false {}",words.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut url=reqwest::Url::parse("https://api.github.com/search/repositories").unwrap();
    url.query_pairs_mut().append_pair("q",q.trim()).append_pair("per_page","20");
    url.to_string()
}
fn repositories(data:&Value)->Result<Vec<String>,String>{
    let items=data["items"].as_array().ok_or("repository search returned no items")?;
    let mut seen=BTreeSet::new();
    Ok(items.iter().take(20).filter_map(|row|{
        if row["private"]==true||row["archived"]==true{return None;}
        let repo=repo_name(row["full_name"].as_str()?)?;
        seen.insert(repo.clone()).then(||format!("{PREFIX}{repo}"))
    }).collect())
}
fn parse_manifest(bytes:&[u8])->Result<Manifest,String>{
    let m:Manifest=serde_json::from_slice(bytes).map_err(|_|"repository does not publish a valid dominion-extension.json")?;
    m.validate()?;validate_url(&m.artifact.url)?;Ok(m)
}
/// Re-fetch the published manifest during installation. The app requires
/// exact equality with the manifest explicitly reviewed by the owner.
/// Artifact bytes are separately pinned by SHA-256, not a mutable branch.
pub async fn resolve(source:&str)->Result<Manifest,String>{
    let repo=source_repo(source)?;
    let url=format!("https://raw.githubusercontent.com/{repo}/HEAD/dominion-extension.json");
    parse_manifest(&fetch(&url,MAX_INDEX).await?)
}
pub async fn search(query:&str)->Result<Vec<(String,Result<Manifest,String>)>,String>{
    let data:Value=serde_json::from_slice(&fetch(&search_url(query),MAX_INDEX).await?).map_err(|_|"invalid repository search response")?;
    let sources=repositories(&data)?;
    // At most 20 metadata fetches, no code execution and no credentials.
    let mut pending=tokio::task::JoinSet::new();
    for source in sources{
        pending.spawn(async move{let result=resolve(&source).await;(source,result)});
    }
    let mut out=Vec::new();
    while let Some(joined)=pending.join_next().await{
        if let Ok(row)=joined{out.push(row);}
    }
    out.sort_by(|a,b|a.0.cmp(&b.0));Ok(out)
}
#[cfg(test)]
mod tests{
 use super::*;
 use serde_json::json;
 #[test]fn repository_search_keeps_topic_and_never_accepts_url_or_qualifier_injection(){
    assert!(source_repo("github:owner/repo").is_ok());
    for bad in ["github:owner/../repo","github:https://evil.com","github:owner/repo?token=x","github:owner/%2e%2e","github:owner/.."]{assert!(source_repo(bad).is_err(),"{bad}");}
    let parsed=reqwest::Url::parse(&search_url("topic:other is:private / 업무")).unwrap();
    let q=parsed.query_pairs().find(|(k,_)|k=="q").unwrap().1;
    assert!(q.starts_with("topic:dominion-extension archived:false"));
    assert!(!q.contains("topic:other")&&!q.contains("is:private"));
    let rows=repositories(&json!({"items":[{"full_name":"Owner/Repo"},{"full_name":"owner/repo"},{"full_name":"hidden/repo","private":true},{"full_name":"bad/../repo"}]})).unwrap();
    assert_eq!(rows,vec!["github:owner/repo"]);
 }
}
