//! Experience similarity and failure-weighted ranking on already authorized cases.
use serde_json::Value;
use std::collections::{HashMap,HashSet};
pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

pub fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(str::to_string)
        .collect()
}

pub fn signature(request: &str) -> String {
    let mut seen = HashSet::new();
    tokens(request)
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .take(10)
        .collect::<Vec<_>>()
        .join(" ")
}

/* Hybrid reranker uses corpus-weighted words plus Korean/spacing-resistant
 * character n-grams. It runs entirely on stored experience records and makes
 * no model or embedding request. This is a fuzzy lexical kernel, not a
 * substitute for a semantic embedding index. */
fn hybrid_terms(text: &str) -> HashSet<String> {
    let mut result = HashSet::new();
    let lower = text.to_lowercase();
    for token in lower.split(|ch: char| !ch.is_alphanumeric()) {
        let chars = token.chars().collect::<Vec<_>>();
        if chars.len() >= 2 {
            result.insert(format!("w:{token}"));
            if chars.iter().any(|ch| ('가'..='힣').contains(ch)) {
                for pair in chars.windows(2) {
                    result.insert(format!("g:{}", pair.iter().collect::<String>()));
                }
            }
        }
    }
    // Collapse whitespace/punctuation so "데이터베이스" also finds
    // "데이터 베이스"; unlike substring matching, record both boundaries.
    let compact = normalize(text);
    let chars = compact.chars().collect::<Vec<_>>();
    for window in chars.windows(3) {
        result.insert(format!("c:{}", window.iter().collect::<String>()));
    }
    result
}

pub fn balanced_rerank(query: &str, cases: &[Value], failure_boost: f64) -> Vec<(f64, Value)> {
    let query_terms = hybrid_terms(query);
    if query_terms.is_empty() {
        return Vec::new();
    }
    let documents = cases.iter().map(|record| {
        let body = format!(
            "{} {} {}",
            record["signature"].as_str().unwrap_or(""),
            record["request"].as_str().unwrap_or(""),
            record["lesson"].as_str().unwrap_or("")
        );
        hybrid_terms(&body)
    }).collect::<Vec<_>>();
    let mut document_frequency: HashMap<&str, usize> = HashMap::new();
    for terms in &documents {
        for term in terms {
            if query_terms.contains(term) {
                *document_frequency.entry(term.as_str()).or_default() += 1;
            }
        }
    }
    let corpus = documents.len() as f64;
    cases.iter().zip(documents.iter()).filter_map(|(record, terms)| {
        let shared = query_terms.intersection(terms).collect::<Vec<_>>();
        if shared.is_empty() {
            return None;
        }
        let evidence = shared.iter().map(|term| {
            let df = *document_frequency.get(term.as_str()).unwrap_or(&0) as f64;
            let idf = (1.0 + (corpus - df + 0.5) / (df + 0.5)).ln();
            let weight = if term.starts_with("w:") { 2.0 }
                else if term.starts_with("g:") { 0.7 } else { 0.5 };
            idf * weight
        }).sum::<f64>();
        // Normalization prevents huge stored lessons from dominating a query.
        let coverage = shared.len() as f64 / query_terms.len().max(1) as f64;
        let size_penalty = (terms.len().max(1) as f64).sqrt();
        let frequency = (1.0 + record["occurrences"].as_u64().unwrap_or(1) as f64).ln();
        let failure = if record["outcome"].as_str() == Some("failure") {
            failure_boost.clamp(0.0, 4.0) * 0.15
        } else {
            0.15
        };
        Some((evidence / size_penalty + coverage * 4.0 + frequency * 0.10 + failure,
              record.clone()))
    }).collect()
}


#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]fn related_case_scores_above_unrelated_case(){
        let rows=[json!({"request":"데이터 베이스 연결","lesson":"재연결","outcome":"failure"}),json!({"request":"산책","lesson":"길"})];
        let ranked=balanced_rerank("데이터베이스 연결",&rows,2.0);
        assert_eq!(ranked.len(),1);
        assert_eq!(ranked[0].1["request"],"데이터 베이스 연결");
    }
}
