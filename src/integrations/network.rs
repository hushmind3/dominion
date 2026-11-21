//! Optional web-search and public HTTP transport. Network isolation and
//! redirect/DNS SSRF admission move with the transport, not into the executor.
use serde_json::{json,Value};
use std::time::Duration;

pub(crate) const HTTP_FETCH_LIMIT: usize = 1024 * 1024;
const HTTP_REDIRECT_LIMIT: usize = 5;

/// Reject non-public destinations even when URL parsing normalizes unusual
/// IPv4 literals. The same check runs again after every redirect and DNS
/// resolution; no configured proxy can bypass the pinned address.
pub(crate) fn public_http_ip(ip: std::net::IpAddr) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    match ip {
        IpAddr::V4(v4) => {
            let n=v4.octets();
            !v4.is_private() && !v4.is_loopback() && !v4.is_link_local()
                && !v4.is_multicast() && !v4.is_broadcast() && !v4.is_unspecified()
                && !v4.is_documentation()
                && n[0]!=0 && n[0]<224
                && !(n[0]==100 && (64..=127).contains(&n[1]))
                && !(n[0]==192 && n[1]==0 && n[2]==0)
                && !(n[0]==192 && n[1]==88 && n[2]==99)
                && !(n[0]==198 && (18..=19).contains(&n[1]))
            && v4!=Ipv4Addr::new(255,255,255,255)
        }
        IpAddr::V6(v6) => {
            if let Some(mapped)=v6.to_ipv4_mapped() {
                return public_http_ip(IpAddr::V4(mapped));
            }
            let segments=v6.segments();
            // Permit only global-unicast space. This excludes NAT64 and
            // IPv4-compatible encodings; explicitly reject 6to4, IETF
            // protocol assignments and the current IPv6 documentation block.
            let global_unicast=segments[0]&0xe000==0x2000;
            let protocol_assignment=segments[0]==0x2001&&segments[1]<0x0200;
            let six_to_four=segments[0]==0x2002;
            let documentation=segments[0]==0x3fff&&segments[1]<0x1000;
            global_unicast&&!protocol_assignment&&!six_to_four&&!documentation
                &&!v6.is_loopback() && !v6.is_unspecified() && !v6.is_multicast()
                && !v6.is_unique_local() && !v6.is_unicast_link_local()
                && !(segments[0]==0x2001 && segments[1]==0x0db8)
        }
    }
}

pub(crate) fn public_http_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed=reqwest::Url::parse(url).map_err(|error|error.to_string())?;
    if !matches!(parsed.scheme(),"http"|"https") {
        return Err("http_fetch는 공개 HTTP(S) URL만 허용합니다.".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("http_fetch URL에 사용자 인증 정보를 넣을 수 없습니다.".into());
    }
    let host=parsed.host_str().ok_or("URL 호스트가 없습니다.")?;
    let name=host.trim_matches(|ch|ch=='['||ch==']').trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name=="localhost" || name.ends_with(".localhost")
        || name.ends_with(".local") || name.ends_with(".internal")
        || name.ends_with(".lan") || name.ends_with(".home.arpa")
        || name.ends_with(".invalid") || name.ends_with(".test") {
        return Err("내부 네트워크 호스트는 http_fetch에서 사용할 수 없습니다.".into());
    }
    if let Ok(ip)=name.parse::<std::net::IpAddr>() {
        if !public_http_ip(ip) {
            return Err("내부 또는 예약된 IP로의 http_fetch 요청을 차단했습니다.".into());
        }
    }
    Ok(parsed)
}

pub(crate) fn append_http_chunk(output:&mut Vec<u8>,chunk:&[u8])->bool {
    let room=HTTP_FETCH_LIMIT.saturating_sub(output.len());
    output.extend_from_slice(&chunk[..chunk.len().min(room)]);
    chunk.len()>room
}

