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
/// RSS `source` is the publisher, not the RSS search channel.
/// Google News often mirrors publisher articles under a news.google.com URL.
/// Keep the distinction and never treat two search feeds as independent news.
fn publisher_domain(item:&str,article_url:&str)->String{
    let source_tag=item.find("<source ").and_then(|start|{
        let rest=&item[start..];
        rest.find('>').map(|end|&rest[..=end])
    });
    if let Some(tag)=source_tag{
        for quote in ['"', '\'']{
            let needle=format!("url={quote}");
            if let Some(pos)=tag.find(&needle){
                let part=&tag[pos+needle.len()..];
                if let Some(end)=part.find(quote){
                    let candidate=&part[..end];
                    if let Ok(parsed)=reqwest::Url::parse(candidate){
                        if matches!(parsed.scheme(),"http"|"https"){
                            if let Some(host)=parsed.host_str(){
                                return host.trim_start_matches("www.").to_ascii_lowercase();
                            }
                        }
                    }
                }
            }
        }
    }
    if let Ok(parsed)=reqwest::Url::parse(article_url){
        if let Some(host)=parsed.host_str(){
            let host=host.trim_start_matches("www.").to_ascii_lowercase();
            // A Google News feed URL is an aggregator, not a publisher.
            if host!="news.google.com" && host!="google.com" {
                return host;
            }
        }
    }
    String::new()
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
        let publisher_name=element(item,"source")
            .map(|value|clean_text(value,160)).unwrap_or_default();
        let publisher_domain=publisher_domain(item,&url);
        let normalized=normalized_title(&title);
        Some(crate::engine::money_intelligence::enrich_signal(json!({
            "id":digest(&url),"title":title,"url":url,
            "sourceName":source,"publisherName":publisher_name,
            "publisherDomain":publisher_domain,
            "storyClusterId":digest(&normalized),
            "storyRelationship":"unassessed",
            "description":desc,"publishedAt":published,
            "capturedAt":timestamp,"contentStatus":"headline_observed",
            "articleExcerpt":"","verification":"unverified_market_observation"
        })))
    }).collect()
}

fn normalized_title(title:&str)->String{
    title.to_lowercase().chars().filter(|c|c.is_alphanumeric()).collect()
}

