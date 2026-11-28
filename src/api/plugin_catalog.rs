//! Public registry metadata only. Catalog reads never install or execute a plugin.
use axum::{extract::{Query,State},http::{header,HeaderValue,StatusCode},response::{IntoResponse,Response},Json};
use crate::state::AppState;
use serde_json::Value;
use std::{collections::HashMap,sync::{Mutex,OnceLock},time::{Duration,Instant}};
use super::response::error;

#[derive(Clone)]
struct Cached { until:Instant,value:Value }
#[derive(Default)]
struct Cache { entries:HashMap<String,Cached>,blocked_until:Option<Instant> }
fn cache()->&'static Mutex<Cache>{static CACHE:OnceLock<Mutex<Cache>>=OnceLock::new();CACHE.get_or_init(||Mutex::new(Cache::default()))}
fn endpoint(query:&HashMap<String,String>)->Result<reqwest::Url,String>{
    let mut url=reqwest::Url::parse("https://clawhub.ai/api/v1/").map_err(|e|e.to_string())?;
    let view=query.get("view").map(String::as_str).unwrap_or("overview");
    let path=match view {
        "overview"=>"plugins/overview", "categories"=>"plugins/categories",
        "list"=>"plugins", "search"=>"plugins/search",
        "detail"=>{
            let id=query.get("id").ok_or("Missing plugin name")?;
            if id.len()>200||id.split('/').any(|part|part.is_empty()||part=="."||part=="..")
                ||!id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'@'|b'/'|b'-'|b'_'|b'.')){
                return Err("Invalid plugin name".into());
            }
            let mut segments=url.path_segments_mut().map_err(|_|"Invalid registry URL")?;
            segments.pop_if_empty().push("packages").push(id).push("detail");
            drop(segments);
            if let Some(version)=query.get("version").filter(|s|s.len()<=120){url.query_pairs_mut().append_pair("version",version);}
            return Ok(url);
        }
        _=>return Err("Unknown catalog view".into()),
    };
    url.set_path(&format!("/api/v1/{path}"));
    if matches!(view,"list"|"search"){
        url.query_pairs_mut().append_pair("limit","36");
        for key in ["q","category","cursor","sort","featured"]{
            if let Some(value)=query.get(key){
                if value.len()>4096{return Err("Catalog query is too long".into());}
                if key=="sort"&&!matches!(value.as_str(),"recommended"|"downloads"|"updated"|"trending"){return Err("Invalid catalog sort".into());}
                url.query_pairs_mut().append_pair(key,value);
            }
        }
    }
    Ok(url)
}
pub async fn catalog(State(_state):State<AppState>,Query(query):Query<HashMap<String,String>>)->Response{
    let endpoint=match endpoint(&query){Ok(url)=>url,Err(message)=>return error(StatusCode::BAD_REQUEST,&message)};
    let key=endpoint.to_string();
    {
        let locked=cache().lock().unwrap();
        if let Some(entry)=locked.entries.get(&key).filter(|entry|entry.until>Instant::now()){
            return Json(entry.value.clone()).into_response();
        }
        if let Some(until)=locked.blocked_until.filter(|until|*until>Instant::now()){
            return throttled(until.saturating_duration_since(Instant::now()).as_secs().max(1));
        }
    }
    let client=match reqwest::Client::builder().timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::none()).build(){Ok(client)=>client,Err(_)=>return error(StatusCode::BAD_GATEWAY,"Cannot open plugin registry")};
    let mut response=match client.get(endpoint).header("Accept","application/json").send().await{Ok(response)=>response,Err(_)=>return error(StatusCode::BAD_GATEWAY,"플러그인 레지스트리에 연결하지 못했습니다. 잠시 후 다시 시도하세요.")};
    if response.status()==reqwest::StatusCode::TOO_MANY_REQUESTS{
        let seconds=response.headers().get(header::RETRY_AFTER).and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok()).unwrap_or(60).clamp(1,3600);
        cache().lock().unwrap().blocked_until=Some(Instant::now()+Duration::from_secs(seconds));
        return throttled(seconds);
    }
    if !response.status().is_success(){
        return error(if response.status()==reqwest::StatusCode::NOT_FOUND{StatusCode::NOT_FOUND}else{StatusCode::BAD_GATEWAY},"레지스트리에서 플러그인 정보를 가져오지 못했습니다.");
    }
    let mut body=Vec::new();
    loop{match response.chunk().await{
        Ok(Some(chunk))=>{if body.len()+chunk.len()>4*1024*1024{return error(StatusCode::BAD_GATEWAY,"Registry metadata exceeds the supported size");}body.extend_from_slice(&chunk);}
        Ok(None)=>break,Err(_)=>return error(StatusCode::BAD_GATEWAY,"Registry response was interrupted"),
    }}
    let value:Value=match serde_json::from_slice(&body){Ok(value)=>value,Err(_)=>return error(StatusCode::BAD_GATEWAY,"Invalid plugin registry response")};
    let mut locked=cache().lock().unwrap();
    locked.entries.retain(|_,entry|entry.until>Instant::now());
    if locked.entries.len()>=128{locked.entries.clear();}
    locked.entries.insert(key,Cached{until:Instant::now()+Duration::from_secs(300),value:value.clone()});
    Json(value).into_response()
}
fn throttled(seconds:u64)->Response{
    let mut response=error(StatusCode::TOO_MANY_REQUESTS,&format!("플러그인 레지스트리 요청 한도입니다. {seconds}초 후 다시 시도하세요."));
    if let Ok(value)=HeaderValue::from_str(&seconds.to_string()){response.headers_mut().insert(header::RETRY_AFTER,value);}
    response
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn artwork_uses_registry_sources_and_excludes_private_networks(){
        let metadata=serde_json::json!({"items":[{"icon":"/api/v1/skill-icons/example","ownerImage":"https://cdn.publisher.example/icon.png","readme":"https://untrusted.example/x"}]});
        assert!(registry_artwork(&metadata,"https://cdn.publisher.example/icon.png"));
        assert!(registry_artwork(&metadata,"https://clawhub.ai/api/v1/skill-icons/example"));
        assert!(!registry_artwork(&metadata,"https://untrusted.example/x"));
        for ip in ["127.0.0.1","10.0.0.1","169.254.169.254","100.64.0.1","::1","::ffff:127.0.0.1","fc00::1"]{assert!(!public_artwork_ip(ip.parse().unwrap()),"{ip}");}
        assert!(public_artwork_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn untrusted_queries_cannot_select_a_host_or_escape_metadata_routes(){
        for name in ["https://evil.test","../download","@owner/../download","x?url=evil","x#fragment"]{
            assert!(endpoint(&HashMap::from([("view".into(),"detail".into()),("id".into(),name.into())])).is_err());
        }
        let url=endpoint(&HashMap::from([("view".into(),"detail".into()),("id".into(),"@owner/plugin".into())])).unwrap();
        assert_eq!(url.host_str(),Some("clawhub.ai"));assert!(url.path().ends_with("/detail"));assert!(url.path().contains("%2F"));
        assert!(endpoint(&HashMap::from([("view".into(),"download".into())])).is_err());
    }
}

/// Artwork sources come from registry metadata, rather than a maintained list
/// of publisher brands. DNS is pinned to public addresses for each redirect.
fn registry_artwork(value:&Value,source:&str)->bool{
    match value{
        Value::Object(fields)=>fields.iter().any(|(key,value)|matches!(key.as_str(),"icon"|"ownerImage")&&value.as_str().is_some_and(|candidate|candidate==source||reqwest::Url::parse("https://clawhub.ai").ok().and_then(|base|base.join(candidate).ok()).is_some_and(|url|url.as_str()==source))||registry_artwork(value,source)),
        Value::Array(items)=>items.iter().any(|item|registry_artwork(item,source)),_=>false,
    }
}
fn public_artwork_ip(ip:std::net::IpAddr)->bool{
    match ip{
        std::net::IpAddr::V4(ip)=>!ip.is_private()&&!ip.is_loopback()&&!ip.is_link_local()&&!ip.is_unspecified()&&!ip.is_multicast()&&!ip.is_broadcast()&&!ip.is_documentation()&&ip.octets()[0]!=0&&ip.octets()[0]<224&&!(ip.octets()[0]==100&&(64..128).contains(&ip.octets()[1]))&&!(ip.octets()[0]==198&&matches!(ip.octets()[1],18|19)),
        std::net::IpAddr::V6(ip)=>ip.to_ipv4_mapped().map(|ip|public_artwork_ip(ip.into())).unwrap_or_else(||ip.segments()[0]&0xe000==0x2000&&!(ip.segments()[0]==0x2001&&ip.segments()[1]==0x0db8)),
    }
}
pub async fn artwork(Query(query):Query<HashMap<String,String>>)->Response{
    fn allowed(url:&reqwest::Url)->bool{url.scheme()=="https"&&url.port_or_known_default()==Some(443)&&url.username().is_empty()&&url.password().is_none()&&url.host_str().is_some()}
    let Some(source)=query.get("src")else{return error(StatusCode::BAD_REQUEST,"Missing artwork source")};
    if !cache().lock().unwrap().entries.values().any(|entry|registry_artwork(&entry.value,source)){return error(StatusCode::BAD_REQUEST,"Artwork must come from the plugin registry")}
    let mut url=match reqwest::Url::parse(source){Ok(url)if allowed(&url)=>url,_=>return error(StatusCode::BAD_REQUEST,"Unsupported artwork source")};
    let mut fetched=None;
    for _ in 0..4{
        let host=url.host_str().unwrap();
        let addresses=match tokio::time::timeout(Duration::from_secs(5),tokio::net::lookup_host((host,443))).await{
            Ok(Ok(addresses))=>addresses.filter(|address|public_artwork_ip(address.ip())).collect::<Vec<_>>(),_=>return error(StatusCode::BAD_GATEWAY,"Artwork host unavailable"),
        };
        if addresses.is_empty(){return error(StatusCode::BAD_REQUEST,"Unsupported artwork address")}
        let client=match reqwest::Client::builder().no_proxy().resolve_to_addrs(host,&addresses).timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build(){Ok(client)=>client,Err(_)=>return error(StatusCode::BAD_GATEWAY,"Artwork unavailable")};
        let response=match client.get(url.clone()).send().await{Ok(response)=>response,Err(_)=>return error(StatusCode::NOT_FOUND,"Artwork unavailable")};
        if response.status().is_redirection(){
            let next=response.headers().get(header::LOCATION).and_then(|v|v.to_str().ok()).and_then(|v|url.join(v).ok());
            match next{Some(next)if allowed(&next)=>{url=next;continue},_=>return error(StatusCode::BAD_GATEWAY,"Unsupported artwork redirect")}
        }
        if !response.status().is_success(){return error(StatusCode::NOT_FOUND,"Artwork unavailable")}
        fetched=Some(response);break;
    }
    let Some(mut upstream)=fetched else{return error(StatusCode::BAD_GATEWAY,"Too many artwork redirects")};
    let content_type=upstream.headers().get(header::CONTENT_TYPE).and_then(|v|v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").to_string();
    if !matches!(content_type.as_str(),"image/png"|"image/jpeg"|"image/webp"|"image/gif"|"image/svg+xml"|"image/avif"|"image/x-icon"){return error(StatusCode::BAD_GATEWAY,"Unsupported artwork format")}
    let mut bytes=Vec::new();
    loop{match upstream.chunk().await{Ok(Some(chunk))=>{if bytes.len()+chunk.len()>2*1024*1024{return error(StatusCode::BAD_GATEWAY,"Artwork is too large")};bytes.extend_from_slice(&chunk);},Ok(None)=>break,Err(_)=>return error(StatusCode::BAD_GATEWAY,"Artwork download was interrupted")}}
    let mut response=bytes.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE,HeaderValue::from_str(&content_type).unwrap());
    response.headers_mut().insert(header::CACHE_CONTROL,HeaderValue::from_static("private, max-age=86400"));
    response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS,HeaderValue::from_static("nosniff"));
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY,HeaderValue::from_static("default-src 'none'; sandbox"));
    response
}
