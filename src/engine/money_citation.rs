//! Independent, no-LLM page-quotation check for Money hypotheses.
use serde_json::{json,Value};
fn bound(v:&Value,n:usize)->String{
    v.as_str().unwrap_or("").trim().chars().take(n).collect()
}
pub fn normalized(s:&str)->String{
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}
pub fn exact_page_quote(source:&Value,quote:&str)->bool{
    !quote.trim().is_empty() && source["contentStatus"]=="page_fetched"
        && source["articleExcerpt"].as_str()
            .is_some_and(|body|normalized(body).contains(&normalized(quote)))
}
pub fn annotate(row:&Value,source:Option<&Value>)->Value{
    let url=bound(&row["url"],1800);
    let quote=bound(&row["quote"],600);
    let archived=source.filter(|s|s["url"].as_str()==Some(url.as_str()));
    let status=if !url.starts_with("https://")||url.contains(char::is_whitespace){
        "invalid_source_url"
    }else if archived.is_none(){"source_not_archived"}
    else if archived.is_some_and(|s|s["contentStatus"]!="page_fetched"
        ||s["articleExcerpt"].as_str().is_none_or(str::is_empty)){
        "source_not_fetched"
    }else if quote.chars().count()<10{"missing_exact_quote"}
    else if archived.is_some_and(|s|exact_page_quote(s,&quote)){
        "archived_page_quote_match"
    }else{"source_quote_mismatch"};
    json!({"url":url,"claim":bound(&row["claim"],900),
        "quote":quote,"kind":bound(&row["kind"],70),
        "publishedAt":row["publishedAt"],"capturedAt":row["capturedAt"],
        "contentStatus":archived.map(|s|s["contentStatus"].clone()).unwrap_or(Value::Null),
        "quotationStatus":status,
        "sourceId":archived.map(|s|s["id"].clone()).unwrap_or(Value::Null),
        "sourceRevisionId":archived.map(|s|s["contentRevisionId"].clone())
            .unwrap_or(Value::Null),
        "sourceCapturedAt":archived.map(|s|s["capturedAt"].clone())
            .unwrap_or(Value::Null),
        "reviewNote":"Matching archived page text does not establish a true claim, actual purchase or paid transaction"})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn revision_and_url_must_match_the_citation(){
        let source=json!({"id":"s1","url":"https://example.org/rfp",
            "contentStatus":"page_fetched","contentRevisionId":"s1:rev",
            "articleExcerpt":"The procurement notice invites quotations for a medical device."});
        let item=json!({"url":"https://example.org/rfp",
            "quote":"notice invites quotations for a medical device"});
        let checked=annotate(&item,Some(&source));
        assert_eq!(checked["quotationStatus"],"archived_page_quote_match");
        assert_eq!(checked["sourceRevisionId"],"s1:rev");
        assert_eq!(annotate(&json!({"url":"https://other.org/rfp",
            "quote":"notice invites quotations for a medical device"}),Some(&source))
            ["quotationStatus"],"source_not_archived");
        assert_eq!(annotate(&json!({"url":"https://example.org/rfp",
            "quote":"the customer already purchased a million units"}),Some(&source))
            ["quotationStatus"],"source_quote_mismatch");
        assert_eq!(annotate(&json!({"url":"https://example.org/rfp","claim":"Buyer demand"}),
            Some(&source))["quotationStatus"],"missing_exact_quote");
        let mut headline=source;
        headline["contentStatus"]=json!("headline_observed");
        assert_eq!(annotate(&item,Some(&headline))["quotationStatus"],"source_not_fetched");
    }
}