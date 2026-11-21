//! Durable, per-(room session, agent) delivery cursors, independent of
//! chat truncation, provider context windows or the owner-visible transcript.
//! A cursor is updated only in the transaction committing that agent's final
//! answer; failed/in-flight turns never acknowledge messages they did not read.
use serde_json::{json,Value};

pub(crate) const MAX_UNREAD_PER_TURN:usize=4;
pub(crate) const MAX_REQUIRED_UNREAD_CHARS:usize=4800;

pub(crate) fn binding_id(session:&str,agent:&str)->String {
    format!("room-cursor-{}",crate::engine::memory::stable_id(&[session,agent]))
}
#[derive(Debug)]
pub(crate) struct Window {
    pub required_indices:Vec<usize>,
    pub through_message_id:Option<String>,
    pub previous_message_id:Option<String>,
    pub unread_remaining:usize,
    pub oversized_first_unread:bool,
}
/// Return a contiguous prefix of unread *completed* chat messages. Tool
/// traffic and system/learning frames are excluded. The current owner turn
/// remains mandatory in the room compiler, even if the older backlog is large.
pub(crate) fn plan(
    messages:&[Value],session_id:&str,agent_id:&str,source_id:&str,cursor:Option<&Value>,
)->Result<Window,String>{
    let previous=if let Some(cursor)=cursor {
        if cursor["sessionId"].as_str()!=Some(session_id)
            || cursor["agentId"].as_str()!=Some(agent_id) {
            return Err("Room cursor binding does not match this agent or session".into());
        }
        Some(cursor["throughMessageId"].as_str()
            .filter(|v|!v.is_empty()).ok_or("Room cursor lacks a committed message anchor")?)
    }else{None};
    let after=match previous {
        Some(id)=>messages.iter().position(|m|m["id"].as_str()==Some(id))
            .ok_or("Room cursor points to a missing transcript record")?+1,
        // A newly joined agent starts with the triggering owner message,
        // while earlier room history remains available for indexed recall.
        None=>messages.iter().position(|m|m["id"].as_str()==Some(source_id)
            && m["session_id"].as_str()==Some(session_id)
            && m["role"]=="user"
            && m["metadata"]["authorKind"]=="owner")
            .ok_or("Room cursor has no matching owner trigger")?,
    };
    let unread=messages.iter().enumerate().skip(after).filter(|(_,m)|{
        m["session_id"].as_str()==Some(session_id)
            && ((m["role"]=="user"
                && m["metadata"]["authorKind"]=="owner")
             || (m["role"]=="assistant"
                && m["metadata"]["authorKind"]=="agent"
                && m["tool_calls"].as_array().is_none_or(Vec::is_empty)))
            && m["id"].as_str().is_some_and(|id|!id.is_empty())
    }).collect::<Vec<_>>();
    // First delivery includes the triggering owner message through the
    // mandatory room-context frame. Only acknowledge that trigger, never
    // later peer messages that the new agent did not receive as required
    // unread input. The next turn will drain those messages normally.
    // History before the trigger remains available to the recall ranker.
    if previous.is_none() {
        return Ok(Window {
            required_indices:Vec::new(),
            through_message_id:Some(source_id.to_string()),
            previous_message_id:None,
            unread_remaining:unread.len().saturating_sub(1),
            oversized_first_unread:false,
        });
    }
    let mut required=Vec::new();
    let mut chars=0usize;
    for (index,m) in unread.iter().take(MAX_UNREAD_PER_TURN) {
        let size=m["content"].as_str().unwrap_or("").chars().count();
        if chars.saturating_add(size)>MAX_REQUIRED_UNREAD_CHARS {break;}
        required.push(*index);
        chars+=size;
    }
    let through=required.last().and_then(|index|messages[*index]["id"].as_str())
        .map(str::to_string);
    let remaining=unread.len().saturating_sub(required.len());
    let oversized_first_unread=!unread.is_empty()&&required.is_empty();
    Ok(Window{
        required_indices:required,
        through_message_id:through.or_else(||previous.map(str::to_string)),
        previous_message_id:previous.map(str::to_string),
        unread_remaining:remaining,
        oversized_first_unread,
    })
}
pub(crate) fn checkpoint(session_id:&str,agent_id:&str,source_id:&str,through:&str)->Value {
    json!({"id":binding_id(session_id,agent_id),"sessionId":session_id,
        "agentId":agent_id,"sourceMessageId":source_id,
        "throughMessageId":through,"schemaVersion":1})
}
#[cfg(test)]
mod tests {
    use super::*;
    fn msg(id:&str,role:&str,session:&str,text:&str)->Value{
        json!({"id":id,"session_id":session,"role":role,"content":text,
            "metadata":{"authorKind":if role=="user"{"owner"}else{"agent"}},
            "tool_calls":[]})
    }
    #[test]
    fn each_delivery_is_a_contiguous_prefix_and_next_turn_starts_after_cursor(){
        let messages=(0..11).map(|i|msg(&format!("m{i}"),
            if i==0 {"user"} else {"assistant"},"room:one","short"))
            .collect::<Vec<_>>();
        let a=plan(&messages,"room:one","a","m0",None).unwrap();
        assert!(a.required_indices.is_empty(),"bootstrap uses existing context ranker");
        assert_eq!(a.through_message_id.as_deref(),Some("m0"));
        let cursor=checkpoint("room:one","a","m0","m0");
        let b=plan(&messages,"room:one","a","m0",Some(&cursor)).unwrap();
        assert_eq!(b.required_indices,vec![1,2,3,4]);
        assert_eq!(b.through_message_id.as_deref(),Some("m4"));
        assert_eq!(b.unread_remaining,6);
        let next=checkpoint("room:one","a","m0","m4");
        let again=plan(&messages,"room:one","a","m0",Some(&next)).unwrap();
        assert_eq!(again.required_indices,vec![5,6,7,8]);
        assert_eq!(again.through_message_id.as_deref(),Some("m8"));
        let other=plan(&messages,"room:one","b","m0",None).unwrap();
        assert!(other.required_indices.is_empty());
        assert_eq!(other.through_message_id.as_deref(),Some("m0"));
    }
    #[test]
    fn cursors_fail_closed_for_other_sessions_and_missing_anchors(){
        let messages=vec![msg("x","user","room:one","owner")];
        let wrong=checkpoint("room:other","a","owner","x");
        assert!(plan(&messages,"room:one","a","x",Some(&wrong)).is_err());
        let missing=checkpoint("room:one","a","owner","deleted");
        assert!(plan(&messages,"room:one","a","x",Some(&missing)).is_err());
    }
    #[test]
    fn oversized_unread_message_is_not_acked_or_silently_skipped(){
        let messages=vec![
            msg("previous","user","room:one","old"),
            msg("large","user","room:one",&"x".repeat(MAX_REQUIRED_UNREAD_CHARS+1)),
            msg("small","assistant","room:one","good"),
        ];
        let prior=checkpoint("room:one","a","previous","previous");
        let window=plan(&messages,"room:one","a","large",Some(&prior)).unwrap();
        assert!(window.required_indices.is_empty());
        assert_eq!(window.unread_remaining,2);
        assert!(window.oversized_first_unread);
        assert_eq!(window.through_message_id.as_deref(),Some("previous"));
    }
    #[test]
    fn tool_fragments_and_foreign_sessions_are_not_acknowledged(){
        let messages=vec![
            msg("other","user","room:other","wrong session"),
            json!({"id":"fragment","session_id":"room:one","role":"assistant",
                "metadata":{"authorKind":"agent"},"tool_calls":[{"id":"t"}]}),
            msg("owner","user","room:one","current"),
        ];
        let window=plan(&messages,"room:one","a","owner",None).unwrap();
        assert!(window.required_indices.is_empty());
        assert_eq!(window.through_message_id.as_deref(),Some("owner"));
    }
    #[test]
    fn long_room_replays_150_turns_once_per_agent_across_cursor_snapshots(){
        use std::collections::HashSet;
        let mut transcript=vec![msg("owner","user","room:one","begin")];
        for i in 1..=150 {
            transcript.push(msg(&format!("turn-{i}"),"assistant",
                "room:one",&format!("committed answer {i}")));
        }
        let mut a=checkpoint("room:one","agent-a","owner","owner");
        let mut consumed=HashSet::<String>::new();
        // A restart reconstructs only this saved record; no in-process
        // delivery cache or model context is needed to reconstruct the window.
        for _ in 0..150 {
            let batch=plan(&transcript,"room:one","agent-a","owner",Some(&a)).unwrap();
            if batch.required_indices.is_empty(){break;}
            assert!(batch.required_indices.len()<=MAX_UNREAD_PER_TURN);
            for idx in &batch.required_indices {
                let id=transcript[*idx]["id"].as_str().unwrap();
                assert!(consumed.insert(id.to_string()),
                    "a committed room message was delivered twice: {id}");
            }
            let through=batch.through_message_id.unwrap();
            a=checkpoint("room:one","agent-a","owner",&through);
        }
        assert_eq!(consumed.len(),150);
        assert_eq!(a["throughMessageId"],"turn-150");
        assert!(plan(&transcript,"room:one","agent-a","owner",Some(&a))
            .unwrap().required_indices.is_empty());
        let b=checkpoint("room:one","agent-b","owner","turn-100");
        let next=plan(&transcript,"room:one","agent-b","owner",Some(&b)).unwrap();
        assert_eq!(next.required_indices,vec![101,102,103,104]);
        assert_eq!(next.unread_remaining,46);
        assert_ne!(binding_id("room:one","agent-a"),binding_id("room:one","agent-b"));
        // Different rooms must never share a cursor, even with same agent IDs.
        assert!(plan(&transcript,"room:other","agent-a","owner",Some(&a)).is_err());
    }
    #[test]
    fn joining_agent_does_not_ack_peer_messages_after_the_owner_trigger(){
        let messages=vec![
            msg("ancient","assistant","room:one","unrelated prior task"),
            msg("trigger","user","room:one","new owner task"),
            msg("peer-one","assistant","room:one","first peer observation"),
            msg("peer-two","assistant","room:one","second peer observation"),
        ];
        let first=plan(&messages,"room:one","new-agent","trigger",None).unwrap();
        assert_eq!(first.through_message_id.as_deref(),Some("trigger"));
        assert!(first.required_indices.is_empty());
        let receipt=checkpoint("room:one","new-agent","trigger","trigger");
        let following=plan(&messages,"room:one","new-agent","trigger",Some(&receipt)).unwrap();
        assert_eq!(following.required_indices,vec![2,3]);
        assert_eq!(following.through_message_id.as_deref(),Some("peer-two"));
        assert_eq!(following.unread_remaining,0);
        let committed=checkpoint("room:one","new-agent","trigger","peer-two");
        assert!(plan(&messages,"room:one","new-agent","trigger",Some(&committed))
            .unwrap().required_indices.is_empty());
    }
    #[test]
    fn joining_a_long_room_does_not_pin_unrelated_ancient_speech(){
        let mut messages=(0..100).map(|i|msg(&format!("ancient-{i}"),
            "assistant","room:one","unrelated old history")).collect::<Vec<_>>();
        messages.push(msg("current","user","room:one","new owner task"));
        messages.push(msg("peer","assistant","room:one","reply on this task"));
        let window=plan(&messages,"room:one","new-agent","current",None).unwrap();
        assert!(window.required_indices.is_empty());
        assert_eq!(window.through_message_id.as_deref(),Some("current"));
        assert_eq!(window.unread_remaining,1);
        assert!(!window.required_indices.contains(&0));
    }

}
