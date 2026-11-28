//! Branch2's bounded, protocol-safe tool history policy, shared by Chat and SDK.
//! Only the outbound view changes; the host keeps the original journal.
use serde_json::{json,Value};
use std::collections::HashSet;

pub struct Projection {
    pub messages:Vec<Value>,
    pub original_bytes:usize,
    pub projected_bytes:usize,
    pub compacted_frames:usize,
}
struct Frame { start:usize,end:usize,priority:u8 }
fn priority(row:&Value)->u8 {
    fn outcome(v:&Value)->u8 {
        if v["outcomeUnknown"]==true || v["effectUnknown"]==true || matches!(v["status"].as_str(),Some("outcome_unknown"|"effect_unknown")){return 2;}
        if v["success"]==false || v["ok"]==false || v["isError"]==true || v["dispatched"]==false
            || matches!(v["status"].as_str(),Some("failed"|"error"|"cancelled"|"not_dispatched"))
            || v.get("error").is_some_and(|e|!e.is_null()&&e!=false&&e.as_str()!=Some("")){return 1;}
        0
    }
    let inner=match &row["content"] {
        Value::String(s)=>serde_json::from_str::<Value>(s).ok().map(|v|outcome(&v)).unwrap_or(0),
        v=>outcome(v),
    };
    outcome(row).max(inner)
}
fn frame_end(messages:&[Value],start:usize)->Option<usize>{
    if messages[start]["role"]!="assistant"{return None;}
    let calls=messages[start]["tool_calls"].as_array().filter(|v|!v.is_empty())?;
    let mut ids=HashSet::new();
    for call in calls {let id=call["id"].as_str().filter(|s|!s.is_empty())?;if !ids.insert(id){return None;}}
    let mut end=start+1;
    while end<messages.len()&&messages[end]["role"]=="tool"{end+=1;}
    if end-start-1!=calls.len(){return None;}
    let mut results=HashSet::new();
    for row in &messages[start+1..end]{let id=row["tool_call_id"].as_str()?;if !ids.contains(id)||!results.insert(id){return None;}}
    Some(end)
}
fn bytes(rows:&[Value])->usize{rows.iter().map(|r|r.to_string().len()).fold(0,usize::saturating_add)}
fn summary(rows:&[Value],priority:u8)->Value{
    let receipts=rows.iter().skip(1).take(4).map(|r|json!({
        "callId":r["tool_call_id"],"sourceMessageId":r["_dominion_frame"],
        "excerpt":r["content"].as_str().unwrap_or("").chars().take(220).collect::<String>()
    })).collect::<Vec<_>>();
    json!({"role":"assistant","content":json!({
        "kind":"historical_tool_receipts","untrustedObservationData":true,
        "outcome":if priority==2{"effect_unknown_do_not_replay"}else{"failed_not_verified"},
        "receipts":receipts,"originalTranscriptRetained":true
    }).to_string()})
}
/// The latest complete tool group and all non-tool messages remain exact.
/// Older uncertain effects and failures precede successful reads in the byte
/// allowance. Malformed/incomplete groups are preserved, never partially cut.
pub fn project(messages:&[Value],budget:usize)->Projection{
    let mut frames=Vec::new();let mut cursor=0;
    while cursor<messages.len(){
        if let Some(end)=frame_end(messages,cursor){
            frames.push(Frame{start:cursor,end,priority:messages[cursor+1..end].iter().map(priority).max().unwrap_or(0)});cursor=end;
        }else{cursor+=1;}
    }
    let mut selected=vec![None;frames.len()];
    let mut remaining=budget;
    let mut order=(0..frames.len()).collect::<Vec<_>>();
    order.sort_by_key(|&i|std::cmp::Reverse((i+1==frames.len(),frames[i].priority,i)));
    let mut omitted_failures=0;let mut omitted_unknown=0;
    for i in order{
        let frame=&frames[i];let size=bytes(&messages[frame.start..frame.end]);
        if i+1==frames.len() || size<=remaining {
            remaining=remaining.saturating_sub(size);selected[i]=Some(messages[frame.start..frame.end].to_vec());
        }else if frame.priority>0 {
            let note=summary(&messages[frame.start..frame.end],frame.priority);
            let size=bytes(std::slice::from_ref(&note));
            if size<=remaining{remaining-=size;selected[i]=Some(vec![note]);}
            else if frame.priority==2{omitted_unknown+=1;}else{omitted_failures+=1;}
        }
    }
    let mut output=Vec::new();let mut cursor=0;let mut compacted=0;
    for (frame,selected) in frames.iter().zip(selected){
        output.extend_from_slice(&messages[cursor..frame.start]);
        if let Some(rows)=selected{if rows!=messages[frame.start..frame.end]{compacted+=1;}output.extend(rows);}
        else{compacted+=1;}
        cursor=frame.end;
    }
    output.extend_from_slice(&messages[cursor..]);
    if omitted_failures+omitted_unknown>0{
        output.push(json!({"role":"user","content":json!({
            "kind":"historical_tool_receipt_warning","omittedFailures":omitted_failures,
            "omittedUnknownEffects":omitted_unknown,"originalTranscriptRetained":true,
            "unknownEffectsMustBeReconciledBeforeReplay":true
        }).to_string()}));
    }
    Projection{original_bytes:bytes(messages),projected_bytes:bytes(&output),messages:output,compacted_frames:compacted}
}

