//! Zero-model-token, scheduled market intelligence collector. Public RSS and
//! article metadata enter a persistent, de-duplicated SQLite source corpus.
//! Fetch uses the engine's existing SSRF-safe HTTP tool.
use crate::{engine::jobs::JobContext,state::AppState};
use serde_json::{json,Value};
use sha2::{Sha256,Digest};

const SOURCES:&[(&str,&str)] = &[
    ("HN new technology","https://hnrss.org/newest?points=25"),
    ("BBC business","https://feeds.bbci.co.uk/news/business/rss.xml"),
    ("TechCrunch","https://techcrunch.com/feed/"),
    ("MIT Technology Review","https://www.technologyreview.com/feed/"),
    ("South China Morning Post","https://www.scmp.com/rss/36/feed"),
    ("Yonhap economy","https://www.yna.co.kr/rss/economy.xml"),
    ("Yonhap industry","https://www.yna.co.kr/rss/industry.xml"),
    ("Yonhap market","https://www.yna.co.kr/rss/market.xml"),
    ("Bloomberg technology","https://feeds.bloomberg.com/technology/news.rss"),
    ("Reuters business via Google News","https://news.google.com/rss/search?q=when:24h+site:reuters.com/business&hl=en-US&gl=US&ceid=US:en"),
    ("Manufacturing supply change","https://news.google.com/rss/search?q=manufacturing+robotics+cost+price+when:7d&hl=en-US&gl=US&ceid=US:en"),
    ("Korea procurement and buyer signals","https://news.google.com/rss/search?q=%EA%B5%AC%EB%A7%A4+%EB%B0%9C%EC%A3%BC+%EC%9E%85%EC%B0%B0+when:7d&hl=ko&gl=KR&ceid=KR:ko"),
    ("Korean customer complaints and requests","https://news.google.com/rss/search?q=%EA%B3%A0%EA%B0%9D+%EB%B6%88%EB%A7%8C+%EA%B0%9C%EC%84%A0+%EC%9A%94%EC%B2%AD+when:7d&hl=ko&gl=KR&ceid=KR:ko"),
    ("Chinese buyer procurement intentions","https://news.google.com/rss/search?q=%E9%87%87%E8%B4%AD%E6%84%8F%E5%90%91+%E6%8B%9B%E6%A0%87+when:7d&hl=zh-CN&gl=CN&ceid=CN:zh-Hans"),
    ("Chinese manufacturing supply prices","https://news.google.com/rss/search?q=%E5%B7%A5%E5%8E%82+%E9%99%8D%E6%9C%AC+%E4%BE%9B%E5%BA%94%E9%93%BE+when:7d&hl=zh-CN&gl=CN&ceid=CN:zh-Hans"),
    ("Japanese buyer requests and tenders","https://news.google.com/rss/search?q=%E5%85%AC%E5%8B%9F+%E8%AA%BF%E9%81%94+%E5%85%A5%E6%9C%AD+when:7d&hl=ja&gl=JP&ceid=JP:ja"),
    ("EU public buyer tenders","https://news.google.com/rss/search?q=public+buyer+tender+RFP+when:7d&hl=en-GB&gl=GB&ceid=GB:en"),
    ("Global vendor and service requests","https://news.google.com/rss/search?q=%22request+for+proposals%22+%22service%22+when:7d&hl=en-US&gl=US&ceid=US:en"),
    ("Ask HN customer problems","https://hnrss.org/ask?points=10"),
    ("Show HN new supplier tools","https://hnrss.org/show?points=10"),
    ("Korea public tender notices","https://news.google.com/rss/search?q=%EC%9E%85%EC%B0%B0%EA%B3%B5%EA%B3%A0+when:3d&hl=ko&gl=KR&ceid=KR:ko"),
    ("Global RFQ and invitation to tender","https://news.google.com/rss/search?q=%22request+for+quotation%22+OR+%22invitation+to+tender%22+when:7d&hl=en-US&gl=US&ceid=US:en"),
    ("Contract awards and buyer decisions","https://news.google.com/rss/search?q=%22contract+award%22+OR+%22supplier+selected%22+when:7d&hl=en-US&gl=US&ceid=US:en"),
];
const MAX_ITEMS_PER_SOURCE:usize=18;
const SCRAPES_PER_CYCLE:usize=4;

