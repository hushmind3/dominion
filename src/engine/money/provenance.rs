//! Model-free attribution to the exact archived Money source revision.
//! This verifies that a cited candidate headline comes from the stored
//! title (not that the whole webpage, buyer intent or payment is true).
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

/// Content-addressed revision of the *archived evidence snapshot*. Re-enriching
/// a headline with page text creates a new revision without mutating an old
/// candidate's stored provenance.
pub fn revision(source:&Value)->String{
    let fields=json!([source["url"],source["title"],
        source["description"],source["articleExcerpt"],source["publishedAt"]]);
    format!("{:x}",Sha256::digest(fields.to_string().as_bytes()))
}

/// A stored title match is a literal comparison against a local source row,
/// not verification of the publisher, actual market transactions, or quotes
/// embedded elsewhere in the research brief.
pub fn evidence(source:&Value,claim:&str,kind:&str)->Value{
    let title=source["title"].as_str().unwrap_or("").trim();
    let match_status=if !claim.is_empty()&&title==claim{
        "stored_title_exact"
    }else if !claim.is_empty()&&title.starts_with(claim){
        "stored_title_prefix"
    }else{"not_in_stored_title"};
    let url=source["url"].as_str().unwrap_or("")
        .chars().take(1500).collect::<String>();
    json!({
        "claim":claim,"url":url,"kind":kind,
        "sourceArchive":"money_signal",
        "sourceId":source["id"],"sourceRevision":revision(source),
        "quoteMatchStatus":match_status,
        "sourceTextStatus":if source["contentStatus"]=="page_fetched"
            && source["articleExcerpt"].as_str().is_some_and(|text|!text.is_empty()){
            "archived_page_excerpt"
        }else{"headline_or_description_only"},
        "transactionVerification":"unknown",
        "publishedAt":source["publishedAt"],
        "capturedAt":source["capturedAt"],
        "contentStatus":source["contentStatus"]
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn provenance_keeps_real_source_id_and_distinguishes_headline_from_full_page(){
        let source=json!({
            "id":"buyer-1","url":"https://buyer.example/notice",
            "title":"Customer asks for industrial equipment",
            "description":"Public notice, not a contract",
            "articleExcerpt":"","contentStatus":"headline_observed",
            "publishedAt":"2026-11-17T00:00:00Z"
        });
        let verified=evidence(&source,"Customer asks for industrial equipment","buyer_side_observation");
        assert_eq!(verified["quoteMatchStatus"],"stored_title_exact");
        assert_eq!(verified["sourceId"],"buyer-1");
        assert_eq!(verified["transactionVerification"],"unknown");
        assert_eq!(verified["sourceTextStatus"],"headline_or_description_only");
        let prefix=evidence(&source,"Customer asks","buyer_side_observation");
        assert_eq!(prefix["quoteMatchStatus"],"stored_title_prefix");
        let invented=evidence(&source,"A completed 500M sale","buyer_side_observation");
        assert_eq!(invented["quoteMatchStatus"],"not_in_stored_title");
    }
    #[test]
    fn page_enrichment_changes_content_revision_without_overwriting_old_claim(){
        let mut source=json!({"id":"source-a",
            "url":"https://supplier.example/pricing",
            "title":"New API cheaper","description":"summary",
            "articleExcerpt":"","contentStatus":"headline_observed"});
        let old=evidence(&source,"New API cheaper","supply_side_observation");
        source["articleExcerpt"]=json!("Full fetched page excerpt from another crawl");
        source["contentStatus"]=json!("page_fetched");
        let latest=evidence(&source,"New API cheaper","supply_side_observation");
        assert_ne!(old["sourceRevision"],latest["sourceRevision"]);
        assert_eq!(old["sourceTextStatus"],"headline_or_description_only");
        assert_eq!(latest["sourceTextStatus"],"archived_page_excerpt");
        assert_eq!(old["quoteMatchStatus"],latest["quoteMatchStatus"]);
    }
}