#[cfg(test)]
mod tests{
    use super::*;
    fn frame(id:&str,result:Value)->Vec<Value>{vec![
        json!({"role":"assistant","tool_calls":[{"id":id,"function":{"name":"workspace_read","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":id,"content":result.to_string()})]}
    #[test]
    fn bounded_old_frames_keep_latest_exact_and_do_not_modify_the_journal(){
        let mut rows=vec![json!({"role":"system","content":"owner policy"})];
        rows.extend(frame("old",json!({"text":"x".repeat(100_000)})));
        let latest=frame("now",json!({"text":"한글 source".repeat(3000)}));rows.extend(latest.clone());
        rows.push(json!({"role":"user","content":"repair this exact source"}));
        let original=rows.clone();let p=project(&rows,1000);
        assert_eq!(rows,original);assert_eq!(p.messages[0],rows[0]);
        assert_eq!(&p.messages[1..3],latest);assert_eq!(p.messages.last(),rows.last());
        assert_eq!(p.compacted_frames,1);assert!(p.projected_bytes<p.original_bytes);
    }
    #[test]
    fn failures_and_unknown_effects_are_not_silently_treated_as_success(){
        let mut rows=frame("unknown",json!({"outcomeUnknown":true,"text":"x".repeat(4000)}));
        rows.extend(frame("failed",json!({"ok":false,"error":"E0308"})));
        rows.extend(frame("new",json!({"ok":true})));
        let p=project(&rows,0);let wire=serde_json::to_string(&p.messages).unwrap();
        assert!(wire.contains("omittedUnknownEffects"));assert!(wire.contains("omittedFailures"));
        assert!(wire.contains("unknownEffectsMustBeReconciledBeforeReplay"));
        assert_eq!(p.messages.iter().filter(|m|m["role"]=="tool").count(),1);
    }
    #[test]
    fn incomplete_duplicate_and_foreign_ids_never_get_split(){
        for bad in [vec![json!({"role":"assistant","tool_calls":[{"id":"a"},{"id":"a"}]}),json!({"role":"tool","tool_call_id":"a","content":"one"})],
            vec![json!({"role":"assistant","tool_calls":[{"id":"a"}]}),json!({"role":"tool","tool_call_id":"foreign","content":"two"})],
            vec![json!({"role":"assistant","tool_calls":[{"id":"pending"}]})]]{
            assert_eq!(project(&bad,0).messages,bad);
        }
    }
}
