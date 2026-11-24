//! Pure lexical memory scoring. The Host filters visibility and receipt lineage
//! before invoking this code; scores never authorize access or establish proof.
use serde_json::Value;
use std::collections::BTreeSet;
pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

/// Stable deterministic IDs make replayed extraction jobs harmless. This is an
/// identifier, never a password digest or other security primitive.
use dominion_contracts::identity::stable_id;

/// Stable content revision. Identity fields that do not change the remembered
/// value (timestamps, counters) are excluded, so a re-save of identical content
/// keeps the same revision.
pub fn content_revision(record: &Value) -> String {
    stable_id(&[
        record["id"].as_str().unwrap_or(""),
        record["text"].as_str().unwrap_or(""),
        record["type"].as_str().unwrap_or(""),
        record["scope"].as_str().unwrap_or(""),
        record["status"].as_str().unwrap_or("active"),
    ])
}

fn contains_hangul(text: &str) -> bool {
    text.chars().any(|ch| ('가'..='힣').contains(&ch))
}

pub fn contains_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("-----begin ") && lower.contains("private key-----")
        || lower.contains("authorization: bearer ")
        || lower.split_whitespace().any(|token| {
            (token.starts_with("sk-") && token.len() > 20)
                || (token.starts_with("ghp_") && token.len() > 20)
                || (token.starts_with("xoxb-") && token.len() > 20)
        })
}

/// Character bigrams handle Korean particles/spacing without shipping a model
/// or importing a tokenizer. Latin words keep whole-token matching.
pub fn terms(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for token in text.to_lowercase().split(|ch: char| !ch.is_alphanumeric()) {
        let chars: Vec<char> = token.chars().collect();
        if chars.len() < 2 {
            continue;
        }
        out.insert(token.to_string());
        if contains_hangul(token) {
            for pair in chars.windows(2) {
                out.insert(pair.iter().collect());
            }
        }
    }
    out
}

pub fn bm25_scores(
    query_terms: &BTreeSet<String>,
    documents: &[BTreeSet<String>],
    k1: f64,
    b: f64,
) -> Vec<f64> {
    let count = documents.len().max(1) as f64;
    let average = documents
        .iter()
        .map(|document| document.len() as f64)
        .sum::<f64>()
        / count;
    query_terms
        .iter()
        .map(|term| {
            let df = documents
                .iter()
                .filter(|document| document.contains(term))
                .count() as f64;
            let idf = (1.0 + (count - df + 0.5) / (df + 0.5)).ln();
            documents
                .iter()
                .map(|document| {
                    let tf = if document.contains(term) { 1.0 } else { 0.0 };
                    let length = document.len() as f64;
                    let norm = k1 * (1.0 - b + b * length / average.max(1.0));
                    idf * tf * (k1 + 1.0) / (tf + norm)
                })
                .collect::<Vec<_>>()
        })
        .fold(vec![0.0; documents.len()], |mut total, scores| {
            for (index, score) in scores.into_iter().enumerate() {
                total[index] += score;
            }
            total
        })
}


pub fn rank_score(bm25:Option<f64>,rerank_strategy:&str,phrase:bool,always:bool,matched:usize,relevance:f64,importance:f64,bonus:f64)->f64{
    if let Some(lexical) = bm25 {
                if rerank_strategy == "bm25-importance" {
                    lexical * 2.0 + if phrase { 4.0 } else { 0.0 }
                        + importance + bonus
                } else if always && matched == 0 && !phrase {
                    // The BM25 path must not silently drop standing owner
                    // preferences/procedures when the query shares no words
                    // with them. Keep the same non-BM25 baseline for exactly
                    // the records the filter above promised to keep.
                    1.0 + importance + bonus
                } else {
                    lexical * 2.0 + if phrase { 4.0 } else { 0.0 }
                }
            } else if always {
                1.0 + importance + bonus
            } else {
                relevance * 10.0 + if phrase { 4.0 } else { 0.0 } + importance + bonus
            }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn unicode_terms_and_standing_preference_are_preserved(){
        assert!(terms("데이터베이스").contains("데이"));
        assert_eq!(normalize("Data Base"),"database");
        assert_eq!(rank_score(Some(0.0),"bm25",false,true,0,0.0,0.75,0.5),2.25);
        let docs=[terms("데이터베이스"),terms("파일")];
        let scores=bm25_scores(&terms("데이터"),&docs,1.2,0.75);
        assert!(scores[0]>scores[1]);
    }
}