fn digest(input:&str)->String{format!("{:x}",Sha256::digest(input.as_bytes()))}
fn clean_text(value:&str,max:usize)->String{
    let value=value.replace("<![CDATA[","").replace("]]>","")
        .replace("&amp;","&").replace("&quot;","\"")
        .replace("&apos;","'").replace("&lt;","<")
        .replace("&gt;",">").replace("&#39;","'");
    let mut out=String::new();
    let mut in_tag=false;
    for ch in value.chars(){
        match ch {
            '<'=>in_tag=true,
            '>'=>in_tag=false,
            c if !in_tag=>out.push(c),
            _=>{},
        }
        if out.chars().count()>max{break}
    }
    out.trim().chars().take(max).collect()
}
fn element<'a>(text:&'a str,tag:&str)->Option<&'a str>{
    let open=format!("<{tag}");
    let begin=text.find(&open)?;
    let after=&text[begin+open.len()..];
    if !after.starts_with(|c:char|c==' '||c=='\t'||c=='>'||c=='\n'){
        return None;
    }
    let gt=after.find('>')?;
    let close=format!("</{tag}>");
    let end=after[gt+1..].find(&close)?;
    Some(&after[gt+1..gt+1+end])
}
fn link(item:&str)->String{
    if let Some(link)=element(item,"link"){
        let text=clean_text(link,2000);
        if text.starts_with("https://")||text.starts_with("http://"){return text}
    }
    if let Some(start)=item.find("<link "){
        let tag=&item[start..item[start..].find('>').map(|n|start+n).unwrap_or(item.len())];
        for quote in ['"', '\'']{
            let attr=format!("href={quote}");
            if let Some(start)=tag.find(&attr){
                let rest=&tag[start+attr.len()..];
                if let Some(end)=rest.find(quote){return clean_text(&rest[..end],2000);}
            }
        }
    }
    String::new()
}
fn entries(feed:&str)->Vec<&str>{
    let mut output=Vec::new();
    let (begin,end)=if feed.contains("<item") {("<item","</item>")}else{("<entry","</entry>")};
    let mut rest=feed;
    while let Some(start)=rest.find(begin){
        let item=&rest[start..];
        let Some(end_idx)=item.find(end)else{break;};
        output.push(&item[..end_idx+end.len()]);
        if output.len()>=MAX_ITEMS_PER_SOURCE{break;}
        rest=&item[end_idx+end.len()..];
    }
    output
}
fn parse(feed:&str,source:&str)->Vec<Value>{
    let timestamp=chrono::Utc::now().to_rfc3339();
    entries(feed).into_iter().filter_map(|item|{
        let title=element(item,"title").map(|x|clean_text(x,250)).unwrap_or_default();
        let url=link(item);
        if title.chars().count()<8||!(url.starts_with("https://")||url.starts_with("http://")){
            return None;
        }
        let desc=element(item,"description").or_else(||element(item,"summary"))
            .map(|x|clean_text(x,500)).unwrap_or_default();
        let published=element(item,"pubDate").or_else(||element(item,"published"))
            .or_else(||element(item,"updated")).map(|x|clean_text(x,100)).unwrap_or_default();
        Some(crate::engine::money_intelligence::enrich_signal(json!({"id":digest(&url),"title":title,"url":url,"sourceName":source,
            "description":desc,"publishedAt":published,
            "capturedAt":timestamp,"contentStatus":"headline_observed",
            "articleExcerpt":"","verification":"unverified_market_observation"})))
    }).collect()
}

fn normalized_title(title:&str)->String{
    title.to_lowercase().chars().filter(|c|c.is_alphanumeric()).collect()
}

/// The same story arrives from several feeds with different URLs. Drop the
/// repeats before they reach the corpus so tokens are never spent on them.
fn dedupe_titles(signals:Vec<Value>,history:&[Value])->Vec<Value>{
    let mut seen=history.iter().filter_map(|s|s["title"].as_str())
        .map(normalized_title).collect::<std::collections::HashSet<_>>();
    signals.into_iter().filter(|s|s["title"].as_str()
        .map(|title|seen.insert(normalized_title(title))).unwrap_or(false)).collect()
}

