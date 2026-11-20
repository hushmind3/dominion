//! Revisit public web evidence by source ID without replaying entire HTML pages
//! into every model turn. Full readable text remains in SQLite, scoped to the
//! originating session; model-visible excerpts are reproducible char windows.
use crate::db::Store;
use serde_json::{json, Value};

const DEFAULT_EXCERPT_CHARS: usize = 10_000;
const MAX_EXCERPT_CHARS: usize = 24_000;

pub(crate) fn save(
    store: &Store,
    session_id: &str,
    requested_url: &str,
    resolved_url: &str,
    text: &str,
    status: u16,
    content_type: &str,
    truncated_at_source: bool,
    redirects: usize,
) -> Result<Value, String> {
    let id = format!("webdoc-{}", crate::engine::memory::stable_id(&[
        "http-document-v1", session_id, resolved_url, text,
    ]));
    if store.get("http_document", &id).is_none() {
        let record = json!({
            "id":id,
            "sessionId":session_id,
            "requestedUrl":requested_url,
            "url":resolved_url,
            "contentType":content_type,
            "status":status,
            "text":text,
            "sourceTruncated":truncated_at_source,
            "redirects":redirects,
            "createdAt":chrono::Utc::now().to_rfc3339(),
        });
        store.put_if_absent("http_document", &record)
            .ok_or("웹 원문을 보존하지 못했습니다.")?;
    }
    read(store, session_id, &id, 0, DEFAULT_EXCERPT_CHARS, None)
}

/// The original text is never shortened in storage. Callers can read later
/// windows or jump to a literal phrase without refetching the remote site.
pub(crate) fn read(
    store: &Store,
    session_id: &str,
    id: &str,
    offset: usize,
    requested_chars: usize,
    find: Option<&str>,
) -> Result<Value, String> {
    let document = store.get("http_document", id)
        .filter(|record| record["sessionId"].as_str() == Some(session_id))
        .ok_or("이 세션에서 읽을 수 있는 웹 문서가 없습니다.")?;
    let text = document["text"].as_str()
        .ok_or("웹 문서의 원문이 손상됐습니다.")?;
    let total_chars = text.chars().count();
    let effective_offset = if let Some(needle) = find.filter(|s| !s.trim().is_empty()) {
        text.find(needle)
            .map(|byte| text[..byte].chars().count())
            .ok_or("웹 문서에서 해당 문구를 찾지 못했습니다.")?
    } else {
        offset.min(total_chars)
    };
    let max_chars = requested_chars.clamp(1, MAX_EXCERPT_CHARS);
    let excerpt: String = text.chars().skip(effective_offset).take(max_chars).collect();
    let end = effective_offset.saturating_add(excerpt.chars().count());
    Ok(json!({
        "documentId":id,
        "url":document["url"],
        "requestedUrl":document["requestedUrl"],
        "status":document["status"],
        "contentType":document["contentType"],
        "text":excerpt,
        "offset":effective_offset,
        "totalChars":total_chars,
        "nextOffset":(end < total_chars).then_some(end),
        "truncated":document["sourceTruncated"] == true || end < total_chars,
        "sourceTruncated":document["sourceTruncated"],
        "redirects":document["redirects"],
        "retrieval":"source-backed; use http_document_read with documentId, offset or find",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_web_evidence_survives_excerpt_and_can_be_reopened() {
        let dir = std::env::temp_dir().join(format!("dominion-web-evidence-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("records.sqlite")).unwrap();
        let text = format!("START:{}:SEARCH_TARGET:{}:END", "a".repeat(18_000), "b".repeat(12_000));
        let first=save(&store,"session-1","https://example.org/one",
            "https://example.org/final",&text,200,"text/html",false,1).unwrap();
        assert_eq!(first["offset"],0);
        assert_eq!(first["text"].as_str().unwrap().chars().count(),DEFAULT_EXCERPT_CHARS);
        assert_eq!(first["totalChars"],text.chars().count());
        assert!(first["nextOffset"].as_u64().is_some());
        assert!(first["text"].as_str().unwrap().len()<text.len());
        let id=first["documentId"].as_str().unwrap();
        let matched=read(&store,"session-1",id,0,1024,Some("SEARCH_TARGET")).unwrap();
        assert!(matched["text"].as_str().unwrap().starts_with("SEARCH_TARGET"));
        let remaining=read(&store,"session-1",id,text.chars().count()-3,1000,None).unwrap();
        assert_eq!(remaining["text"],"END");
        assert!(remaining["nextOffset"].is_null());
        assert!(read(&store,"session-2",id,0,1000,None).is_err());
        assert_eq!(store.get("http_document",id).unwrap()["text"],text);
        drop(store);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stable_documents_deduplicate_identical_fetches() {
        let dir=std::env::temp_dir().join(format!("dominion-web-dedupe-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store=Store::open(&dir.join("records.sqlite")).unwrap();
        let first=save(&store,"s","https://a.test","https://a.test","readable",200,"text/plain",false,0).unwrap();
        let second=save(&store,"s","https://a.test","https://a.test","readable",200,"text/plain",false,0).unwrap();
        assert_eq!(first["documentId"],second["documentId"]);
        drop(store);
        let _=std::fs::remove_dir_all(dir);
    }
}