pub(crate) async fn http_fetch(url: &str) -> Result<Value, String> {
    let mut current=public_http_url(url)?;
    for redirect in 0..=HTTP_REDIRECT_LIMIT {
        let hostname=current.host_str().ok_or("URL 호스트가 없습니다.")?
            .trim_matches(|ch|ch=='['||ch==']').to_string();
        let port=current.port_or_known_default().ok_or("URL 포트가 없습니다.")?;
        let client_builder=reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30));
        // Resolve exactly once and pin that address for this request. Every
        // resolved address must be public to prevent mixed DNS answer bypasses.
        let client_builder=if hostname.parse::<std::net::IpAddr>().is_ok() {
            client_builder
        } else {
            let resolved=tokio::net::lookup_host((hostname.as_str(),port))
                .await.map_err(|error|format!("공개 주소 DNS 조회 실패: {error}"))?
                .collect::<Vec<_>>();
            if resolved.is_empty() || resolved.iter().any(|addr|!public_http_ip(addr.ip())) {
                return Err("DNS가 사설 또는 예약된 네트워크 주소를 반환했습니다.".into());
            }
            client_builder.resolve(&hostname,resolved[0])
        };
        let response=client_builder.build().map_err(|error|error.to_string())?
            .get(current.clone())
            .header("User-Agent","DOMINION/0.2 (+rust)")
            .send().await.map_err(|error|error.to_string())?;
        if response.status().is_redirection() {
            if redirect==HTTP_REDIRECT_LIMIT {
                return Err("http_fetch 리다이렉션 상한을 초과했습니다.".into());
            }
            let location=response.headers().get(reqwest::header::LOCATION)
                .and_then(|value|value.to_str().ok())
                .ok_or("리다이렉션 Location 헤더가 없습니다.")?;
            let next=current.join(location).map_err(|error|error.to_string())?;
            current=public_http_url(next.as_str())?;
            continue;
        }
        let status=response.status().as_u16();
        let content_type=response.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|value|value.to_str().ok()).unwrap_or("").to_string();
        let mut response=response;
        let mut bytes=Vec::new();
        let mut truncated=false;
        while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())? {
            if append_http_chunk(&mut bytes,&chunk) {
                truncated=true;
                break;
            }
        }
        let body=String::from_utf8_lossy(&bytes);
        let text=if content_type.contains("html") {strip_html(&body)} else {body.into_owned()};
        let clipped=text.chars().take(60_000).collect::<String>();
        truncated|=clipped.len()!=text.len();
        return Ok(json!({
            "status":status,"contentType":content_type,"text":clipped,
            "truncated":truncated,"maxResponseBytes":HTTP_FETCH_LIMIT,
            "redirects":redirect
        }));
    }
    Err("http_fetch 리다이렉션 상한을 초과했습니다.".into())
}

pub(crate) async fn web_search(query: &str) -> Result<Value, String> {
    let response = reqwest::Client::new()
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .timeout(Duration::from_secs(25))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let mut response=response;
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())? {
        if append_http_chunk(&mut bytes,&chunk) {
            return Err("web_search 응답이 1 MiB 크기 제한을 초과했습니다.".into());
        }
    }
    let html=String::from_utf8_lossy(&bytes);
    let mut results = Vec::new();
    for chunk in html.split("result__a").skip(1).take(10) {
        let Some(href_start) = chunk.find("href=\"") else {
            continue;
        };
        let rest = &chunk[href_start + 6..];
        let Some(href_end) = rest.find('"') else {
            continue;
        };
        let href = decode_ddg(&rest[..href_end]);
        let title = rest[href_end..]
            .find('>')
            .map(|start| {
                let tail = &rest[href_end + start + 1..];
                let end = tail.find("</a>").unwrap_or(tail.len().min(200));
                strip_html(&tail[..end]).trim().to_string()
            })
            .unwrap_or_default();
        if !href.is_empty() && !title.is_empty() {
            results.push(json!({ "title": title, "url": href }));
        }
    }
    Ok(json!({ "query": query, "results": results }))
}

fn decode_ddg(raw: &str) -> String {
    let href = raw
        .strip_prefix("//")
        .map(|rest| format!("https://{rest}"))
        .unwrap_or_else(|| raw.to_string());
    if let Some(index) = href.find("uddg=") {
        let value = &href[index + 5..];
        let value = value.split('&').next().unwrap_or(value);
        return percent_decode(value);
    }
    href
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}


fn tool(name:&str,description:&str,parameters:Value)->Value {
 json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}
pub fn definitions()->Vec<Value>{
 let object=|properties:Value,required:Vec<&str>|json!({"type":"object","properties":properties,
    "required":required,"additionalProperties":false});
 let text=|description:&str|json!({"type":"string","description":description});
 vec![
        tool("web_search", "Search the public web for current information", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("http_fetch", "Fetch a URL and return its readable text content", object(json!({"url": text("HTTP or HTTPS URL")}), vec!["url"])),
 ]
}
pub async fn execute(name:&str,args:&Value)->Option<Result<Value,String>>{
 match name {
  "web_search"=>Some(async{let query=args["query"].as_str().ok_or("Missing argument: query")?;
    web_search(query).await}.await),
  "http_fetch"=>Some(async{let url=args["url"].as_str().ok_or("Missing argument: url")?;
    http_fetch(url).await}.await),
  _=>None,
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn network_tools_are_optional_registry_entries(){
  let names=definitions().into_iter().map(|item|item["function"]["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
  assert_eq!(names,vec!["web_search","http_fetch"]);
  assert!(public_http_url("http://127.0.0.1:4317/secret").is_err());
 }
}
