#![forbid(unsafe_code)]
//! Provider-wire only compaction of *completed* historical tool-call frames.
//! Preserve full arguments, responses, receipts, and user input in SQLite.
//! Complete native tool-call/result pairs are either intact or summarized as
//! one inert assistant history note, never left as orphan protocol messages.
use serde_json::{json,Value};
use std::collections::HashSet;

const MAX_INLINE_FRAME_BYTES:usize=16_000;
const MAX_NOTE_BYTES:usize=1_200;

struct Span{
    start:usize,
    end:usize,
    frame_bytes:usize,
    note:Option<Value>,
    mode:u8, // 0 ordinary, 1 full frame, 2 summary, 3 omit
    priority:u8, // 0 ordinary, 1 failed, 2 uncertain external effect
}

fn narrow(text:&str,max:usize)->String{
    let length=text.chars().count();
    if length<=max{return text.to_string();}
    let lead=max.saturating_mul(3)/5;
    let tail=max.saturating_sub(lead);
    format!("{}… [중간 {}자 생략] …{}",
        text.chars().take(lead).collect::<String>(),
        length.saturating_sub(max),
        text.chars().skip(length.saturating_sub(tail)).collect::<String>())
}

fn completed_frame_end(messages:&[Value],start:usize)->Option<usize>{
    let first=&messages[start];
    if first["role"]!="assistant"{return None;}
    let calls=first["tool_calls"].as_array().filter(|calls|!calls.is_empty())?;
    let mut ids=HashSet::new();
    for call in calls{
        let id=call["id"].as_str()?;
        if id.is_empty()||!ids.insert(id){return None;}
    }
    let mut end=start+1;
    while end<messages.len() && messages[end]["role"]=="tool"{
        end+=1;
    }
    if end-start-1!=calls.len(){return None;}
    let mut seen=HashSet::new();
    for result in &messages[start+1..end]{
        let id=result["tool_call_id"].as_str()?;
        if !ids.contains(id)||!seen.insert(id){return None;}
    }
    Some(end)
}

// Inspect typed outcomes, not arbitrary text such as a source file mentioning
// "error". Unknown effects have priority over known failures and ordinary reads.
fn outcome_priority(row:&Value)->u8{
    fn priority(value:&Value)->u8{
        if value["outcomeUnknown"]==true || value["effectUnknown"]==true
            || matches!(value["status"].as_str(),Some("outcome_unknown"|"effect_unknown")){
            return 2;
        }
        if value["ok"]==false || value["success"]==false || value["dispatched"]==false
            || value["isError"]==true
            || matches!(value["status"].as_str(),Some("failed"|"error"|"not_dispatched"|"cancelled"))
            || value.get("error").is_some_and(|error|!error.is_null() && error!=false
                && error.as_str()!=Some("")){
            return 1;
        }
        0
    }
    let outer=priority(row);
    let inner=match &row["content"]{
        Value::String(text) if text.trim_start().starts_with('{')=>
            serde_json::from_str::<Value>(text).ok()
                .map(|parsed|priority(&parsed)).unwrap_or(0),
        Value::Object(_)=>priority(&row["content"]),
        _=>0,
    };
    outer.max(inner)
}
fn frame_priority(messages:&[Value],start:usize,end:usize)->u8{
    messages[start+1..end].iter().map(outcome_priority).max().unwrap_or(0)
}

