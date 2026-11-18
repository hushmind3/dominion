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

fn note(messages:&[Value],start:usize,end:usize)->Value{
    let assistant=&messages[start];
    let calls=assistant["tool_calls"].as_array().expect("completed tool group");
    let results=&messages[start+1..end];
    let mut lines=Vec::new();
    let content=assistant["content"].as_str().unwrap_or("");
    if !content.trim().is_empty(){
        lines.push(format!("모델 설명: {}",narrow(content,160)));
    }
    for call in calls.iter().take(8){
        let id=call["id"].as_str().unwrap_or("");
        let name=call["function"]["name"].as_str().unwrap_or("unknown");
        let raw=call["function"]["arguments"].as_str().unwrap_or("");
        let path=serde_json::from_str::<Value>(raw).ok()
            .and_then(|obj|obj["path"].as_str().map(str::to_owned))
            .map(|path|format!(" path={}",narrow(&path,110))).unwrap_or_default();
        let result=results.iter().find(|r|r["tool_call_id"]==id);
        let outcome=result.and_then(|r|r["content"].as_str()).unwrap_or("");
        lines.push(format!("{} (id={}){} → {}",
            narrow(name,65),narrow(id,70),path,narrow(outcome,270)));
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
pub(super) fn bounded_tool_trace(messages:&[Value],budget_bytes:usize)->Vec<Value>{
    if messages.is_empty(){return Vec::new();}
    let mut spans=Vec::new();
    let mut cursor=0usize;
    while cursor<messages.len(){
        if let Some(end)=completed_frame_end(messages,cursor){
            let size=messages[cursor..end].iter()
                .map(|row|row.to_string().len()).sum::<usize>();
            spans.push(Span{start:cursor,end,frame_bytes:size,
                note:None,mode:1});
            cursor=end;
        }else{
            spans.push(Span{start:cursor,end:cursor+1,frame_bytes:0,
                note:None,mode:0});
            cursor+=1;
        }
    }
    let mut remaining=budget_bytes.max(1024);
    for span in spans.iter_mut().rev(){
        if span.mode==0{continue;}
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
        assert!(!subsequent.to_string().contains(&source[..500]));
        assert!(subsequent.to_string().len()<transcript.to_string().len()/3);
        // A narrow route cannot opt in to unlimited source replay.
        let constrained=bounded_tool_trace(&transcript[..3],8_000);
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
        assert!(!wire.to_string().contains("LARGE_WRITE_CONTENT_LARGE_WRITE_CONTENT_"));
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
