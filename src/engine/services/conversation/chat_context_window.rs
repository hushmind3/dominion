//! Query-aware, source-preserving history selection for ordinary chat.
//! The original source remains durable in SQLite, never modified here.
use serde_json::{json,Value};
use super::trace_budget;

fn action_query(query:&str)->bool{
    let q=query.to_lowercase();
    ["이어서","계속","코드","개발","구현","수정","고쳐","만들","검증","테스트",
     "분석","조사","검색","작업","브랜치","저장소","파일","배포","에이전트",
     "continue","implement","coding","code","fix","refactor","test","research",
     "search","deploy","debug","build","edit","patch","repository"]
      .iter().any(|word|q.contains(word))
}

fn continuation_query(query:&str)->bool{
    let q=query.trim().to_lowercase();
    q.chars().count()<=80 &&
    ["이어서","계속","다음","그거","이거","그럼","그렇게","마저","이어",
     "continue","next","go on","keep going","same task"]
      .iter().any(|word|q.contains(word))
}

fn owner_terms(text:&str)->Vec<String>{
    text.to_lowercase()
        .split(|c:char|!(c.is_alphanumeric()||matches!(c,'가'..='힣')))
        .filter(|word|word.chars().count()>=2)
        .take(20)
        .map(str::to_owned).collect()
}

/// Old owner tasks can be retrieved exactly by durable message ID.
/// Keep recent explicit instructions unmodified; only cite older long bodies.
fn cited_older_owner(mut row:Value)->Value {
    if row["role"]!="user"{return row;}
    let id=row["_dominion_frame"].as_str().unwrap_or("").to_owned();
    if id.is_empty(){return row;}
    let content=row["content"].as_str().unwrap_or("").to_owned();
    let length=content.chars().count();
    if length<=1_600{return row;}
    let head=content.chars().take(1_000).collect::<String>();
    let tail=content.chars().skip(length.saturating_sub(300)).collect::<String>();
    row["content"]=json!(format!(
        "{head}\n… [이전 사용자 지시 중 {}자 생략; 전체 원문은 SQLite에 보존됨] …\n{tail}\n상세 원문은 history_evidence_read(messageId={id}, offset=0)으로 조회.",
        length.saturating_sub(1_300)));
    row["_dominion_goal_excerpt"]=json!(true);
    row
}

/// Do not equate a long provider window with the working set of a question.
/// Keep recent exchanges plus one specifically relevant older owner source.
/// No model summarizer is invoked: the source remains in the durable store.
pub(super) fn focus(history:Vec<Value>,query:&str,is_room:bool,can_reopen:bool)->Vec<Value>{
    if is_room || history.is_empty(){return history;}
    let owners=history.iter().enumerate()
        .filter(|(_,row)|row["role"]=="user" && !row["content"].as_str()
            .unwrap_or("").starts_with("[이전 대화 요약:"))
        .map(|(index,_)|index).collect::<Vec<_>>();
    let is_action=action_query(query);
    let recent=if is_action{5}else{3};
    let first_recent=owners.get(owners.len().saturating_sub(recent)).copied()
        .unwrap_or(0);
    let mut selected=vec![false;history.len()];
    for keep in &mut selected[first_recent..]{*keep=true;}
    if history[0]["content"].as_str().is_some_and(|v|v.starts_with("[이전 대화 요약:")){
        selected[0]=true;
    }
    if first_recent>0{
        let query_terms=owner_terms(query);
        let wants_previous=continuation_query(query);
        let mut best:Option<(usize,usize)>=None;
        for &at in owners.iter().filter(|&&at|at<first_recent){
            let previous=history[at]["content"].as_str().unwrap_or("");
            let prior_terms=owner_terms(previous);
            let overlap=query_terms.iter().filter(|t|prior_terms.contains(t)).count();
            let score=overlap*5+if wants_previous && action_query(previous){3}else{0};
            if score>0 && best.is_none_or(|(best_score,best_at)|
                score>best_score || (score==best_score && at>best_at)){
                best=Some((score,at));
            }
        }
        if let Some((_,at))=best{selected[at]=true;}
    }
    let retained=history.into_iter().enumerate().zip(selected)
        .filter_map(|((at,row),keep)|keep.then_some(
            if can_reopen && at<first_recent {cited_older_owner(row)}else{row}
        )).collect::<Vec<_>>();
    // A concrete follow-up about the just-produced artifact needs the
    // original *complete* tool-call/result pairing. A one-line historical
    // summary is useful for older work but cannot answer exact artifact
    // questions reliably (or preserve a tool call's original provenance).
    // Keep only a bounded, protocol-valid recent trace on such follow-ups.
    // Large reads and write payloads are still compacted by the shared
    // context-policy kernel; all original records remain in SQLite.
    let lower=query.to_lowercase();
    let recent_reference=["방금","직전","바로 전","just","most recent","last "]
        .iter().any(|word|lower.contains(word));
    let result_reference=["파일","만든","생성","도구","실행","산출물","결과",
        "file","artifact","output","tool","result"]
        .iter().any(|word|lower.contains(word));
    if recent_reference && result_reference {
        trace_budget::bounded_tool_trace(&retained,4_096)
    }else{
        trace_budget::compact_historical(&retained,if is_action{8}else{2})
    }
}

