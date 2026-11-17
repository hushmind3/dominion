//! Buyer-demand evidence is attached to an archived source, not to a
//! model-authored URL label. Source refresh uses bounded GETs but no secondary model inference.
//!
//! A matching quotation demonstrates only that a stored page said something
//! about procurement. It is NOT a payment, buyer confirmation or open order.
use serde_json::{json,Value};
use sha2::Digest;

fn trimmed(value:&Value,max:usize)->String{
    value.as_str().unwrap_or("").trim().chars().take(max).collect()
}
fn normalized(s:&str)->String{
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}
fn buyer_language(quote:&str)->bool{
    let q=normalized(quote);
    [
        "request for proposal","request for quotation","request for tender",
        "procurement notice","invitation to bid","solicitation","purchase order",
        "budget allocated","budget approval","call for tenders",
        "rfp","rfq","招标","招標","采购","採購","采购意向",
        "入札","調達","発注","발주","조달","입찰",
        "견적 요청","견적요청","구매 요청","구매요청","예산 배정","예산편성"
    ].iter().any(|term|q.contains(term))
}
fn has_quote(source:&Value,quote:&str)->bool{
    let expected=normalized(quote);
    if expected.is_empty(){return false;}
    // RSS headline/description are not proof of text in the fetched document.
    // Prefer the full archived body; fall back only for old excerpt-only rows.
    source["articleBody"].as_str().filter(|s|!s.is_empty())
        .or_else(||source["articleExcerpt"].as_str().filter(|s|!s.is_empty()))
        .is_some_and(|body|normalized(body).contains(&expected))
}
/// Classification deliberately uses an explicit archived page + exact quote.
/// Titles, model-written explanations, URL shapes, aggregator snippets and
/// platform popularity cannot silently become validated customer payments.
pub fn annotate(row:&Value,source:Option<&Value>)->Value{
    let kind=trimmed(&row["type"],60);
    let url=trimmed(&row["url"],1800);
    let claim=trimmed(&row["claim"],800);
    let quote=trimmed(&row["quote"],600);
    let buyer_type=matches!(kind.as_str(),"public_purchase_request"|"official_budget");
    let is_url=url.starts_with("https://")&&!url.contains(char::is_whitespace);
    let fetched=source.is_some_and(|s|s["contentStatus"]=="page_fetched");
    let matched=source.is_some_and(|s|has_quote(s,&quote));
    let status=if !buyer_type {"not_buying_evidence"}
        else if !is_url {"invalid_source_url"}
        else if source.is_none() {"source_not_archived"}
        else if !fetched {"source_not_fetched"}
        else if quote.chars().count()<10 {"missing_source_quote"}
        else if !matched {"source_quote_mismatch"}
        else if !buyer_language(&quote) {"no_buyer_wording_in_quote"}
        else {"source_excerpt_buyer_intent"};
    json!({
        "type":kind,"url":url,"claim":claim,"quote":quote,
        "observationStatus":status,
        "sourceId":source.map(|s|s["id"].clone()).unwrap_or(Value::Null),
        // An evidence annotation refers to one immutable stored revision.
        // Legacy rows without snapshots leave this field explicitly null.
        "sourceRevisionId":source.map(|s|s["sourceRevisionId"].clone()).unwrap_or(Value::Null),
        "sourceCapturedAt":source.map(|s|s["capturedAt"].clone()).unwrap_or(Value::Null),
        "reviewNote":"Archived text match only; not a purchase order or confirmed payment"
    })
}
pub fn buying_signal(row:&Value)->bool{
    matches!(row["type"].as_str(),Some("public_purchase_request"|"official_budget"))
        &&row["observationStatus"]=="source_excerpt_buyer_intent"
}