/// Include freshly captured records first; matching must happen in this
/// acquisition cycle rather than waiting for the next 30-minute RSS slot.
fn research_window(new_signals:&[Value],history:&[Value])->Vec<Value>{
    let mut seen=std::collections::HashSet::new();
    new_signals.iter().chain(history.iter()).filter(|row|{
        row["id"].as_str().is_some_and(|id|seen.insert(id.to_owned()))
    }).take(120).cloned().collect()
}
fn needs_article_fetch(item:&Value,stored:Option<&Value>)->bool{
    if item["url"].as_str().is_none_or(|url|!url.starts_with("https://")&&!url.starts_with("http://")){
        return false;
    }
    // Headline-only records still need a real source page.
    !stored.is_some_and(|row|row["articleExcerpt"].as_str()
        .is_some_and(|text|text.chars().count()>100))
}
pub async fn run(context:&JobContext,job:&Value)->Result<Value,String>{
    let bucket=job["input"]["bucket"].as_i64().unwrap_or(0);
    let start=((bucket.rem_euclid(SOURCES.len() as i64)) as usize)%SOURCES.len();
    let chosen=(0..3).map(|i|SOURCES[(start+i)%SOURCES.len()]).collect::<Vec<_>>();
    let mut signals=Vec::new();
    let mut reports=Vec::new();
    let mut scraped=0;
    let mut scrape_attempts=0;
    for (name,url) in chosen{
        match crate::engine::tools::http_fetch(url).await{
            Ok(body) if body["status"].as_u64()==Some(200)=>{
                let mut items=parse(body["text"].as_str().unwrap_or(""),name);
                let count=items.len();
                for item in items.iter_mut(){
                    if scrape_attempts>=SCRAPES_PER_CYCLE{break;}
                    let stored=item["id"].as_str()
                        .and_then(|id|context.state.store.get("money_signal",id));
                    if !needs_article_fetch(item,stored.as_ref()){continue;}
                    let Some(article_url)=item["url"].as_str().map(str::to_owned)else{continue;};
                    scrape_attempts+=1;
                    if let Ok(article)=crate::engine::tools::http_fetch(&article_url).await{
                        if article["status"].as_u64()==Some(200){
                            let text=article["text"].as_str().unwrap_or("").chars().take(2000).collect::<String>();
                            if text.chars().count()>100{
                                item["articleExcerpt"]=json!(text);
                                item["contentStatus"]=json!("page_fetched");
                                scraped+=1;
                            }
                        }
                    }
                }
                reports.push(json!({"name":name,"feedUrl":url,"found":count}));
                signals.extend(items.into_iter().map(crate::engine::money_intelligence::enrich_signal));
            },
            Ok(body)=>reports.push(json!({"name":name,"feedUrl":url,"error":format!("HTTP {}",body["status"])})),
            Err(err)=>reports.push(json!({"name":name,"feedUrl":url,"error":err.chars().take(200).collect::<String>()}))
        }
    }
    // New observations can connect with last month's old signals: perform
    // bounded cross-market matching after ingestion, without model tokens.
    let history=context.state.store.money_recent_signals(240).unwrap_or_default();
    let signals=dedupe_titles(signals,&history);
    let found=signals.len();
    // All providers failing is a real acquisition failure. Successfully
    // visited feeds with no *new* articles are healthy, not failed jobs;
    // repeatedly treating this as an error would exhaust autopilot retries.
    if !reports.iter().any(|r|r["found"].as_u64().is_some()){
        return Err(format!("시장 출처 응답을 가져오지 못했습니다: {}",json!(reports)));
    }
    let (added,linked)=if found==0{
        (0,0)
    }else{
        let window=research_window(&signals,&history);
        let added=context.state.store.money_ingest_signals(&signals)?;
        let hypotheses=crate::engine::money_mechanism::discover(&window,30);
        let linked=context.state.store.money_store_tensions(&hypotheses)?;
        (added,linked)
    };
    let summary=json!({"id":format!("market:{}",context.job_id),
        "jobId":context.job_id,"found":found,"new":added,"newCrossMarketQuestions":linked,
        "scrapedPages":scraped,"sources":reports,"capturedAt":chrono::Utc::now().to_rfc3339()});
    context.state.store.put("money_ingest_report",&summary)
        .ok_or("시장 수집 기록을 저장하지 못했습니다.")?;
    context.state.bus.record("money_market",&summary);
    Ok(summary)
}

