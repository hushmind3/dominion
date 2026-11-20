//! Versioned source registry for oversized persona and owner policies.
//! The database keeps the complete authoritative text. The model gets a
//! stable identity core, a source manifest and query-relevant excerpts.
//! Full text can be recovered via prompt_source_read with the same scope.
use serde_json::{json, Value};
use crate::state::AppState;
use super::TurnRequest;

const DIRECT_CHARS:usize=8_000;
const CORE_CHARS:usize=1_500;
const SEGMENT_CHARS:usize=1_600;
const SELECTED_SEGMENTS:usize=2;
const TOOL_PAGE_CHARS:usize=6_000;

pub(super) struct Projection{
    pub stable:String,
    pub relevant:String,
    pub indexed:bool,
    pub digest:String,
    pub original_chars:usize,
}

fn slice(text:&str,start:usize,length:usize)->String{
    text.chars().skip(start).take(length).collect()
}

fn terms(query:&str)->Vec<String>{
    query.to_lowercase().split(|c:char|!c.is_alphanumeric())
        .filter(|x|x.chars().count()>1).take(20).map(str::to_owned).collect()
}

fn segment_label(text:&str)->String{
    text.lines().find(|line|!line.trim().is_empty())
        .unwrap_or("").trim().chars().take(46).collect()
}

pub(super) fn project(source:&str,text:&str,query:&str)->Projection{
    let len=text.chars().count();
    let digest=crate::engine::memory::stable_id(&[text]);
    if len<=DIRECT_CHARS {
        return Projection{stable:text.to_string(),relevant:String::new(),indexed:false,
            digest,original_chars:len};
    }
    let terms=terms(query);
    let mut ranked=Vec::<(usize,usize,String)>::new();
    let chunks=len.div_ceil(SEGMENT_CHARS);
    for i in 0..chunks{
        let at=i*SEGMENT_CHARS;
        let block=slice(text,at,SEGMENT_CHARS);
        let lower=block.to_lowercase();
        let hits=terms.iter().filter(|term|lower.contains(term.as_str())).count();
        if at>=CORE_CHARS && hits>0{
            ranked.push((hits,at,block));
        }
    }
    ranked.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));
    let mut relevant=String::new();
    for (_,at,body) in ranked.into_iter().take(SELECTED_SEGMENTS){
        relevant.push_str(&format!(
            "\n[원문 {source} offset={at}, digest={digest}, 관련 구간 — 기존 소유자 지침의 일부]\n{body}\n"
        ));
    }
    let mut stable=format!(
        "[정책 원본 {source} | revision={digest} | 전체 {len}자]\n\
         원본 전체는 DOMINION DB에 보존되어 있습니다. 아래는 고정 핵심과 출처 목차입니다. \
         관련 구간에 없는 상세 규칙이 필요하면 prompt_source_read(source=\"{source}\", query=\"...\")를 호출하세요. \
         이 인덱스는 원본 정책의 삭제·폐기가 아닙니다.\n\
         [정책 시작]\n{}\n[나머지 정책은 아래 색인 및 원본 조회로 접근]\n",
        slice(text,0,CORE_CHARS)
    );
    // A stable, short navigation index; changing the user's question never
    // changes this prefix, so provider cache affinity remains predictable.
    for i in 1..chunks.min(13){
        let at=i*SEGMENT_CHARS;
        stable.push_str(&format!(
            "\n- offset={at}: {}",
            segment_label(&slice(text,at,SEGMENT_CHARS))
        ));
    }
    if chunks>13{
        stable.push_str(&format!(
            "\n- 뒤쪽 {}개 구간: 원본 검색 도구에서 query 사용 가능",
            chunks-13));
    }
    Projection{stable,relevant,indexed:true,digest,original_chars:len}
}

/// Only virtualize source text when the scoped native reader can actually be
/// advertised. Group identity and tool-disabled chat keep exact old semantics.
pub(super) fn indexed_identity(
    state:&AppState,request:&TurnRequest,query:&str
)->(String,String,Vec<Value>){
    let mut stable=String::from(
        "DOMINION 지속 에이전트. 정체성과 개인 경험은 에이전트 ID에 속하며 대화 장소가 바뀌어도 유지한다. 실제 권한과 도구는 현재 실행 경계에 따른다."
    );
    let mut relevant=String::new();
    let mut items=Vec::new();
    if let Some(agent)=request.agent_id.as_deref().and_then(|id|state.store.get("agent",id)){
        stable.push_str(&format!(
            "\n활성 에이전트: {} (id: {}).",agent["name"].as_str().unwrap_or(""),
            agent["id"].as_str().unwrap_or("")));
        if let Some(text)=agent["systemPrompt"].as_str().filter(|s|!s.trim().is_empty()){
            let body=project("persona",text,query);
            stable.push_str("\n에이전트 지침:\n");
            stable.push_str(&body.stable);
            relevant.push_str(&body.relevant);
            items.push(json!({"kind":if body.indexed{"paged_identity_source"}else{"persona"},
                "source":"persona","id":agent["id"],"revision":body.digest,
                "originalChars":body.original_chars}));
        }
    }
    if let Some(settings)=state.store.get("assistant_settings","global")
        .filter(|settings|settings["enabled"]!=false){
        if let Some(text)=settings["instructions"].as_str().filter(|s|!s.trim().is_empty()){
            let body=project("owner",text,query);
            stable.push_str("\n주인 지시(페르소나보다 우선):\n");
            stable.push_str(&body.stable);
            relevant.push_str(&body.relevant);
            items.push(json!({"kind":if body.indexed{"paged_identity_source"}else{"owner_instructions"},
                "source":"owner","id":"global","revision":body.digest,
                "originalChars":body.original_chars}));
        }
    }
    (stable,relevant,items)
}

