//! Experience matching and failure classification, independent of any host.
//! The caller must supply scoped, eligible cases; these scores cannot grant
//! permission or independently prove that a proposed intervention succeeded.
use serde_json::Value;
use std::collections::{HashMap,HashSet};
use crate::memory::normalize;
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

pub fn failure_class(error: &str) -> Option<&'static str> {
    let lower = error.to_lowercase();
    if lower.contains("permission denied") || lower.contains("access denied") {
        Some("permission_denied")
    } else if lower.contains("not found") || lower.contains("no such file")
        || lower.contains("does not exist") {
        Some("not_found")
    } else if lower.contains("timed out") || lower.contains("timeout") {
        Some("timeout")
    } else if lower.contains("rate limit") || lower.contains("too many requests") {
        Some("rate_limited")
    } else if lower.contains("conflict") || lower.contains("stale")
        || lower.contains("version mismatch") || lower.contains("sha mismatch")
        || lower.contains("precondition") || lower.contains(" 409")
    {
        Some("stale_state")
    } else if lower.contains("invalid argument") || lower.contains("invalid params")
        || lower.contains("schema validation") {
        Some("invalid_inputs")
    } else {
        None
    }
}

/// Names, not values, allow the same intervention to be considered for a
/// different path/document without replaying the old tool arguments.
pub fn input_shape(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    let mut names = object.keys().map(String::as_str).collect::<Vec<_>>();
    names.sort_unstable();
    Some(names.join(","))
}

pub fn observed_success(value: &Value) -> bool {
    value.is_object()
        && !value.as_object().is_some_and(|v| v.is_empty())
        && value["outcomeUnknown"].as_bool() != Some(true)
        && value["isError"].as_bool() != Some(true)
        && value["success"].as_bool() != Some(false)
        && value["error"].is_null()
        && value.get("exitCode").is_none_or(|v| v.as_i64() == Some(0))
        && value.get("status").and_then(Value::as_u64).is_none_or(|v| v < 400)
}

pub fn observed_failure(value: &Value) -> bool {
    value["isError"].as_bool() == Some(true)
        || value["success"].as_bool() == Some(false)
        || !value["error"].is_null()
        || value.get("exitCode").is_some_and(|v| v.as_i64().is_some_and(|n| n != 0))
        || value.get("status").and_then(Value::as_u64).is_some_and(|v| v >= 400)
}


pub fn rank_authorized(query:&str,candidates:Vec<Value>,kernel:&str,failure_boost:f64,limit:usize)->Vec<Value>{
    let query_tokens=tokens(query);
    if query_tokens.is_empty(){return vec![];}
    let mut scored = if kernel == "balanced" {
        balanced_rerank(query, &candidates, failure_boost)
    } else {
        let mut legacy = Vec::new();
        for record in candidates {
            let haystack = format!(
                "{} {} {}",
                record["signature"].as_str().unwrap_or(""),
                record["request"].as_str().unwrap_or(""),
                record["lesson"].as_str().unwrap_or("")
            ).to_lowercase();
            let overlap = query_tokens.iter()
                .filter(|token| haystack.contains(token.as_str())).count();
            if overlap == 0 {
                continue;
            }
            let boost = if record["outcome"] == "failure" { failure_boost } else { 1.0 };
            let occurrences = record["occurrences"].as_u64().unwrap_or(1) as f64;
            legacy.push((overlap as f64 * 3.0 + boost + occurrences * 0.1, record));
        }
        legacy
    };
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(limit.clamp(1,8)).map(|(_,record)|record).collect()
}
#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn korean_spacing_and_failure_experience_transfer_without_model(){
        let cases=vec![json!({"id":"match","request":"데이터 베이스 연결","lesson":"연결 재시도","outcome":"failure"}),
            json!({"id":"other","request":"unrelated garden","outcome":"success"})];
        let found=rank_authorized("데이터베이스 연결",cases,"balanced",2.0,8);
        assert_eq!(found[0]["id"],"match");
        assert_eq!(found.len(),1);
        assert_eq!(failure_class("HTTP 409 conflict"),Some("stale_state"));
        assert_eq!(input_shape(&json!({"z":1,"a":2})).as_deref(),Some("a,z"));
    }
}