fn note(messages:&[Value],start:usize,end:usize)->Value{
    let assistant=&messages[start];
    let calls=assistant["tool_calls"].as_array().expect("completed tool group");
    let results=&messages[start+1..end];
    let mut lines=Vec::new();
    let unknown=results.iter().filter(|row|outcome_priority(row)==2).count();
    let failed=results.iter().filter(|row|outcome_priority(row)==1).count();
    if unknown>0{
        lines.push(format!("[outcome_unknown: {unknown}개] 외부 효과 발생 여부 불명. 원본 기록으로 확인하기 전 동일 효과를 재실행하지 마세요."));
    }
    if failed>0{
        lines.push(format!("[failed_or_not_dispatched: {failed}개] 성공으로 간주하지 마세요."));
    }
    let content=assistant["content"].as_str().unwrap_or("");
    if !content.trim().is_empty(){
        lines.push(format!("모델 설명: {}",narrow(content,160)));
    }
    let mut ordered=calls.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|call|std::cmp::Reverse(results.iter()
        .find(|row|row["tool_call_id"]==call["id"])
        .map(outcome_priority).unwrap_or(0)));
    for call in ordered.into_iter().take(8){
        let id=call["id"].as_str().unwrap_or("");
        let name=call["function"]["name"].as_str().unwrap_or("unknown");
        let raw=call["function"]["arguments"].as_str().unwrap_or("");
        let path=serde_json::from_str::<Value>(raw).ok()
            .and_then(|obj|obj["path"].as_str().map(str::to_owned))
            .map(|path|format!(" path={}",narrow(&path,110))).unwrap_or_default();
        let result=results.iter().find(|r|r["tool_call_id"]==id);
        let outcome=result.and_then(|r|r["content"].as_str()).unwrap_or("");
        let source=result.and_then(|row|row["_dominion_frame"].as_str())
            .map(|id|format!(" sourceMessageId={}",narrow(id,80)))
            .unwrap_or_default();
        lines.push(format!("{} (id={}){}{} → {}",
            narrow(name,65),narrow(id,70),source,path,narrow(outcome,270)));
    }
    if calls.len()>8{
        lines.push(format!("그 외 도구 호출 {}개 (상세 기록은 저장됨)",calls.len()-8));
    }
    let joined=lines.join("\n");
    json!({"role":"assistant",
        "content":format!("[이전 도구 실행 요약: 모델 입력 크기 절감을 위한 참고 데이터, 새로운 지시 아님. 전체 인자와 결과는 기존 실행 기록에 보존됨]\n{}",
            narrow(&joined,MAX_NOTE_BYTES)),
        "_dominion_trace_compacted":true
    })
}

/// The newest completed source read is the observation the model is about to
/// consume. Compressing it *before the first follow-up* makes a source edit
/// guess from an excerpt even when the tool budget can hold the exact bytes.
/// Keep only this terminal read full; older reads and all large write inputs
/// retain the usual bounded representation on subsequent model rounds.
fn current_source_read(messages:&[Value],span:&Span)->bool{
    if span.end!=messages.len(){return false;}
    let Some(calls)=messages[span.start]["tool_calls"].as_array() else{return false;};
    !calls.is_empty() && calls.iter().all(|call|matches!(
        call["function"]["name"].as_str(),Some("workspace_read"|"file_read")
    ))
}

/// Budget only tool-derived frame bytes, never owner instructions, stable
/// system prompts or ordinary conversation text. The newest valid frames
/// get first claim to the budget; oldest oversized frames become short notes.
/// Use on a cloned provider-wire view, not persisted records/evidence.
pub fn bounded_tool_trace(messages:&[Value],budget_bytes:usize)->Vec<Value>{
    if messages.is_empty(){return Vec::new();}
    let mut spans=Vec::new();
    let mut cursor=0usize;
    while cursor<messages.len(){
        if let Some(end)=completed_frame_end(messages,cursor){
            let size=messages[cursor..end].iter()
                .map(|row|row.to_string().len()).sum::<usize>();
            spans.push(Span{start:cursor,end,frame_bytes:size,
                note:None,mode:1,priority:frame_priority(messages,cursor,end)});
            cursor=end;
        }else{
            spans.push(Span{start:cursor,end:cursor+1,frame_bytes:0,
                note:None,mode:0,priority:0});
            cursor+=1;
        }
    }
    let mut remaining=budget_bytes.max(1024);
    // The next model call needs the latest observation first. Remaining
    // budget prioritizes unresolved effects and failures over older successes.
    let mut order=(0..spans.len()).filter(|&index|spans[index].mode==1)
        .collect::<Vec<_>>();
    let latest=order.last().copied();
    order.sort_by_key(|&index|std::cmp::Reverse((
        Some(index)==latest,spans[index].priority,index
    )));
    for index in order{
        let span=&mut spans[index];
        if span.frame_bytes<=remaining
            && (span.frame_bytes<=MAX_INLINE_FRAME_BYTES
                || current_source_read(messages,span)){
            remaining-=span.frame_bytes;
            continue;
        }
        // Most tool pairs are small. Avoid parsing their JSON arguments
        // or allocating summaries on every provider round unless required.
        let summary=note(messages,span.start,span.end);
        let note_size=summary.to_string().len();
        if note_size<=remaining{
            span.mode=2;
            span.note=Some(summary);
            remaining-=note_size;
        }else{
            span.mode=3;
        }
    }
    let mut out=Vec::with_capacity(messages.len());
    for span in spans{
        match span.mode{
            0|1=>out.extend_from_slice(&messages[span.start..span.end]),
            2=>out.push(span.note.expect("summary")),
            _=>{}
        }
    }
    out
}

