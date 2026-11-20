//! Optional web/HTTP implementation.
use super::*;

fn public_http_ip(ip: std::net::IpAddr) -> bool {
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
            !v6.is_loopback() && !v6.is_unspecified() && !v6.is_multicast()
                && !v6.is_unique_local() && !v6.is_unicast_link_local()
                && !(segments[0]==0x2001 && segments[1]==0x0db8)
                && !(segments[0]==0x2001 && segments[1]==0)
        }
    }
}

pub(super) fn public_http_url(url: &str) -> Result<reqwest::Url, String> {
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

fn append_http_chunk(output:&mut Vec<u8>,chunk:&[u8])->bool {
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

/// The test-only override is constrained to a loopback HTTP endpoint, so
/// a test cannot accidentally query public providers or arbitrary LAN hosts.
fn web_search_test_endpoint()->Option<String>{
    let raw=std::env::var("DOMINION_WEB_SEARCH_TEST_ENDPOINT").ok()?;
    let endpoint=reqwest::Url::parse(&raw).ok()?;
    (endpoint.scheme()=="http"
        && matches!(endpoint.host_str(),Some("127.0.0.1"|"localhost"))
    ).then_some(raw)
}

pub(super) fn parse_ddg_search_results(html:&str)->Vec<Value>{
    let mut results=Vec::new();
    for chunk in html.split("result__a").skip(1).take(12) {
        let Some(href_start)=chunk.find("href=") else{continue;};
        let raw=&chunk[href_start+5..];
        if !raw.starts_with('"') {continue;}
        let rest=&raw[1..];
        let Some(href_end)=rest.find('"') else{continue;};
        let href=decode_ddg(&rest[..href_end]);
        let title=rest[href_end..].find('>').map(|start|{
            let tail=&rest[href_end+start+1..];
            let end=tail.find("</a>").unwrap_or(tail.len().min(200));
            strip_html(&tail[..end])
        }).unwrap_or_default();
        if (href.starts_with("https://")||href.starts_with("http://"))
            && !title.trim().is_empty()
            && !results.iter().any(|r:&Value|r["url"]==href)
        {
            results.push(json!({"title":title,"url":href}));
        }
    }
    results.truncate(10);
    results
}

pub(super) fn parse_bing_rss_search_results(rss:&str)->Vec<Value>{
    let mut results=Vec::new();
    for part in rss.split("<item>").skip(1).take(12) {
        let segment=part.split("</item>").next().unwrap_or("");
        let value=|tag:&str|->Option<String>{
            let start=format!("<{tag}>");
            let end=format!("</{tag}>");
            let part=segment.split_once(&start)?.1.split_once(&end)?.0;
            let cleaned=strip_html(part);
            (!cleaned.is_empty()).then_some(cleaned)
        };
        let (Some(title),Some(url))=(value("title"),value("link")) else {continue};
        if (url.starts_with("https://")||url.starts_with("http://"))
            && !results.iter().any(|r:&Value|r["url"]==url) {
            results.push(json!({"title":title,"url":url}));
        }
    }
    results.truncate(10);
    results
}

pub(super) async fn web_search(query: &str) -> Result<Value, String> {
    let client=reqwest::Client::builder()
        .timeout(Duration::from_secs(18)).build().map_err(|e|e.to_string())?;
    let test_endpoint=web_search_test_endpoint();
    let endpoint=test_endpoint.as_deref()
        .unwrap_or("https://html.duckduckgo.com/html/");
    let first=client.post(endpoint).form(&[("q",query)])
        .header("User-Agent","Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
        .send().await;
    let mut errors=Vec::new();
    let mut results=match first {
        Ok(response) if response.status().is_success()=>{
            parse_ddg_search_results(&response.text().await.map_err(|e|e.to_string())?)
        },
        Ok(response)=>{errors.push(format!("DuckDuckGo HTTP {}",response.status()));Vec::new()},
        Err(error)=>{errors.push(format!("DuckDuckGo: {error}"));Vec::new()},
    };
    // One independent backend prevents a silent "successful" empty result
    // if the HTML scraper changes or DDG challenges this client.
    if results.is_empty() && test_endpoint.is_none() {
        let fallback=client.get("https://www.bing.com/search")
            .query(&[("format","rss"),("q",query)])
            .header("User-Agent","Mozilla/5.0")
            .send().await;
        match fallback {
            Ok(response) if response.status().is_success()=>{
                let rss=response.text().await.map_err(|e|e.to_string())?;
                results=parse_bing_rss_search_results(&rss);
            },
            Ok(response)=>errors.push(format!("Bing RSS HTTP {}",response.status())),
            Err(error)=>errors.push(format!("Bing RSS: {error}")),
        }
    }
    if results.is_empty() {
        return Err(format!("검색 공급자가 검증 가능한 결과를 반환하지 않았습니다: {}",
            errors.join("; ")));
    }
    Ok(json!({"query":query,"results":results,"count":results.len(),
        "source":"public_web_search"}))
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