/// Source scanning does not require a campaign or an LLM subscription.
/// One durable job per 30-minute UTC slot, including after process restart.
pub fn schedule(state:&AppState){
    let bucket=chrono::Utc::now().timestamp().div_euclid(1800);
    let id=format!("money:source:{bucket}");
    let input=json!({"bucket":bucket,"source":"public_market_rss"});
    if state.store.get("job",&id).is_none(){
        let _=crate::engine::jobs::enqueue_once(state,&id,"money_ingest",input);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn extracts_rss_and_deduplicates_stable_urls(){
        let rss=r#"<?xml version="1.0"?><rss><channel><item><title>New AI tool &amp; hardware cost</title>
        <link>https://example.com/article?id=4&amp;region=KR</link>
        <description><![CDATA[<p>Lower manufacturing costs</p>]]></description>
        <pubDate>Mon, 16 Nov 2026 05:00:00 GMT</pubDate></item></channel></rss>"#;
        let items=parse(rss,"test");
        assert_eq!(items.len(),1);
        assert_eq!(items[0]["url"],"https://example.com/article?id=4&region=KR");
        assert_eq!(items[0]["description"],"Lower manufacturing costs");
        assert_eq!(items[0]["id"],digest("https://example.com/article?id=4&region=KR"));
    }
    #[test] fn repeated_headline_from_another_feed_is_dropped(){
        let history=vec![json!({"title":"Supplier raises prices 20%"})];
        let batch=vec![
            json!({"title":"supplier raises prices 20%!"}),
            json!({"title":"New public buyer tender"}),
        ];
        let kept=dedupe_titles(batch,&history);
        assert_eq!(kept.len(),1);
        assert_eq!(kept[0]["title"],"New public buyer tender");
    }
    #[test] fn new_buyer_document_joins_existing_supply_in_same_cycle(){
        let supplier=json!({"id":"s","sourceName":"hardware source",
            "url":"https://supply.example/s","title":"New hardware cost falls",
            "topics":["hardware"],"changeTags":["cost_drop"],"contentStatus":"page_fetched"});
        let buyer=json!({"id":"b","sourceName":"construction buyer",
            "url":"https://buyer.example/b","title":"New construction purchase request",
            "topics":["construction"],"changeTags":["buyer_request"],"contentStatus":"page_fetched"});
        // The old pipeline analyzed history only, silently dropping new
        // demand until the next scheduled crawl.
        assert!(crate::engine::money_mechanism::discover(&[supplier.clone()],5).is_empty());
        let joined=research_window(&[buyer],&[supplier]);
        assert_eq!(joined.len(),2);
        let matches=crate::engine::money_mechanism::discover(&joined,5);
        assert_eq!(matches.len(),1);
        assert_eq!(matches[0]["buyerSignalId"],"b");
        assert_eq!(matches[0]["purchaseEvidence"],false);
    }
    #[test] fn research_window_deduplicates_ids_and_remains_bounded(){
        let new=json!({"id":"a","sourceName":"fresh"});
        let old=json!({"id":"a","sourceName":"old"});
        let history=(0..150).map(|i|json!({"id":format!("history-{i}")})).collect::<Vec<_>>();
        let mut records=vec![old];records.extend(history);
        let window=research_window(&[new],&records);
        assert_eq!(window.len(),120);
        assert_eq!(window[0]["sourceName"],"fresh");
        assert_eq!(window.iter().filter(|x|x["id"]=="a").count(),1);
    }
    #[test] fn fetched_sources_are_skipped_but_headlines_can_be_enriched(){
        let item=json!({"url":"https://example.org/story"});
        assert!(needs_article_fetch(&item,None));
        let headline=json!({"articleExcerpt":"","contentStatus":"headline_observed"});
        assert!(needs_article_fetch(&item,Some(&headline)));
        let fetched=json!({"articleExcerpt":"article ".repeat(30),"contentStatus":"page_fetched"});
        assert!(!needs_article_fetch(&item,Some(&fetched)));
        assert!(!needs_article_fetch(&json!({"url":"javascript:alert(1)"}),None));
    }
    #[test] fn rotating_sources_span_multiple_markets_and_buyers(){
        assert!(SOURCES.len()>=20);
        for needle in ["Korean customer complaints","Chinese buyer",
            "Japanese buyer","Global vendor","Ask HN"]{
            assert!(SOURCES.iter().any(|(name,_)|name.contains(needle)));
        }
        assert!(SOURCES.iter().all(|(_,url)|url.starts_with("https://")));
    }
    #[test] fn atom_links_are_supported(){
        let feed=r#"<feed><entry><title>Supply change in batteries</title>
        <link href="https://example.com/atom"/><published>2026-11-16T00:00:00Z</published></entry></feed>"#;
        assert_eq!(parse(feed,"test").len(),1);
    }
}