/// Read-only expansion of model-suggested URLs: at most one bounded GET per
/// distinct URL per job. Failed fetches are cooled down to avoid endlessly
/// revisiting unavailable websites during unattended 27B research.
fn reference_urls(parsed:&Value,max:usize)->Vec<String>{
    let mut seen=std::collections::HashSet::new();
    let mut urls=Vec::new();
    for idea in parsed["opportunities"].as_array().into_iter().flatten().take(12){
        for row in idea["buyerEvidence"].as_array().into_iter().flatten().take(12){
            let url=trimmed(&row["url"],1800);
            if !matches!(row["type"].as_str(),Some("public_purchase_request"|"official_budget"))
                ||!url.starts_with("https://")
                ||url.contains(char::is_whitespace)
                ||trimmed(&row["quote"],600).chars().count()<10
                ||!seen.insert(url.clone()){continue;}
            urls.push(url);
            if urls.len()>=max.min(6){return urls;}
        }
    }
    urls
}
fn already_fetched(source:&Value)->bool{
    source["contentStatus"]=="page_fetched"
        &&source["articleExcerpt"].as_str()
            .is_some_and(|body|body.chars().count()>100)
}
fn captured_source(url:&str,text:&str,at:&str)->Value{
    let host=url.split_once("://").map(|(_,host)|host).unwrap_or("")
        .split('/').next().unwrap_or("").chars().take(100).collect::<String>();
    let id=format!("{:x}",sha2::Sha256::digest(url.as_bytes()));
    json!({"id":id,"url":url,"title":format!("Buyer-cited page: {host}"),
        "description":"Buyer-referenced public page; no demand verified by collection alone",
        "articleExcerpt":text.chars().take(2000).collect::<String>(),
        "articleBody":text.chars().take(16_000).collect::<String>(),
        "sourceName":"Buyer-cited public document",
        "contentStatus":"page_fetched",
        "verification":"direct_document_text_not_confirmed_purchase",
        "publishedAt":Value::Null,"capturedAt":at})
}
/// Import actual readable page text *before* buyer-evidence grading. This is
/// deliberately a limited GET-only pass and never reaches checkout, messaging,
/// contracts, advertising or code changes.
pub async fn capture_referenced_sources(
    state:&crate::state::AppState,parsed:&Value,max:usize
)->Value{
    let mut attempted=0usize;
    let mut imported=0usize;
    let mut cached=0usize;
    let mut cooled_down=0usize;
    let mut failed=0usize;
    for url in reference_urls(parsed,max){
        // An RSS title alone is not enough for evidence classification.
        // Fetch the same URL and let the immutable URL-derived source ID
        // enrich its body without duplicating or replacing its provenance.
        if state.store.money_signal_by_url(&url).ok().flatten()
            .as_ref().is_some_and(already_fetched){
            cached+=1;continue;
        }
        let id=format!("buyer-fetch:{:x}",sha2::Sha256::digest(url.as_bytes()));
        if let Some(previous)=state.store.get("money_buyer_fetch",&id){
            let last=previous["attemptedAt"].as_str()
                .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t|t.with_timezone(&chrono::Utc));
            if last.is_some_and(|t|chrono::Utc::now()-t<chrono::Duration::hours(24)){
                cooled_down+=1;continue;
            }
        }
        attempted+=1;
        let fetched=crate::engine::tools::http_fetch(&url).await;
        let (status,detail)=match fetched{
            Ok(body) if body["status"].as_u64()==Some(200)=>{
                let content=body["text"].as_str().unwrap_or("");
                if content.chars().count()>=100 {
                    let at=chrono::Utc::now().to_rfc3339();
                    let source=captured_source(&url,content,&at);
                    match state.store.money_ingest_signals(&[source]) {
                        Ok(_)=>{imported+=1;("archived","".to_string())},
                        Err(err)=>{failed+=1;("storage_error",err)}
                    }
                }else{failed+=1;("short_page","Too little readable text".to_string())}
            }
            Ok(body)=>{failed+=1;("http_error",format!("HTTP {}",body["status"]))}
            Err(err)=>{failed+=1;("fetch_error",err)}
        };
        let _=state.store.put("money_buyer_fetch",&json!({
            "id":id,"url":url,"status":status,
            "error":detail.chars().take(200).collect::<String>(),
            "attemptedAt":chrono::Utc::now().to_rfc3339()
        }));
    }
    json!({"attempted":attempted,"archived":imported,
        "alreadyArchived":cached,"cooldownSkips":cooled_down,"failed":failed,
        "method":"bounded_public_http_get_no_model_calls"})
}
#[cfg(test)]
mod tests{
    use super::*;
    fn source()->Value{
        json!({"id":"source-1","url":"https://example.org/rfp",
            "title":"Requests and notices",
            "description":"New tender for hospital document services",
            "articleExcerpt":"The procurement notice requests proposals for digital invoice matching.",
            "contentStatus":"page_fetched","capturedAt":"2026-11-16T00:00:00Z"})
    }
    #[test] fn exact_archived_buyer_wording_is_a_cue_not_a_payment(){
        let row=json!({"type":"public_purchase_request","url":"https://example.org/rfp",
            "quote":"procurement notice requests proposals for digital invoice matching",
            "claim":"Hospital purchasing opportunity"});
        let result=annotate(&row,Some(&source()));
        assert!(buying_signal(&result));
        assert_eq!(result["sourceId"],"source-1");
        assert!(result["reviewNote"].as_str().unwrap().contains("not a purchase order"));
    }
    #[test] fn invented_url_or_model_claim_is_not_validated(){
        let row=json!({"type":"official_budget","url":"https://invented.example/rfp",
            "quote":"procurement notice requests proposals for digital invoice matching"});
        assert_eq!(annotate(&row,None)["observationStatus"],"source_not_archived");
        let mut wrong=row.clone();
        wrong["quote"]=json!("Procurement notice with entirely fabricated claims");
        assert_eq!(annotate(&wrong,Some(&source()))["observationStatus"],"source_quote_mismatch");
    }
    #[test] fn headline_only_and_non_buying_text_do_not_earn_bonus(){
        let row=json!({"type":"public_purchase_request","url":"https://example.org/rfp",
            "quote":"procurement notice requests proposals for digital invoice matching"});
        let mut s=source();
        s["contentStatus"]=json!("headline_observed");
        assert_eq!(annotate(&row,Some(&s))["observationStatus"],"source_not_fetched");
        let no_purchase=json!({"type":"official_budget","url":"https://example.org/rfp",
            "quote":"digital invoice matching"});
        assert_eq!(annotate(&no_purchase,Some(&source()))["observationStatus"],"no_buyer_wording_in_quote");
    }
    #[test] fn title_or_feed_description_is_not_cited_article_content(){
        let mut s=source();
        s["title"]=json!("Official procurement notice requests proposals");
        s["description"]=json!("Official procurement notice requests proposals");
        s["articleExcerpt"]=json!("Other text from the actual saved source document.");
        let row=json!({"type":"public_purchase_request",
            "url":"https://example.org/rfp",
            "quote":"procurement notice requests proposals"});
        assert_eq!(annotate(&row,Some(&s))["observationStatus"],"source_quote_mismatch");
    }
    #[test] fn full_body_quote_beyond_excerpt_is_verified_with_revision_id(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let url="https://example.org/long-rfp";
        let body=format!("{}The procurement notice requests proposals for long-term maintenance.",
            "Unrelated background sentences. ".repeat(100));
        let item=captured_source(url,&body,"2026-11-18T00:00:00Z");
        store.money_ingest_signals(&[item]).unwrap();
        let saved=store.money_signal_by_url(url).unwrap().unwrap();
        assert!(saved["articleExcerpt"].as_str().unwrap().chars().count()<=2000);
        assert!(saved["articleBody"].as_str().unwrap().chars().count()>2000);
        let row=json!({"type":"public_purchase_request","url":url,
            "quote":"procurement notice requests proposals for long-term maintenance"});
        let result=annotate(&row,Some(&saved));
        assert_eq!(result["observationStatus"],"source_excerpt_buyer_intent");
        let revision_id=result["sourceRevisionId"].as_str().expect("bound source revision");
        assert_eq!(revision_id,saved["sourceRevisionId"]);
        assert_eq!(store.money_source_revisions(saved["id"].as_str().unwrap(),10)
            .unwrap()[0]["id"],revision_id);
    }
    #[test] fn model_assumption_cannot_change_evidence_class(){
        let row=json!({"type":"assumption","url":"https://example.org/rfp",
            "quote":"procurement notice requests proposals for digital invoice matching"});
        assert_eq!(annotate(&row,Some(&source()))["observationStatus"],"not_buying_evidence");
    }
    #[test] fn archived_headline_must_be_enriched_before_buying_claim_is_scored(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let url="https://example.org/notice";
        let id=format!("{:x}",sha2::Sha256::digest(url.as_bytes()));
        let old=json!({"id":id,"url":url,"title":"New purchase request",
            "description":"Purchasing announcement","articleExcerpt":"",
            "sourceName":"Buyer bulletin","contentStatus":"headline_observed",
            "capturedAt":"2026-11-16T00:00:00Z"});
        assert_eq!(store.money_ingest_signals(&[old]).unwrap(),1);
        let stored=store.money_signal_by_url(url).unwrap().unwrap();
        assert!(!already_fetched(&stored));
        let refreshed=captured_source(url,&"The procurement notice requests supplier quotations for tools and equipment. ".repeat(3),
            "2026-11-17T00:00:00Z");
        assert_eq!(store.money_ingest_signals(&[refreshed]).unwrap(),0);
        let current=store.money_signal_by_url(url).unwrap().unwrap();
        assert!(already_fetched(&current));
        assert_eq!(current["sourceName"],"Buyer bulletin");
        assert_eq!(current["capturedAt"],"2026-11-16T00:00:00Z");
        assert_eq!(store.money_corpus_stats().unwrap()["money_signal"],1);
    }
    #[test] fn referenced_sources_have_strict_cap_and_do_not_scan_every_claim(){
        let a=json!({"opportunities":[{"buyerEvidence":[
            {"type":"public_purchase_request","quote":"Public tender document 2026","url":"https://a.test/item"},
            {"type":"official_budget","quote":"Public tender document 2026","url":"https://a.test/item"},
            {"type":"customer_complaint","quote":"Public tender document 2026","url":"https://b.test/item"},
            {"type":"official_budget","quote":"Published procurement notice","url":"http://c.test/item"},
            {"type":"official_budget","quote":"Published procurement notice","url":"https://d.test/item"}
        ]}]});
        assert_eq!(reference_urls(&a,1),vec!["https://a.test/item"]);
        assert_eq!(reference_urls(&a,6),vec!["https://a.test/item","https://d.test/item"]);
    }
    #[test] fn cited_document_preserves_exact_source_provenance(){
        let s=captured_source("https://example.org/notice",
            "A procurement notice invites supplier quotations for public equipment orders.",
            "2026-11-17T00:00:00Z");
        assert_eq!(s["contentStatus"],"page_fetched");
        assert_eq!(s["sourceName"],"Buyer-cited public document");
        assert_eq!(s["verification"],"direct_document_text_not_confirmed_purchase");
        assert_eq!(s["id"].as_str().unwrap().len(),64);
    }
    #[test] fn multilingual_procurement_wording_is_supported(){
        let s=json!({"id":"cn","contentStatus":"page_fetched",
            "description":"采购意向公告：设备更新计划",
            "articleExcerpt":"采购意向公告：设备更新计划"});
        let row=json!({"type":"public_purchase_request","url":"https://example.cn/rfp",
            "quote":"采购意向公告：设备更新计划"});
        assert!(buying_signal(&annotate(&row,Some(&s))));
    }
}