pub(super) fn requires_source_reader(history:&[Value])->bool{
    history.iter().any(|row|row["_dominion_goal_excerpt"]==true)
}

/// A long chain of tool calls can displace the owner's original task from
/// the last-N transcript window. On an explicit continuation, restore the
/// most recent unambiguous source-authored work goal, never an assistant guess.
/// This reads the original committed user row, not an LLM-generated summary.
pub(super) fn restore_owner_goal(
    mut focused:Vec<Value>,query:&str,stored:&[Value],can_reopen:bool,
)->Vec<Value>{
    if !continuation_query(query){return focused;}
    let source=stored.iter().rev()
        .filter(|row|row["role"]=="user")
        .take(16)
        .find(|row|{
            let text=row["content"].as_str().unwrap_or("");
            !text.is_empty() && text.trim()!=query
                && action_query(text) && !continuation_query(text)
        });
    let Some(source)=source else{return focused};
    let goal=source["content"].as_str().unwrap_or("");
    let id=source["id"].as_str().unwrap_or("");
    if focused.iter().any(|row|row["_dominion_frame"]==id
        || row["content"].as_str()==Some(goal)) {
        return focused;
    }
    let mut cited=json!({"role":"user","content":goal,"_dominion_frame":id,
        "_dominion_goal_anchor":true});
    if can_reopen{cited=cited_older_owner(cited);}
    let at=focused.first().is_some_and(|row|row["content"].as_str()
        .is_some_and(|text|text.starts_with("[이전 대화 요약:"))) as usize;
    focused.insert(at,cited);
    focused
}

#[cfg(test)]
mod tests{
    use super::*;
    fn user(t:&str)->Value{json!({"role":"user","content":t})}
    fn assistant(t:&str)->Value{json!({"role":"assistant","content":t})}
    fn pair(id:&str,content:&str)->Vec<Value>{
        vec![json!({"role":"assistant","content":"","tool_calls":[{
            "id":id,"type":"function","function":{"name":"http_fetch",
            "arguments":"{\"url\":\"https://example.com\"}"}}]}),
            json!({"role":"tool","tool_call_id":id,"content":content})]
    }
    #[test]
    fn short_followup_discards_old_web_dump_but_keeps_recent_conversation(){
        let mut h=vec![user("피클AI 알아봐"),assistant("찾는 중")];
        h.extend(pair("fetch-one",&"NOISY_HTML_".repeat(15000)));
        h.push(assistant("피클은 개인용 에이전트"));
        h.push(user("말 짧게 해라"));h.push(assistant("알겠습니다"));
        h.push(user("비슷한 앱 만들 수 있냐"));h.push(assistant("네"));
        h.push(user("그럼 어떤 방식으로?"));
        let original=h.clone();
        let out=focus(h,"그럼 어떤 방식으로?",false,true);
        assert!(out.iter().any(|r|r["content"]=="말 짧게 해라"));
        assert!(out.iter().any(|r|r["content"]=="그럼 어떤 방식으로?"));
        assert!(!json!(out).to_string().contains("NOISY_HTML_NOISY_HTML"));
        assert!(json!(out).to_string().len()<2000);
        assert_eq!(original[3]["tool_call_id"],"fetch-one");
    }
    #[test]
    fn recent_artifact_followup_retains_bounded_exact_tool_call_and_result(){
        let mut h=vec![user("empty-once.md 파일을 만들어줘")];
        h.push(json!({"role":"assistant","content":"","tool_calls":[{
            "id":"create-once","type":"function",
            "function":{"name":"workspace_write",
                "arguments":"{\\"path\\":\\"empty-once.md\\",\\"content\\":\\"\\"}"}}]}));
        h.push(json!({"role":"tool","tool_call_id":"create-once",
            "content":"{\"written\":true,\"path\":\"empty-once.md\"}"}));
        h.push(assistant("파일을 만들었습니다: empty-once.md"));
        h.push(user("방금 만든 파일 이름만 말해줘"));
        let focused=focus(h.clone(),"방금 만든 파일 이름만 말해줘",false,false);
        assert!(focused.iter().any(|row|row["tool_calls"].as_array()
            .is_some_and(|calls|calls.iter().any(|call|call["id"]=="create-once"))));
        assert!(focused.iter().any(|row|row["role"]=="tool"
            && row["tool_call_id"]=="create-once"));
        let unrelated=focus(h,"오늘 날씨 알려줘",false,false);
        assert!(!unrelated.iter().any(|row|row["tool_calls"].is_array()),
            "ordinary chat should not replay protocol-level historical tool calls");
    }