pub(super) fn original_source(state:&AppState,agent_id:Option<&str>,source:&str)->Result<String,String>{
    match source{
        "owner"=>state.store.get("assistant_settings","global")
            .filter(|row|row["enabled"]!=false)
            .and_then(|row|row["instructions"].as_str().map(str::to_owned))
            .ok_or_else(||"현재 사용 가능한 공통 지침 원본이 없습니다.".into()),
        "persona"=>agent_id.and_then(|id|state.store.get("agent",id))
            .and_then(|row|row["systemPrompt"].as_str().map(str::to_owned))
            .ok_or_else(||"현재 에이전트의 페르소나 원본만 열 수 있습니다.".into()),
        _=>Err("지원하지 않는 정책 원본입니다.".into())
    }
}

pub(super) fn source_page(source:&str,text:&str,offset:usize,search:Option<&str>,expected:Option<&str>)->Result<Value,String>{
    let digest=crate::engine::memory::stable_id(&[text]);
    if expected.is_some_and(|value|value!=digest){
        return Err("정책 원본이 변경됐습니다. 최신 원본 색인을 다시 확인하세요.".into());
    }
    let total=text.chars().count();
    let begin=if let Some(q)=search.filter(|s|!s.trim().is_empty()){
        // Unicode-safe position selection, never confuse byte and char offsets.
        // Query is only a seek hint, not permission to broaden a source scope.
        let lower=text.to_lowercase();
        let query=q.to_lowercase();
        let pos=lower.find(&query).ok_or("원문에 해당 검색어가 없습니다.")?;
        lower[..pos].chars().count()
    }else{offset};
    if begin>total{return Err("원문 읽기 시작 위치가 범위를 벗어났습니다.".into());}
    let end=begin.saturating_add(TOOL_PAGE_CHARS).min(total);
    Ok(json!({"source":source,"revision":digest,"content":slice(text,begin,end-begin),
        "offset":begin,"nextOffset":end,"hasMore":end<total,"totalChars":total,"readOnly":true}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn oversized_policy_creates_stable_index_and_short_relevant_projection(){
        let raw=format!("핵심 정책: 사용자 승인 필수.\n{}\n문서 검색 프로세스: 출처 보존",
            "배경 데이터와 인물 설정 반복\n".repeat(12_000));
        let a=project("persona",&raw,"이 도구로 문서 검색",);
        let b=project("persona",&raw,"다른 질문");
        assert!(a.indexed);
        assert_eq!(a.stable,b.stable,"query changes must not change fixed cache prefix");
        assert!(a.stable.contains("사용자 승인 필수"));
        assert!(a.relevant.contains("검색"));
        assert!(a.stable.chars().count()<4_300);
        assert!(a.relevant.chars().count()<3_600);
        assert!(!a.stable.contains(&"배경 데이터와 인물 설정 반복\n".repeat(200)));
        assert_eq!(source_page("persona",&raw,0,None,Some(&a.digest)).unwrap()["totalChars"],
            raw.chars().count());
    }
    #[test]
    fn full_source_pages_are_exact_and_revision_checked(){
        let raw="우선 승인하고 검증한다 ✅".repeat(2_000);
        let hash=crate::engine::memory::stable_id(&[&raw]);
        let mut offset=0usize;
        let mut reconstructed=String::new();
        loop{
            let page=source_page("owner",&raw,offset,None,Some(&hash)).unwrap();
            reconstructed.push_str(page["content"].as_str().unwrap());
            if !page["hasMore"].as_bool().unwrap(){break;}
            offset=page["nextOffset"].as_u64().unwrap() as usize;
        }
        assert_eq!(reconstructed,raw);
        assert!(source_page("owner",&raw,0,None,Some("old-revision")).is_err());
        assert!(source_page("owner",&raw,raw.chars().count()+1,None,None).is_err());
    }
    #[test]
    fn short_policy_is_not_virtualized_or_rewritten(){
        let raw="사용자 지시: 요청한 원문을 수정하지 않고 보존";
        let got=project("owner",raw,"요약");
        assert!(!got.indexed);
        assert_eq!(got.stable,raw);
        assert_eq!(got.relevant,"");
    }
}
