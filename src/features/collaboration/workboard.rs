//! Evidence-carrying group workboard. A pure, bounded projection of committed
//! host operations; not a chat transcript, new owner instruction, or success verdict.
//! The host supplies scoped operations. This module never reads SQLite or calls a model.
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Component, Path};

const MAX_SPEAKERS: usize = 6;
const MAX_OPERATIONS_PER_SPEAKER: usize = 3;

fn safe_workspace_path(operation: &Value) -> Option<&str> {
    let tool=operation["tool"].as_str()?;
    if !matches!(tool,"workspace_write"|"workspace_replace"|"workspace_replace_range"
        |"workspace_read"|"workspace_list") {return None;}
    let path=operation["arguments"]["path"].as_str()?;
    if path.is_empty() || path.len()>256 || !Path::new(path).components()
        .all(|part|matches!(part,Component::Normal(_))) {return None;}
    Some(path)
}

/// Build a scoped, chronological handoff for the CURRENT owner message.
/// The resolver must return only operations from the matching host execution;
/// a second runId/session check below prevents cross-session pollution.
/// Never promote an agent statement or an effect without a verifier receipt
/// into a verified result. Complete transcripts remain in durable storage.
pub(crate) fn project<F>(
    session_id:&str,source_id:&str,allowed_agents:&[Value],
    messages:&[Value],mut resolve:F
)->Option<Value>
where F:FnMut(&str,&str)->Vec<Value> {
    let roster=allowed_agents.iter().filter_map(Value::as_str).collect::<HashSet<_>>();
    let mut seen=HashSet::new();
    let mut contributions=Vec::new();
    for message in messages.iter().rev() {
        if contributions.len()>=MAX_SPEAKERS {break;}
        if message["session_id"].as_str()!=Some(session_id)
            || message["role"]!="assistant"
            || message["metadata"]["authorKind"]!="agent"
            || message["metadata"]["replyToMessageId"].as_str()!=Some(source_id)
            || message["tool_calls"].as_array().is_some_and(|v|!v.is_empty()) {continue;}
        let Some(agent_id)=message["agent_id"].as_str().filter(|id|roster.contains(id)) else {continue;};
        let Some(message_id)=message["id"].as_str().filter(|id|!id.is_empty()) else {continue;};
        if !seen.insert(agent_id.to_string()) {continue;}
        let Some(execution_id)=message["metadata"]["executionId"].as_str().filter(|id|!id.is_empty()) else {continue;};
        let mut operations=resolve(execution_id,agent_id).into_iter().rev()
            .filter(|op|op["runId"].as_str()==Some(execution_id)
                && op["sessionId"].as_str().is_none_or(|s|s==session_id)
                && op["status"]=="completed"
                && op["id"].as_str().is_some_and(|id|!id.is_empty())
                && op["tool"].as_str().is_some_and(|name|!name.is_empty()))
            .take(MAX_OPERATIONS_PER_SPEAKER)
            .map(|op|{
                let verified=op["effectState"]=="verified"
                    && op["toolReceipt"]["verified"]==true;
                json!({
                    "operationId":op["id"], "tool":op["tool"],
                    "effectState":op["effectState"],
                    "verifiedEffect":verified,
                    // A workspace path is a pointer, not a copy of the file
                    // or the untrusted/possibly secret tool arguments.
                    "artifactPath":safe_workspace_path(&op),
                })
            }).collect::<Vec<_>>();
        if operations.is_empty() {continue;}
        operations.reverse();
        contributions.push(json!({
            "agentId":agent_id, "messageId":message_id,
            "executionId":execution_id, "operations":operations,
        }));
    }
    if contributions.is_empty(){return None;}
    contributions.reverse();
    Some(json!({
        "format":"room-workboard-v1",
        "sessionId":session_id,"sourceMessageId":source_id,
        "contributions":contributions,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn msg(agent:&str,session:&str,owner:&str,exec:&str,id:&str)->Value{
        json!({"id":id,"session_id":session,"role":"assistant","agent_id":agent,
            "content":"I finished everything","tool_calls":[],
            "metadata":{"authorKind":"agent","replyToMessageId":owner,"executionId":exec}})
    }
    fn op(id:&str,run:&str,session:&str,path:&str,state:&str,verified:bool)->Value{
        json!({"id":id,"runId":run,"sessionId":session,"status":"completed",
            "tool":"workspace_write","arguments":{"path":path,"content":"secret"},
            "effectState":state,"toolReceipt":{"verified":verified}})
    }
    fn roster()->Vec<Value>{vec![json!("a"),json!("b")]}
    #[test]
    fn verified_receipt_is_distinct_from_an_agent_claim_or_unverified_effect(){
        let messages=vec![msg("a","room:one","owner","exec","answer")];
        let board=project("room:one","owner",&roster(),&messages,|_,_|vec![
            op("observed","exec","room:one","src/main.rs","applied",false),
            op("verified","exec","room:one","src/lib.rs","verified",true),
        ]).unwrap();
        let ops=board["contributions"][0]["operations"].as_array().unwrap();
        assert_eq!(ops.len(),2);
        assert_eq!(ops[0]["verifiedEffect"],false);
        assert_eq!(ops[1]["verifiedEffect"],true);
        assert_eq!(ops[1]["artifactPath"],"src/lib.rs");
        assert!(!board.to_string().contains("I finished everything"));
        assert!(!board.to_string().contains("secret"));
    }
    #[test]
    fn owner_session_roster_and_execution_ids_are_all_required(){
        let records=vec![
            msg("a","room:one","old","x","old"),
            msg("a","room:other","owner","x","cross-room"),
            msg("outsider","room:one","owner","x","outsider"),
            msg("a","room:one","owner","legit","valid"),
        ];
        let board=project("room:one","owner",&roster(),&records,|execution,_|vec![
            op("wrong-run","another","room:one","src/mix.rs","verified",true),
            op("wrong-session",execution,"room:other","src/mix.rs","verified",true),
            op("correct",execution,"room:one","src/good.rs","verified",true),
        ]).unwrap();
        assert_eq!(board["contributions"].as_array().unwrap().len(),1);
        assert_eq!(board["contributions"][0]["messageId"],"valid");
        assert_eq!(board["contributions"][0]["operations"].as_array().unwrap().len(),1);
        assert_eq!(board["contributions"][0]["operations"][0]["operationId"],"correct");
    }
    #[test]
    fn path_fields_cannot_escape_workspace_or_disclose_arbitrary_arguments(){
        let records=vec![msg("a","room:one","owner","exec","answer")];
        let board=project("room:one","owner",&roster(),&records,|_,_|vec![
            op("bad","exec","room:one","../secret","verified",true),
            op("abs","exec","room:one","/private/file","verified",true),
            op("good","exec","room:one","src/public.rs","verified",true),
        ]).unwrap();
        let ops=board["contributions"][0]["operations"].as_array().unwrap();
        assert!(ops[0]["artifactPath"].is_null());
        assert!(ops[1]["artifactPath"].is_null());
        assert_eq!(ops[2]["artifactPath"],"src/public.rs");
        assert!(!board.to_string().contains("secret"));
    }
    #[test]
    fn workboard_is_bounded_and_keeps_latest_committed_per_agent(){
        let allowed=(0..12).map(|i|json!(format!("agent{i}"))).collect::<Vec<_>>();
        let messages=(0..12).map(|i|msg(&format!("agent{i}"),
            "room:one","owner",&format!("run{i}"),&format!("msg{i}")))
            .collect::<Vec<_>>();
        let mk=||project("room:one","owner",&allowed,&messages,|run,_|{
            (0..10).map(|i|op(&format!("{run}:{i}"),run,"room:one","src/x.rs","verified",true))
                .collect::<Vec<_>>()
        }).unwrap();
        let first=mk();
        assert_eq!(first,mk(),"identical source data must have a stable frame");
        let parts=first["contributions"].as_array().unwrap();
        assert_eq!(parts.len(),MAX_SPEAKERS);
        assert_eq!(parts[0]["agentId"],"agent6");
        assert_eq!(parts[5]["agentId"],"agent11");
        assert!(parts.iter().all(|row|
            row["operations"].as_array().unwrap().len()==MAX_OPERATIONS_PER_SPEAKER));
    }
}