/// Historical tool observations are references, not a new tool round.
/// Every old successful tool call/result pair gets an inert, source-labelled
/// receipt; only the most recent relevant receipts enter the model context.
/// Complete results and arguments remain in the durable original transcript.
pub fn compact_historical(messages:&[Value],max_receipts:usize)->Vec<Value>{
    let mut frames=Vec::new();
    let mut i=0usize;
    while i<messages.len(){
        if let Some(end)=completed_frame_end(messages,i){
            frames.push((i,frame_priority(messages,i,end)));
            i=end;
        }else{i+=1;}
    }
    // Keep the newest receipt for continuity, then allocate remaining
    // slots to unresolved effects, failures and finally ordinary results.
    // Keep output chronological, even though selection is priority-based.
    let latest=frames.last().map(|(start,_)|*start);
    frames.sort_by_key(|&(start,priority)|std::cmp::Reverse((
        Some(start)==latest,priority,start
    )));
    let omitted_unknown=frames.iter().skip(max_receipts).filter(|(_,priority)|*priority==2).count();
    let omitted_failed=frames.iter().skip(max_receipts).filter(|(_,priority)|*priority==1).count();
    let kept=frames.into_iter().take(max_receipts)
        .map(|(start,_)|start).collect::<HashSet<_>>();
    let mut omitted=false;
    let mut out=Vec::with_capacity(messages.len().min(32));
    i=0;
    while i<messages.len(){
        if let Some(end)=completed_frame_end(messages,i){
            if kept.contains(&i){
                let mut summary=note(messages,i,end);
                if let Some(source)=messages[i]["_dominion_frame"].as_str(){
                    summary["_dominion_frame"]=json!(source);
                }
                out.push(summary);
            } else if !omitted {
                omitted=true;
                let warning=if omitted_unknown>0 || omitted_failed>0 {
                    format!(" 생략된 실행 묶음: 결과 불명 {omitted_unknown}개, 실패 {omitted_failed}개. 원본 실행 기록을 확인하기 전 불명확한 효과를 반복하지 마세요.")
                }else{String::new()};
                out.push(json!({"role":"assistant",
                    "content":format!("[이전 도구 기록은 원본에 보존됨. 필요한 원문은 해당 파일 또는 출처를 다시 조회하세요.{warning}]") }));
            }
            i=end;
            continue;
        }
        if messages[i]["role"]=="tool"{
            // A tool result without its full assistant batch is unusable by
            // the upstream tool protocol. Keep it in SQLite, not on the wire.
            i+=1;
            continue;
        }
        if messages[i]["role"]=="assistant" && messages[i]["tool_calls"].as_array()
            .is_some_and(|calls|!calls.is_empty()){
            // Incomplete prior tool batches do not form an executable frame.
            // Never make them look like a fresh call in a later turn.
            let mut row=messages[i].clone();
            row.as_object_mut().map(|obj|{
                obj.remove("tool_calls");obj.remove("reasoning_content");
            });
            let original=row["content"].as_str().unwrap_or("");
            row["content"]=json!(format!(
                "[이전 도구 호출은 완료되지 않음 — 보존된 실행 기록 참고] {}",
                narrow(original,160)
            ));
            out.push(row);
        }else{
            let mut row=messages[i].clone();
            if row["role"]=="assistant"{
                if let Some(map)=row.as_object_mut(){
                    map.remove("reasoning_content");
                    map.remove("reasoning");
                }
            }
            out.push(row);
        }
        i+=1;
    }
    out
}

