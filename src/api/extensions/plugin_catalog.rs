//! Read-only package discovery. Search results are not install manifests.
//! Never execute an upstream command or treat a library as a trusted plugin.
use axum::{extract::Query,http::StatusCode,response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use std::time::Duration;

#[derive(Deserialize)]
pub struct Search {q:String,source:Option<String>}
const SOURCES:[&str;6]=["github","npm","pypi","crates","huggingface","docker"];
fn clean(value:&Value,max:usize)->String{
    value.as_str().unwrap_or("").chars().take(max).collect()
}
fn valid_name(s:&str,slash:bool)->bool{
    !s.is_empty()&&s.len()<=214&&s.bytes().all(|b|
        b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'|b'.')||
        (slash&&matches!(b,b'/'|b'@')))
        &&!s.split('/').any(|part|part.is_empty()||part=="."||part=="..")
}
fn endpoint(source:&str,q:&str)->Result<Option<reqwest::Url>,String>{
    let url=match source{
        "github"=>"https://api.github.com/search/repositories",
        "npm"=>"https://registry.npmjs.org/-/v1/search",
        "crates"=>"https://crates.io/api/v1/crates",
        "huggingface"=>"https://huggingface.co/api/spaces",
        "pypi"=>{
            // PyPI retired its search API. Exact project lookup only;
            // never scrape the HTML search page or silently fake results.
            if !valid_name(q,false){return Ok(None);}
            let mut u=reqwest::Url::parse("https://pypi.org/pypi/").map_err(|e|e.to_string())?;
            u.path_segments_mut().map_err(|_|"Invalid PyPI URL")?.pop_if_empty().push(q).push("json");
            return Ok(Some(u));
        }
        _=>return Err("Unsupported registry".into())
    };
    let mut u=reqwest::Url::parse(url).map_err(|e|e.to_string())?;
    match source{
        "github"=>{u.query_pairs_mut().append_pair("q",q).append_pair("per_page","20");}
        "npm"=>{u.query_pairs_mut().append_pair("text",q).append_pair("size","20");}
        "crates"=>{u.query_pairs_mut().append_pair("q",q).append_pair("per_page","20");}
        "huggingface"=>{u.query_pairs_mut().append_pair("search",q).append_pair("limit","20");}
        _=>{}
    }
    Ok(Some(u))
}
fn project(source:&str,raw:&Value)->Result<Vec<Value>,String>{
    let rows=match source{
        "github"=>raw["items"].as_array(),
        "npm"=>raw["objects"].as_array(),
        "crates"=>raw["crates"].as_array(),
        "huggingface"=>raw.as_array(),
        "pypi"=>None,
        _=>return Err("Unsupported registry".into())
    };
    let objects:Vec<&Value>=if source=="pypi"{
        if !raw["info"].is_object(){return Err("Invalid PyPI response".into());}
        vec![&raw["info"]]
    }else{rows.ok_or("Invalid registry response")?.iter().take(20).collect()};
    Ok(objects.into_iter().filter_map(|row|{
        let r=if source=="npm"{&row["package"]}else{row};
        let name=match source{"github"=>r["full_name"].as_str()?,"huggingface"=>r["id"].as_str()?,_=>r["name"].as_str()?};
        if !valid_name(name,matches!(source,"github"|"npm"|"huggingface")){return None;}
        if matches!(source,"github"|"huggingface")&&(r["private"]!=false||name.split('/').count()!=2){return None;}
        let (kind,version,description,license,url)=match source{
            "github"=>("repository",Value::Null,clean(&r["description"],500),clean(&r["license"]["spdx_id"],100),format!("https://github.com/{name}")),
            "npm"=>("package",r["version"].clone(),clean(&r["description"],500),clean(&r["license"],100),format!("https://www.npmjs.com/package/{name}")),
            "pypi"=>("package",r["version"].clone(),clean(&r["summary"],500),clean(&r["license_expression"],100),format!("https://pypi.org/project/{name}/")),
            "crates"=>("package",r["newest_version"].clone(),clean(&r["description"],500),String::new(),format!("https://crates.io/crates/{name}")),
            "huggingface"=>("repository",Value::Null,clean(&r["sdk"],100),String::new(),format!("https://huggingface.co/spaces/{name}")),
            _=>return None
        };
        Some(json!({"id":format!("{source}:{name}"),"source":source,"name":name,
            "kind":kind,"version":version.as_str().map(|s|s.chars().take(100).collect::<String>()),
            "description":description,"license":license,"url":url,
            "compatibility":"unverified"}))
    }).collect())
}
async fn lookup(client:&reqwest::Client,source:&str,q:&str)->Result<Vec<Value>,String>{
    if source=="docker"{return crate::api::plugins::discover_images(q).await;}
    let Some(url)=endpoint(source,q)? else{return Ok(vec![]);};
    let response=client.get(url).send().await.map_err(|e|e.to_string())?;
    if source=="pypi"&&response.status()==StatusCode::NOT_FOUND{return Ok(vec![]);}
    if !response.status().is_success(){return Err(format!("Registry HTTP {}",response.status()));}
    let raw=crate::integrations::network::read_bounded_json(response,4*1024*1024,"package registry").await?;
    project(source,&raw)
}
pub async fn search(Query(query):Query<Search>)->Response{
    let q=query.q.trim();
    let source=query.source.as_deref().unwrap_or("all");
    if q.len()<2||q.len()>160||q.chars().any(char::is_control)||
        (source!="all"&&!SOURCES.contains(&source)){
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"Invalid search query or registry"}))).into_response();
    }
    let client=match reqwest::Client::builder().user_agent("DOMINION-plugin-discovery/1")
        .timeout(Duration::from_secs(12)).redirect(reqwest::redirect::Policy::none()).build(){
        Ok(client)=>client,
        Err(_)=>return (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":"Registry client unavailable"}))).into_response()
    };
    let selected=SOURCES.into_iter().filter(|s|source=="all"||*s==source);
    let results=futures_util::future::join_all(selected.map(|s|{
        let client=&client;
        async move{(s,lookup(client,s,q).await)}
    })).await;
    let mut items=Vec::new();
    let mut errors=Vec::new();
    for (s,result) in results{
        match result{
            Ok(rows)=>items.extend(rows),
            Err(message)=>errors.push(json!({"source":s,"message":message}))
        }
    }
    Json(json!({"items":items,"errors":errors,"pypiMode":"exact_name","searchOnly":true})).into_response()
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn query_cannot_redirect_or_inject_registry_parameters(){
        let u=endpoint("github","foo&per_page=99").unwrap().unwrap();
        assert_eq!(u.host_str(),Some("api.github.com"));
        assert_eq!(u.query_pairs().find(|(k,_)|k=="q").unwrap().1,"foo&per_page=99");
        assert!(endpoint("pypi","../secret").unwrap().is_none());
        assert_eq!(endpoint("pypi","pipecat-ai").unwrap().unwrap().path(),"/pypi/pipecat-ai/json");
    }
    #[test]
    fn external_metadata_never_grants_install_or_execution(){
        let rows=project("npm",&json!({"objects":[{"package":{"name":"@a/b","version":"1.0","description":"speech","url":"javascript:alert(1)","install":"run"}}]})).unwrap();
        assert_eq!(rows[0]["compatibility"],"unverified");
        assert_eq!(rows[0]["url"],"https://www.npmjs.com/package/@a/b");
        assert!(rows[0].get("install").is_none());
        assert!(project("github",&json!({"items":[{"full_name":"a/b","private":true}]})).unwrap().is_empty());
        assert!(project("crates",&json!({"unexpected":[]})).is_err());
    }
}
