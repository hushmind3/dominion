//! Durable, zero-LLM market signal extraction.
//! money_signal keeps the source; money_event is a bounded event row. The
//! asymmetry researcher never reads article bodies or sends them to a model.
use crate::{engine::jobs::JobContext,state::AppState};
use serde_json::{json,Value};
use std::collections::HashSet;

fn field<'a>(v:&'a Value,key:&str)->&'a str{v[key].as_str().unwrap_or("")}
fn limit(text:&str,n:usize)->String{text.trim().chars().take(n).collect()}
fn detect_region(content:&str)->&'static str {
    let lower=content.to_lowercase();
    let regions:&[(&str,&[&str])]=&[
        ("CN",&["중국","china","chinese","中国","中國","大陆"]),
        ("KR",&["한국","korea","korean","대한민국","韩国","韓國"]),
        ("JP",&["일본","japan","japanese","日本","日本国"]),
        ("US",&["미국","american","united states","u.s.","美國","美国"])
    ];
    let present=regions.iter().filter(|(_,terms)|terms.iter().any(|term|lower.contains(*term)))
        .map(|(region,_)|*region).collect::<Vec<_>>();
    if present.len()==1 {present[0]} else {"UNKNOWN"}
}
fn source_kind(source:&str)->&'static str{
    match source {
        "GitHub emerging repositories"|"Hugging Face trending models"=>"platform_listing",
        name if name.contains("policy")||name.contains("규제")=>"policy_feed",
        name if name.contains("HN")=>"developer_community",
        name if name=="Buyer-cited public document"=>"buyer_document",
        name if name.contains("procurement")||name.contains("buyer")
            ||name.contains("tender")||name.contains("vendor")=>"buyer_news",
        _=>"news"
    }
}
fn entity_key(row:&Value,topics:&[String])->(String,&'static str){
    let source=field(row,"sourceName");
    let title=field(row,"title");
    if source=="GitHub emerging repositories"{
        if let Some(value)=title.strip_prefix("New repository: "){
            let entity=limit(value,180).to_lowercase();
            if entity.contains('/')&&!entity.contains(' '){
                return (format!("repo:{entity}"),"specific");
            }
        }
    }
    if source=="Hugging Face trending models"{
        if let Some(value)=title.strip_prefix("Open model: "){
            let entity=limit(value,180).to_lowercase();
            if entity.contains('/')&&!entity.contains(' '){
                return (format!("model:{entity}"),"specific");
            }
        }
    }
    // Generic headlines do not identify an exact product/firm reliably. A
    // sector-level join is a research cue only, never evidence of same item.
    if let Some(topic)=topics.first(){
        return (format!("sector:{topic}"),"sector_proxy");
    }
    (String::new(),"unknown")
}
/// A source row can produce a bounded number of event facets. This initial
/// fast path is deterministic; future small local extractors can replace it
/// without changing event IDs, lineage, state transitions or job isolation.
pub fn extract(source:&Value)->Vec<Value>{
    let id=field(source,"id");
    let url=field(source,"url");
    if id.is_empty()||!url.starts_with("https://"){return Vec::new();}
    let facets=crate::engine::money_intelligence::signal_facets(source);
    let topics=facets["topics"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_owned).take(5).collect::<Vec<_>>();
    let actions=facets["changeTags"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).take(3).collect::<Vec<_>>();
    if topics.is_empty()||actions.is_empty(){return Vec::new();}
    // Region must be in the SOURCE TEXT; publisher location != market location.
    let text=format!("{} {} {}",field(source,"title"),field(source,"description"),
        field(source,"articleExcerpt").chars().take(650).collect::<String>());
    let region=detect_region(&text);
    let (entity,precision)=entity_key(source,&topics);
    if entity.is_empty(){return Vec::new();}
    let quote=limit(&format!("{} {}",field(source,"title"),field(source,"description")),310);
    let mut output=Vec::new();
    let mut seen=HashSet::new();
    for action in actions {
        if !seen.insert(action){continue;}
        let event_id=format!("event:{id}:{action}");
        output.push(json!({"id":event_id,"sourceId":id,
            "url":url,"title":limit(field(source,"title"),220),
            "sourceName":field(source,"sourceName"),
            "sourceKind":source_kind(field(source,"sourceName")),
            "marketRegion":region,"regionConfidence":if region=="UNKNOWN"{"unknown"}else{"lexical"},
            "entityKey":entity,"entityPrecision":precision,
            "topics":topics.clone(),"changeTags":[action],"action":action,
            "effectiveDate":source["publishedAt"],
            "publishedAt":source["publishedAt"],"capturedAt":source["capturedAt"],
            "evidenceQuote":quote,"status":"new",
            "verification":"source_text_lexical_event_not_economic_fact",
            "createdAt":chrono::Utc::now().to_rfc3339()}));
    }
    output
}
pub async fn run(context:&JobContext,_job:&Value)->Result<Value,String>{
    let source_batch=context.state.store.money_unprocessed_sources(96)?;
    let mut events=Vec::new();let mut processed=Vec::new();
    for source in &source_batch{
        let Some(id)=source["id"].as_str()else{continue;};
        processed.push(id.to_string());
        events.extend(extract(source));
    }
    let inserted=context.state.store.money_fold_sources(&processed,&events)?;
    // Indexed SQL joins on the small event working set: no corpus text read.
    let pairs=context.state.store.money_pair_events(24)?;
    let new_pairs=context.state.store.money_store_asymmetries(&pairs)?;
    let result=json!({"id":format!("fold:{}",context.job_id),
        "jobId":context.job_id,"sourceCount":processed.len(),"extractedEvents":events.len(),
        "newEvents":inserted,"newAsymmetries":new_pairs,
        "pendingAsymmetries":context.state.store.money_pending_asymmetries(20)?.len(),
        "createdAt":chrono::Utc::now().to_rfc3339()});
    context.state.store.put("money_fold_report",&result)
        .ok_or("money fold report save failed")?;
    Ok(result)
}
pub fn schedule(state:&AppState){
    let has_work=state.store.money_unprocessed_source_count().unwrap_or(0)>0;
    if !has_work{return;}
    let bucket=chrono::Utc::now().timestamp().div_euclid(120);
    let id=format!("money:fold:{bucket}");
    if state.store.get("job",&id).is_none(){
        let _=crate::engine::jobs::enqueue_once(state,&id,"money_fold",
            json!({"bucket":bucket,"mode":"deterministic_event_folding"}));
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn folds_event_without_guessing_market_or_sales(){
        let source=json!({"id":"source1","url":"https://example.org/x",
            "sourceName":"Chinese newspaper",
            "title":"中国机器人芯片降价","description":"hardware cost cuts",
            "publishedAt":"2026-11-16T00:00:00Z"});
        let out=extract(&source);
        assert!(!out.is_empty());
        assert_eq!(out[0]["marketRegion"],"CN");
        assert_eq!(out[0]["entityPrecision"],"sector_proxy");
        assert_eq!(out[0]["status"],"new");
    }
    #[test] fn source_to_event_to_pending_pair_is_indexed_and_restart_safe(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let input=vec![
            json!({"id":"cn1","title":"中国机器人芯片降价","url":"https://example.org/cn",
                "description":"중국 가격 하락", "sourceName":"CN supplier journal",
                "publishedAt":"2026-11-16T00:00:00Z","capturedAt":"2026-11-16T00:05:00Z"}),
            json!({"id":"kr1","title":"한국 반도체 설비 비용 상승","url":"https://example.org/kr",
                "description":"한국 가격 인상","sourceName":"KR industry journal",
                "publishedAt":"2026-11-16T00:00:00Z","capturedAt":"2026-11-16T00:05:00Z"})
        ];
        assert_eq!(store.money_ingest_signals(&input).unwrap(),2);
        assert_eq!(store.money_unprocessed_source_count().unwrap(),2);
        let sources=store.money_unprocessed_sources(96).unwrap();
        let events=sources.iter().flat_map(extract).collect::<Vec<_>>();
        assert!(events.len()>=2);
        assert!(store.money_fold_sources(
            &sources.iter().filter_map(|s|s["id"].as_str().map(str::to_string))
                .collect::<Vec<_>>(),&events).unwrap()>=2);
        assert_eq!(store.money_unprocessed_source_count().unwrap(),0);
        assert!(store.money_unprocessed_sources(96).unwrap().is_empty());
        let pairs=store.money_pair_events(24).unwrap();
        assert_eq!(pairs.len(),1);
        assert_eq!(pairs[0]["axis"],"regional");
        assert_eq!(pairs[0]["absenceProved"],false);
        assert_eq!(store.money_store_asymmetries(&pairs).unwrap(),1);
        assert_eq!(store.money_store_asymmetries(&pairs).unwrap(),0);
        assert_eq!(store.money_pending_asymmetries(20).unwrap().len(),1);
        let tested=crate::engine::money_asymmetry::validate_pair(
            &pairs[0],&pairs[0],
            &store.get("money_event",pairs[0]["sourceA"].as_str().unwrap()).unwrap(),
            &store.get("money_event",pairs[0]["sourceB"].as_str().unwrap()).unwrap()
        );
        assert!(tested.is_ok(),"{tested:?}");
        let id=pairs[0]["id"].as_str().unwrap().to_string();
        // A Money discovery commit must belong to a live host-owned job.
        // The stored job and the domain report are committed under the same
        // SQLite lock, preventing late writes after user cancellation.
        store.put("job",&json!({"id":"moneyjob1","kind":"money_discover",
            "status":"running"})).unwrap();
        let report=json!({"id":"report:1","jobId":"moneyjob1","campaignId":"c1"});
        assert!(store.money_commit_discovery(&report,&[],&[],&[],&[id]).unwrap());
        assert!(store.money_pending_asymmetries(20).unwrap().is_empty());
    }
    #[test] fn ambiguous_geographic_mention_is_unknown(){
        assert_eq!(detect_region("China and Korea joint market"),"UNKNOWN");
        assert_eq!(detect_region("소프트웨어 출시"),"UNKNOWN");
    }
}