#[cfg(test)]
mod tests{
    use super::*;
    fn call(id:&str,name:&str,arguments:&str)->Value{
        json!({"id":id,"type":"function",
            "function":{"name":name,"arguments":arguments}})
    }
    fn pair(id:&str,name:&str,arguments:&str,result:&str)->Vec<Value>{
        vec![
            json!({"role":"assistant","content":"",
                "tool_calls":[call(id,name,arguments)]}),
            json!({"role":"tool","tool_call_id":id,"content":result}),
        ]
    }
    #[test]
    fn small_frames_preserve_provider_protocol_and_owner_text(){
        let mut messages=vec![json!({"role":"system","content":"policy"}),
            json!({"role":"user","content":"fix actual tests"})];
        messages.extend(pair("a","workspace_read",r#"{"path":"main.rs"}"#,"file content"));
        let wire=bounded_tool_trace(&messages,10_000);
        assert_eq!(wire,messages,"short replay must not change semantics");
    }
    #[test]
    fn large_write_arguments_never_reenter_provider_context(){
        let original="PRIVATE_LARGE_SOURCE_".repeat(12_000);
        let args=json!({"path":"src/main.rs","content":original}).to_string();
        let mut messages=vec![json!({"role":"system","content":"identity"}),
            json!({"role":"user","content":"write src/main.rs"})];
        messages.extend(pair("write-1","workspace_write",&args,
            r#"{"ok":true,"path":"src/main.rs"}"#));
        let wire=bounded_tool_trace(&messages,10_000);
        assert_eq!(wire[0],messages[0]);
        assert_eq!(wire[1],messages[1]);
        assert_eq!(wire.len(),3);
        assert_eq!(wire[2]["role"],"assistant");
        assert!(wire[2]["tool_calls"].is_null());
        assert!(wire[2]["content"].as_str().unwrap().contains("workspace_write"));
        assert!(wire[2]["content"].as_str().unwrap().contains("src/main.rs"));
        assert!(wire[2]["content"].as_str().unwrap().contains("ok"));
        assert!(wire[2].to_string().len()<2_000);
        assert!(wire.iter().all(|row|!row.to_string().contains("PRIVATE_LARGE_SOURCE_")));
        assert_eq!(messages[2]["tool_calls"][0]["function"]["arguments"],args);
    }
    #[test]
    fn newest_tool_frame_gets_budget_before_older_frames(){
        let mut messages=vec![json!({"role":"system","content":"stable"})];
        messages.extend(pair("old","workspace_read",r#"{"path":"old"}"#,&"OLD_NOTE ".repeat(1000)));
        messages.push(json!({"role":"user","content":"new owner instruction"}));
        messages.extend(pair("new","workspace_read",r#"{"path":"new"}"#,"NEW_CURRENT_RESULT"));
        let wire=bounded_tool_trace(&messages,2_000);
        assert!(wire.iter().any(|row|row["role"]=="tool"
            && row["content"]=="NEW_CURRENT_RESULT"));
        assert!(wire.iter().any(|row|row["content"]=="new owner instruction"));
        assert!(serde_json::to_string(&wire).unwrap().len()<serde_json::to_string(&messages).unwrap().len()/2);
    }
    #[test]
    fn latest_large_source_read_is_available_for_exact_edit_before_later_compaction(){
        // 2026 harness property: a source read can be >16 KiB while still
        // fitting the already-approved per-round wire budget.
        let source="pub fn exact_source_line() { println!(\\\"code\\\"); }\\n".repeat(550);
        let mut transcript=vec![json!({"role":"system","content":"stable"}),
            json!({"role":"user","content":"inspect and edit the function"})];
        transcript.extend(pair("read-1","workspace_read",r#"{"path":"src/agent.rs"}"#,&source));
        assert!(transcript.iter().map(|row|row.to_string().len()).sum::<usize>()<40_000);
        assert!(source.len()>MAX_INLINE_FRAME_BYTES);
        let immediate=bounded_tool_trace(&transcript,40_000);
        assert_eq!(immediate,transcript,"first provider follow-up needs the exact tool result");
        transcript.push(json!({"role":"assistant","content":"I have consumed the source"}));
        let subsequent=bounded_tool_trace(&transcript,40_000);
        assert_eq!(subsequent.len(),4);
        assert_eq!(subsequent[2]["_dominion_trace_compacted"],true);
        assert!(!json!(subsequent).to_string().contains(&source[..500]));
        assert!(json!(subsequent).to_string().len()<json!(transcript).to_string().len()/3);
        // A narrow route cannot opt in to unlimited source replay.
        let constrained=bounded_tool_trace(&transcript[..4],8_000);
        assert_eq!(constrained[2]["_dominion_trace_compacted"],true);
    }
    #[test]
    fn large_current_write_arguments_remain_compacted_even_with_wire_headroom(){
        let private="LARGE_WRITE_CONTENT_".repeat(1100);
        let args=json!({"path":"src/agent.rs","content":private}).to_string();
        let mut transcript=vec![json!({"role":"user","content":"update file"})];
        transcript.extend(pair("write-1","workspace_write",&args,r#"{"ok":true}"#));
        assert!(transcript.iter().map(|row|row.to_string().len()).sum::<usize>()<40_000);
        let wire=bounded_tool_trace(&transcript,40_000);
        assert_eq!(wire.len(),2);
        assert_eq!(wire[1]["_dominion_trace_compacted"],true);
        assert!(!json!(wire).to_string().contains("LARGE_WRITE_CONTENT_LARGE_WRITE_CONTENT_"));
    }
    #[test]
    fn historical_selection_preserves_uncertain_effect_before_ordinary_reads(){
        let mut messages=vec![json!({"role":"user","content":"Finish task"})];
        messages.extend(pair("unknown-write","workspace_write","{}",
            r#"{"outcomeUnknown":true,"error":"connection lost"}"#));
        for index in 0..6{
            messages.extend(pair(&format!("read-{index}"),"workspace_read","{}","ordinary result"));
        }
        let original=messages.clone();
        let wire=compact_historical(&messages,2);
        let text=json!(wire).to_string();
        assert!(text.contains("unknown-write"));
        assert!(text.contains("[outcome_unknown: 1"));
        assert!(text.contains("read-5"));
        assert!(!text.contains("read-4"));
        assert_eq!(messages,original);
    }
    #[test]
    fn omitted_uncertain_history_is_explicit_even_when_only_latest_receipt_fits(){
        let mut messages=pair("unknown","workspace_write","{}",r#"{"effectUnknown":true}"#);
        messages.extend(pair("latest","workspace_read","{}","ordinary"));
        let wire=compact_historical(&messages,1);
        assert!(wire.iter().any(|row|row["content"].as_str().is_some_and(|text|text.contains("결과 불명 1개"))));
        assert!(json!(wire).to_string().contains("latest"));
    }
    #[test]
    fn bounded_trace_spends_remaining_budget_on_failure_before_success(){
        let mut messages=vec![json!({"role":"user","content":"Continue"})];
        messages.extend(pair("uncertain-write","workspace_write","{}",
            r#"{"status":"outcome_unknown","error":"connection lost"}"#));
        for index in 0..8{
            messages.extend(pair(&format!("read-{index}"),"workspace_read","{}",&"x".repeat(700)));
        }
        messages.extend(pair("latest","workspace_read","{}","next observation"));
        let wire=bounded_tool_trace(&messages,1024);
        let text=json!(wire).to_string();
        assert!(text.contains("uncertain-write"),"{text}");
        assert!(text.contains("latest"));
        assert!(!text.contains("read-0"),"older ordinary output should yield to critical evidence");
        assert!(text.contains("outcome_unknown"));
    }
    #[test]
    fn summary_prioritizes_uncertain_call_even_after_first_eight_batch_members(){
        let calls=(0..12).map(|index|call(&format!("id-{index}"),"workspace_write","{}"))
            .collect::<Vec<_>>();
        let mut messages=vec![json!({"role":"assistant","tool_calls":calls})];
        for index in 0..12{
            messages.push(json!({"role":"tool","tool_call_id":format!("id-{index}"),
                "content":if index==11{r#"{"effectUnknown":true}"#}else{r#"{"ok":true}"#}}));
        }
        let wire=compact_historical(&messages,1);
        let summary=wire[0]["content"].as_str().unwrap();
        assert!(summary.contains("[outcome_unknown: 1"));
        assert!(summary.contains("id-11"));
        assert!(!summary.contains("id-10"));
    }
    #[test]
    fn source_text_mentioning_errors_is_not_mistaken_for_failed_execution(){
        assert_eq!(outcome_priority(&json!({"content":"fn error() { panic!(); }"})),0);
        assert_eq!(outcome_priority(&json!({"content":r#"{"ok":false,"error":"missing"}"#})),1);
        assert_eq!(outcome_priority(&json!({"content":r#"{"ok":false,"outcomeUnknown":true}"#})),2);
    }
    #[test]
    fn incomplete_or_duplicated_tool_frames_are_never_rewritten(){
        let messages=vec![
            json!({"role":"assistant","tool_calls":[
                call("a","workspace_read","{}"),call("b","workspace_read","{}")]}),
            json!({"role":"tool","tool_call_id":"a","content":"only-a"})];
        assert_eq!(bounded_tool_trace(&messages,1024),messages);
        let duplicated=vec![messages[0].clone(),
            messages[1].clone(),messages[1].clone()];
        assert_eq!(bounded_tool_trace(&duplicated,1024),duplicated);
    }
    #[test]
    fn many_tool_frames_fit_bounded_budget_without_mutating_stored_payloads(){
        let mut messages=vec![json!({"role":"user","content":"continue task"})];
        for index in 0..30{
            messages.extend(pair(&format!("call-{index}"),"workspace_read",
                r#"{"path":"src/a.rs"}"#,&"x".repeat(8000)));
        }
        let original=messages.clone();
        let wire=bounded_tool_trace(&messages,12_000);
        let tool_content=wire.iter().filter(|row|row["role"]=="tool")
            .map(|row|row["content"].as_str().unwrap_or("").len()).sum::<usize>();
        assert!(tool_content<=12_000);
        assert!(serde_json::to_string(&wire).unwrap().len()<35_000);
        assert_eq!(messages,original);
    }
}
