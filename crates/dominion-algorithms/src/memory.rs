//! Deterministic ranking over host-authorized memory candidates.
//! Visibility, scope and evidence lineage are enforced by the host BEFORE
//! ranking, and the lineage callback reads trusted persisted evidence only.
use serde_json::{json,Value};
use std::collections::{BTreeSet,HashSet};
const MAX_RETRIEVAL_CANDIDATES:usize=128;

pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

/// Stable deterministic IDs make replayed extraction jobs harmless. This is an
/// identifier, never a password digest or other security primitive.
pub fn stable_id(parts: &[&str]) -> String {
    let mut first = 0xcbf29ce484222325u64;
    let mut second = 0x84222325cbf29ce4u64;
    for part in parts {
        for byte in (part.len() as u64)
            .to_le_bytes()
            .into_iter()
            .chain(part.bytes())
        {
            first = (first ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            second = (second ^ u64::from(byte)).wrapping_mul(0x100000001e7);
        }
    }
    format!("{first:016x}{second:016x}")
}

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

fn recency_bonus(record: &Value, days: f64, now: &chrono::DateTime<chrono::Utc>) -> f64 {
    let raw = record["lastUsedAt"]
        .as_str()
        .or_else(|| record["learnedAt"].as_str())
        .or_else(|| record["createdAt"].as_str());
    let Some(raw) = raw else {
        return 0.0;
    };
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(raw) else {
        return 0.0;
    };
    let age_days = (now.signed_duration_since(at.with_timezone(&chrono::Utc)))
        .num_seconds()
        .max(0) as f64
        / 86_400.0;
    if age_days <= days {
        0.5 * (1.0 - age_days / days.max(1.0))
    } else {
        0.0
    }
}

fn importance_bonus(record: &Value, strategy: &str, algorithm: &Value, now: &chrono::DateTime<chrono::Utc>) -> f64 {
    let uses = record["uses"].as_u64().unwrap_or(0) as f64;
    match strategy {
        "recency-uses" => {
            let weight = algorithm["parameters"]["usesWeight"].as_f64().unwrap_or(0.15);
            let days = algorithm["parameters"]["recencyDays"].as_f64().unwrap_or(7.0);
            uses.min(8.0) * weight + recency_bonus(record, days, now)
        }
        "evidence-weighted" => {
            let uses_weight = algorithm["parameters"]["usesWeight"].as_f64().unwrap_or(0.1);
            let evidence_weight = algorithm["parameters"]["evidenceWeight"].as_f64().unwrap_or(0.2);
            let evidence = record["evidenceMessageIds"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0) as f64;
            uses.min(8.0) * uses_weight + evidence.min(8.0) * evidence_weight
        }
        _ => 0.0,
    }
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


/// Rank only records already filtered for ownership, lifecycle and evidence.
/// The caller supplies a single reference time and a trusted lineage resolver.
pub fn rank_authorized(
    records:Vec<Value>, query:&str, limit:usize, algorithm:&Value, rerank:&Value,
    now:chrono::DateTime<chrono::Utc>, mut lineage:impl FnMut(&Value)->u8,
)->Vec<Value>{
    let query_terms=terms(query);
    let compact_query=normalize(query);
    let rerank_strategy = rerank["implementation"]
        .as_str()
        .or_else(|| rerank["variant"].as_str())
        .unwrap_or("none");
    // Tokenizing every record is the expensive part of ranking, so the corpus
    // is built only when a lexical reranker actually needs it. Corpus indices
    // stay aligned with the record iteration below and are reused there, so a
    // record is never tokenized twice in the same retrieval.
    let corpus = if rerank_strategy.starts_with("bm25") {
        Some(
            records
                .iter()
                .map(|record| terms(record["text"].as_str().unwrap_or("")))
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    let bm25 = corpus.as_ref().map(|corpus| {
        bm25_scores(
            &query_terms,
            corpus,
            rerank["parameters"]["k1"].as_f64().unwrap_or(1.2),
            rerank["parameters"]["b"].as_f64().unwrap_or(0.75),
        )
    });
    let evidence_lineage = rerank_strategy == "evidence-lineage-2026";
    let mut scored = records
        .into_iter()
        .enumerate()
        .filter_map(|(record_index, mut record)| {
            let text = record["text"].as_str()?;
            let extracted;
            let memory_terms = match &corpus {
                Some(corpus) => &corpus[record_index],
                None => {
                    extracted = terms(text);
                    &extracted
                }
            };
            let matched = query_terms.intersection(memory_terms).count();
            let phrase =
                compact_query.chars().count() >= 2 && normalize(text).contains(&compact_query);
            let importance = record["importance"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0);
            // Standing owner preferences and procedures apply to every turn,
            // even when the query shares no words with them.
            let always = importance >= 0.7
                && matches!(
                    record["type"].as_str(),
                    Some("preference" | "procedure")
                );
            if matched == 0 && !phrase && !always {
                return None;
            }
            let relevance = matched as f64
                / (query_terms.len() as f64 * memory_terms.len().max(1) as f64).sqrt();
            let importance_strategy = algorithm["implementation"]
                .as_str()
                .or_else(|| algorithm["variant"].as_str())
                .unwrap_or("recency-uses");
            let bonus = importance_bonus(&record, importance_strategy, algorithm, &now);
            let score = if let Some(scores) = &bm25 {
                let lexical = scores.get(record_index).copied().unwrap_or(0.0);
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
            };
            let provenance_tier = if evidence_lineage {
                lineage(&record)
            } else {0};
            if evidence_lineage {
                record["retrievalEvidence"]=json!({
                    "basis":"persisted_native_message_lineage",
                    "tier":provenance_tier,
                    "unverified":provenance_tier==0,
                    "counterexamples":record["counterexampleIds"].as_array()
                        .map(Vec::len).unwrap_or(0),
                });
            }
            Some((score, provenance_tier, phrase, matched, record))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        (if evidence_lineage {
            b.1.cmp(&a.1)
                .then_with(||b.2.cmp(&a.2))
                .then_with(||b.3.cmp(&a.3))
        } else {
            std::cmp::Ordering::Equal
        })
        .then_with(||b.0.total_cmp(&a.0))
        .then_with(||b.4["createdAt"].as_str().unwrap_or("")
            .cmp(a.4["createdAt"].as_str().unwrap_or("")))
        .then_with(||a.4["id"].as_str().cmp(&b.4["id"].as_str()))
    });
    let mut seen = HashSet::new();
    scored
        .into_iter()
        .filter_map(|(_, _, _, _, record)| {
            let key = format!(
                "{}:{}",
                record["sourceAuthorKey"].as_str().unwrap_or("owner"),
                normalize(record["text"].as_str().unwrap_or(""))
            );
            seen.insert(key).then_some(record)
        })
        // Room context requests a wider candidate pool (128) and applies its
        // own token budget afterwards. The old hard cap of 20 silently dropped
        // most of that pool before the budget could judge it.
        .take(limit.min(MAX_RETRIEVAL_CANDIDATES))
        .collect()
}


#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn lineage_outranks_untrusted_lexical_match(){
        let records=vec![
            json!({"id":"weak","text":"프로젝트 저장 프로젝트 저장","importance":1.0}),
            json!({"id":"proof","text":"프로젝트 저장","importance":0.5}),
        ];
        let selected=rank_authorized(records,"프로젝트 저장",2,&Value::Null,
            &json!({"implementation":"evidence-lineage-2026"}),chrono::Utc::now(),
            |record|if record["id"]=="proof"{3}else{0});
        assert_eq!(selected[0]["id"],"proof");
        assert_eq!(selected[0]["retrievalEvidence"]["tier"],3);
    }
    #[test]
    fn standing_preferences_survive_zero_overlap(){
        let records=vec![json!({"id":"pref","text":"존댓말","type":"preference","importance":0.9})];
        let selected=rank_authorized(records,"database",1,&Value::Null,
            &json!({"implementation":"bm25"}),chrono::Utc::now(),|_|0);
        assert_eq!(selected.len(),1);
    }
}