/// De-duplicate the exact article URL within this fetch batch, not an entire
/// story across publishers. Two distinct archived URLs may expose different
/// corrections, dates or claims and must remain auditable. Repeated headlines
/// get a stable cluster label, not "independent corroboration" credit.
fn preserve_observations(signals:Vec<Value>,history:&[Value])->(Vec<Value>,usize){
    let mut titles=history.iter()
        .filter_map(|row|row["title"].as_str())
        .map(normalized_title).filter(|name|!name.is_empty())
        .collect::<std::collections::HashSet<_>>();
    let mut urls=std::collections::HashSet::new();
    let mut preserved=Vec::new();
    let mut duplicate_urls=0;
    for mut signal in signals{
        let Some(url)=signal["url"].as_str().filter(|url|!url.is_empty()) else{continue;};
        if !urls.insert(url.to_string()){
            duplicate_urls+=1;
            continue;
        }
        let normalized=normalized_title(signal["title"].as_str().unwrap_or(""));
        if normalized.is_empty(){continue;}
        let previously_seen=!titles.insert(normalized.clone());
        signal["storyClusterId"]=json!(digest(&normalized));
        signal["storyRelationship"]=json!(if previously_seen{
            "matching_headline_requires_independent_source_review"
        }else{"first_seen_headline_in_recent_window"});
        signal["independentConfirmation"]=json!(false);
        preserved.push(signal);
    }
    (preserved,duplicate_urls)
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
    let mut attempted_article_urls=std::collections::HashSet::new();
    let mut fetched_feeds=0;
    for (name,url) in chosen{
        match crate::engine::tools::http_fetch(url).await{
            Ok(body) if body["status"].as_u64()==Some(200)=>{
                fetched_feeds+=1;
                let mut items=parse(body["text"].as_str().unwrap_or(""),name);
                let count=items.len();
                for item in items.iter_mut(){
                    if scrape_attempts>=SCRAPES_PER_CYCLE{break;}
                    let stored=item["id"].as_str()
                        .and_then(|id|context.state.store.get("money_signal",id));
                    if !needs_article_fetch(item,stored.as_ref()){continue;}
                    let Some(article_url)=item["url"].as_str().map(str::to_owned)else{continue;};
                    if !attempted_article_urls.insert(article_url.clone()){continue;}
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
    let history=context.state.store.money_recent_signals(240)?;
    let (signals,duplicate_urls)=preserve_observations(signals,&history);
    let found=signals.len();
    // A successful but empty feed is not a failed job. The owner still gets
    // an auditable ingest report; only a complete feed-fetch failure is fatal.
    if fetched_feeds==0{
        return Err(format!("시장 원문 피드를 가져오지 못했습니다: {}",json!(reports)));
    }
    let repeated_headlines=signals.iter()
        .filter(|row|row["storyRelationship"]==
            "matching_headline_requires_independent_source_review").count();
    let added=context.state.store.money_ingest_signals(&signals)?;
    let hypotheses=crate::engine::money_mechanism::discover(&history,30);
    let linked=context.state.store.money_store_tensions(&hypotheses)?;
    let summary=json!({"id":format!("market:{}",context.job_id),
        "jobId":context.job_id,"found":found,"new":added,"newCrossMarketQuestions":linked,
        "repeatedHeadlines":repeated_headlines,"duplicateUrlsDropped":duplicate_urls,
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
    #[test] fn identical_headlines_from_distinct_urls_preserve_both_archives(){
        let history=vec![json!({"title":"Supplier raises prices 20%"})];
        let batch=vec![
            json!({"title":"supplier raises prices 20%!","url":"https://one.example/story"}),
            json!({"title":"supplier raises prices 20%!","url":"https://two.example/story"}),
            json!({"title":"New public buyer tender","url":"https://three.example/tender"}),
            json!({"title":"New public buyer tender","url":"https://three.example/tender"}),
        ];
        let (kept,duplicate_urls)=preserve_observations(batch,&history);
        assert_eq!(kept.len(),3);
        assert_eq!(duplicate_urls,1);
        assert_eq!(kept[0]["storyClusterId"],kept[1]["storyClusterId"]);
        assert_eq!(kept[0]["independentConfirmation"],false);
        assert_eq!(kept[1]["storyRelationship"],
            "matching_headline_requires_independent_source_review");
        assert_eq!(kept[2]["storyRelationship"],
            "first_seen_headline_in_recent_window");
    }

    #[test] fn google_news_feeds_carry_publisher_identity_not_feed_identity(){
        let rss=r#"<rss><channel><item><title>Supplier technology contract award</title>
          <link>https://news.google.com/rss/articles/abc</link>
          <source url="https://www.reuters.com/business">Reuters</source>
          </item></channel></rss>"#;
        let observed=parse(rss,"EU procurement search");
        assert_eq!(observed.len(),1);
        assert_eq!(observed[0]["publisherName"],"Reuters");
        assert_eq!(observed[0]["publisherDomain"],"reuters.com");
        assert_ne!(observed[0]["storyClusterId"],"");
        let without=r#"<rss><item><title>Technology procurement announcement</title>
          <link>https://news.google.com/rss/articles/unknown</link></item></rss>"#;
        assert_eq!(parse(without,"Feed mirror")[0]["publisherDomain"],"");
    }

    #[test] fn recent_identical_headline_does_not_mask_a_new_source_url(){
        let previous=vec![json!({"title":"Korean service request",
            "url":"https://one.example/item"})];
        let current=vec![json!({"title":"Korean service request",
            "url":"https://other.example/item"})];
        let (observations,duplicates)=preserve_observations(current,&previous);
        assert_eq!(duplicates,0);
        assert_eq!(observations.len(),1);
        assert_eq!(observations[0]["independentConfirmation"],false);
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
