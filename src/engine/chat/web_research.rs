//! Host-side enforcement for an owner's explicit public-web research request.
//! Tool selection is normally left to the model. An explicit "search the web"
//! is a requested observable action: successful prose alone is not evidence.
 
/// Deliberately narrow: a local file / conversation / repository search is
/// not automatically sent to a public search engine.
pub(super) fn explicit_query(text:&str)->Option<String> {
    let raw=text.trim();
    if raw.is_empty(){return None;}
    let lower=raw.to_lowercase();
    let internet=["웹에서","웹 검색","웹검색","인터넷","구글 검색","구글검색",
        "온라인 검색","검색엔진","web search","search the web",
        "browse the web","look up online","search online",
        "on the web","online sources"];
    let web_qualified=internet.iter().any(|marker|lower.contains(marker));
    let local_only=["코드 검색","코드검색","파일 검색","파일검색","문서 검색","문서검색",
        "내 대화 검색","대화 검색","기억 검색","메모리 검색","저장소 검색",
        "repository search","search files","search the codebase"];
    if !web_qualified && local_only.iter().any(|marker|lower.contains(marker)) {
        return None;
    }
    let direct=["검색해","검색하","검색 좀","검색좀","검색부터","검색 먼저",
        "검색을 해","검색을 좀","검색해서","검색해줘","검색해봐",
        "찾아봐","찾아줘","찾아라","조사해","조사해줘","조사해서",
        "search for ","search the ","web search","browse the web",
        "look up online","search online","research online",
        "look up ","look into online"];
    let requested=direct.iter().any(|marker|lower.contains(marker))
        || web_qualified && ["찾아","알아봐","확인해","보여","조사","검색",
            "find","lookup","check","search","research","browse"].iter()
            .any(|marker|lower.contains(marker));
    if !requested { return None; }
    // A model cannot silently treat "why didn't you search?" as an explicit
    // instruction to browse. Only positive imperatives are routed here.
    if ["검색 안 했","검색안했","검색을 안","검색 안 해","검색하지 않",
        "don't search","do not search","without searching","검색 없이"].iter()
        .any(|marker|lower.contains(marker)) {
        return None;
    }
    let cleaned=raw.trim_matches(|c:char|c.is_whitespace()||matches!(c,'?'|'？'|'!'|'！'));
    Some(cleaned.chars().take(360).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_owner_web_research_requires_real_tool_observation(){
        for request in [
            "2026년 최신 개발자 자료 검색해",
            "도미니온 구조를 웹에서 찾아봐",
            "깃허브 최신 API를 인터넷 검색해서 비교해",
            "Search the web for current Rust tool calling patterns",
            "뉴스 검색부터 해",
            "2026년 환율 온라인 검색해줘"
        ] {
            assert!(explicit_query(request).is_some(),"{request}");
        }
        for request in [
            "안녕","오늘 뭐해","아까 답변 이어서 해",
            "코드검색해서 함수 위치 찾아줘",
            "파일 검색해","내 대화 검색해","검색을 안 했냐?",
            "검색하지 않고 내 기억으로 답해",
            "2026년 내용 설명해","Please do not search",
        ] {
            assert!(explicit_query(request).is_none(),"{request}");
        }
    }
}
