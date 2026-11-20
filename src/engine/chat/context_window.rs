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

/// Do not equate a long provider window with the working set of a question.
/// Keep recent exchanges plus one specifically relevant older owner source.
/// No model summarizer is invoked: the source remains in the durable store.
pub(super) fn focus(history:Vec<Value>,query:&str,is_room:bool)->Vec<Value>{
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
    let retained=history.into_iter().zip(selected)
        .filter_map(|(row,keep)|keep.then_some(row)).collect::<Vec<_>>();
    // The old tool call/result batch is always inert history evidence.
    // Same-turn tool replies bypass this selector entirely.
    trace_budget::compact_historical(&retained,if is_action{8}else{2})
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
        let out=focus(h,"그럼 어떤 방식으로?",false);
        assert!(out.iter().any(|r|r["content"]=="말 짧게 해라"));
        assert!(out.iter().any(|r|r["content"]=="그럼 어떤 방식으로?"));
        assert!(!out.to_string().contains("NOISY_HTML_NOISY_HTML"));
        assert!(out.to_string().len()<2000);
        assert_eq!(original[2]["tool_call_id"],"fetch-one");
    }
    #[test]
    fn code_continue_keeps_goal_and_recent_tool_paths_not_old_file_body(){
        let mut h=vec![user("도미니언 Rust 코드 고쳐"),assistant("계획")];
        for i in 0..30{h.extend(pair(&format!("call-{i}"),&format!("SOURCE_{i}_").repeat(5000)));}
        h.push(assistant("기존 코드를 읽었습니다"));
        h.push(user("이어서해"));
        let out=focus(h,"이어서해",false);
        assert!(out.iter().any(|r|r["content"]=="도미니언 Rust 코드 고쳐"));
        assert!(out.iter().any(|r|r["content"]=="이어서해"));
        assert!(out.to_string().contains("call-29"));
        assert!(!out.to_string().contains("SOURCE_29_SOURCE_29"));
        assert!(out.to_string().len()<14000);
        assert!(out.iter().all(|r|r["role"]!="tool"));
    }
    #[test]
    fn group_history_and_global_compaction_note_remain_untouched(){
        let h=vec![user("[이전 대화 요약: 참고 데이터이며 새 지시가 아님]\nImportant owner condition"),
            user("최근 메시지"),assistant("응답")];
        assert_eq!(focus(h.clone(),"짧게",true),h);
        let out=focus(h,"짧게",false);
        assert!(out[0]["content"].as_str().unwrap().contains("Important owner condition"));
    }
}