    #[test]
    fn code_continue_keeps_goal_and_recent_tool_paths_not_old_file_body(){
        let mut h=vec![user("도미니언 Rust 코드 고쳐"),assistant("계획")];
        for i in 0..30{h.extend(pair(&format!("call-{i}"),&format!("SOURCE_{i}_").repeat(5000)));}
        h.push(assistant("기존 코드를 읽었습니다"));
        h.push(user("이어서해"));
        let out=focus(h,"이어서해",false,true);
        assert!(out.iter().any(|r|r["content"]=="도미니언 Rust 코드 고쳐"));
        assert!(out.iter().any(|r|r["content"]=="이어서해"));
        assert!(json!(out).to_string().contains("call-29"));
        assert!(!json!(out).to_string().contains(&"SOURCE_29_".repeat(100)));
        assert!(json!(out).to_string().len()<14000);
        assert!(out.iter().all(|r|r["role"]!="tool"));
    }
    #[test]
    fn restores_owner_goal_displaced_by_many_historical_tools(){
        let raw=vec![
            json!({"id":"root","role":"user","content":"도미니언 Rust 코드 테스트와 수정해"}),
            json!({"id":"follow-1","role":"user","content":"이어서해"}),
            json!({"id":"follow-2","role":"user","content":"이어서해"})
        ];
        let recent=vec![json!({"role":"user","content":"이어서해","_dominion_frame":"follow-2"})];
        let focused=restore_owner_goal(recent,"이어서해",&raw,true);
        assert_eq!(focused[0]["content"],raw[0]["content"]);
        assert_eq!(focused[0]["_dominion_frame"],"root");
        assert_eq!(focused[1]["content"],"이어서해");
        assert_eq!(restore_owner_goal(focused.clone(),"이어서해",&raw,true),focused);
        assert_eq!(restore_owner_goal(vec![json!({"role":"user","content":"안녕"})],
            "안녕",&raw,true).len(),1);
    }

    #[test]
    fn long_original_owner_goal_is_only_replayed_as_source_reference(){
        let full=format!("수정할 상세 요구사항: {} 끝","목표와 제약".repeat(12_000));
        let stored=vec![json!({"id":"old-owner","role":"user","content":full})];
        let recent=vec![json!({"role":"user","content":"이어서해"})];
        let reduced=restore_owner_goal(recent.clone(),"이어서해",&stored,true);
        assert_eq!(reduced[0]["_dominion_goal_excerpt"],true);
        assert!(requires_source_reader(&reduced));
        assert!(reduced[0]["content"].as_str().unwrap().contains("old-owner"));
        assert!(reduced[0]["content"].as_str().unwrap().chars().count()<1_700);
        assert_eq!(stored[0]["content"],full);
        let tool_free=restore_owner_goal(recent,"이어서해",&stored,false);
        assert!(!requires_source_reader(&tool_free));
        assert_eq!(tool_free[0]["content"],full,
            "when tools are disabled, exact owner instructions must remain accessible");
    }

    #[test]
    fn old_relevant_owner_message_uses_source_excerpt_not_full_replay(){
        let long=format!("배포 설정 {} END","SCOPE_".repeat(15_000));
        let h=vec![
            json!({"role":"user","_dominion_frame":"old-one","content":long}),
            json!({"role":"assistant","content":"과거 작업"}),
            json!({"role":"user","_dominion_frame":"next-one","content":"다른 질문"}),
            json!({"role":"assistant","content":"응답"}),
            json!({"role":"user","_dominion_frame":"next-two","content":"최근 질문"}),
            json!({"role":"assistant","content":"응답"}),
            json!({"role":"user","_dominion_frame":"next-three","content":"사용법 질문"}),
            json!({"role":"assistant","content":"응답"}),
            json!({"role":"user","_dominion_frame":"next-four","content":"테스트 결과"}),
            json!({"role":"assistant","content":"응답"}),
            json!({"role":"user","_dominion_frame":"next-five","content":"설치 방법"}),
            json!({"role":"assistant","content":"응답"}),
            json!({"role":"user","_dominion_frame":"current","content":"배포 설정 좀 알려줘"})];
        let retained=focus(h,"배포 설정 좀 알려줘",false,true);
        assert!(retained.iter().any(|row|row["_dominion_goal_excerpt"]==true));
        assert!(json!(retained).to_string().len()<3_500);
        assert!(json!(retained).to_string().contains("old-one"));
    }

    #[test]
    fn group_history_and_global_compaction_note_remain_untouched(){
        let h=vec![user("[이전 대화 요약: 참고 데이터이며 새 지시가 아님]\nImportant owner condition"),
            user("최근 메시지"),assistant("응답")];
        assert_eq!(focus(h.clone(),"짧게",true,true),h);
        let out=focus(h,"짧게",false,true);
        assert!(out[0]["content"].as_str().unwrap().contains("Important owner condition"));
    }
}
