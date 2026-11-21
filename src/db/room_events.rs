//! Per-room durable event sequence; not a separate agent loop.
//! A RoomEvent is a committed transcript fact, not a model-generated claim.
//! Exactly-once event recording shares the SQLite transaction with owner
//! message creation or the participant read-cursor advancement.
use super::Store;
use rusqlite::{params,Connection,OptionalExtension};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RoomEvent{
    pub schema_version:u16,
    pub room_id:String,
    pub seq:u64,
    pub event_id:String,
    pub author:String,
    pub reply_to:Option<String>,
    pub origin_run:Option<String>,
    pub kind:String,
    pub created_at:String,
}
pub(super) fn insert_room_event(
    conn:&Connection,room_id:&str,session_id:&str,message_id:&str,kind:&str,
)->Result<Option<u64>,String>{
    if room_id.is_empty()||room_id.len()>256||session_id.is_empty()
        ||message_id.is_empty()||message_id.len()>256 {
        return Err("room event has an invalid identity".into());
    }
    let owned=conn.query_row(
        "SELECT json_extract(data,'$.roomId') FROM records
         WHERE kind='session' AND id=?1",
        [session_id],|row|row.get::<_,Option<String>>(0)
    ).optional().map_err(|e|e.to_string())?.flatten();
    if owned.as_deref()!=Some(room_id){
        return Err("room event references an unbound transcript".into());
    }
    let record=conn.query_row(
        "SELECT role,agent_id,metadata FROM messages
         WHERE id=?1 AND session_id=?2",params![message_id,session_id],
        |row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,
            row.get::<_,String>(2)?))
    ).optional().map_err(|e|e.to_string())?
        .ok_or("room event message is not committed")?;
    let metadata:Value=serde_json::from_str(&record.2).map_err(|e|e.to_string())?;
    if metadata["roomId"].as_str().is_some_and(|id|id!=room_id){
        return Err("room event metadata crosses a room".into());
    }
    let (author,reply_to,origin_run)=match kind{
        "owner_message" if record.0=="user"&&record.1.is_none()
            &&metadata["authorKind"]=="owner"=>{
            ("owner".to_owned(),None,None)
        },
        "agent_reply" if record.0=="assistant"&&record.1.is_some()
            &&metadata["authorKind"]=="agent"=>{
            let parent=metadata["replyToMessageId"].as_str()
                .filter(|s|!s.is_empty()).ok_or("room reply has no source event")?;
            // Older transcripts may predate this projection; bootstrap only
            // the real owner event with matching room and session provenance.
            insert_room_event(conn,room_id,session_id,parent,"owner_message")?;
            let run=metadata["executionId"].as_str().filter(|id|!id.is_empty());
            if let Some(run)=run{
                let owner=conn.query_row(
                    "SELECT json_extract(data,'$.sessionId') FROM records
                     WHERE kind='execution' AND id=?1",
                    [run],|row|row.get::<_,Option<String>>(0)
                ).optional().map_err(|e|e.to_string())?.flatten();
                if owner.as_deref()!=Some(session_id){
                    return Err("room reply references a foreign execution".into());
                }
            }
            (record.1.unwrap_or_default(),Some(parent.to_owned()),run.map(str::to_owned))
        },
        _=>return Err("room event provenance or kind does not match the message".into())
    };
    // Never allocate a new sequence for the same committed message.
    if let Some(seq)=conn.query_row(
        "SELECT seq FROM room_events WHERE room_id=?1 AND event_id=?2",
        params![room_id,message_id],|row|row.get::<_,i64>(0)
    ).optional().map_err(|e|e.to_string())? {
        return Ok(Some(seq as u64));
    }
    let seq=conn.query_row(
        "SELECT COALESCE(MAX(seq),0)+1 FROM room_events WHERE room_id=?1",
        [room_id],|row|row.get::<_,i64>(0)
    ).map_err(|e|e.to_string())?;
    conn.execute(
        "INSERT INTO room_events(room_id,seq,event_id,author,reply_to,origin_run,kind)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![room_id,seq,message_id,author,reply_to,origin_run,kind]
    ).map_err(|e|e.to_string())?;
    Ok(Some(seq as u64))
}

impl Store{
    /// Cursor is EXCLUSIVE and scoped to this room. This is deliberately
    /// different from the globally ordered SSE event_log cursor.
    pub fn room_events(&self,room_id:&str,after:u64,limit:usize)->Result<Vec<RoomEvent>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT room_id,seq,event_id,author,reply_to,origin_run,kind,created_at
             FROM room_events WHERE room_id=?1 AND seq>?2 ORDER BY seq ASC LIMIT ?3"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(
            params![room_id,after.min(i64::MAX as u64) as i64,limit.min(501) as i64],
            |row|Ok(RoomEvent{
                schema_version:1,
                room_id:row.get(0)?,seq:row.get::<_,i64>(1)? as u64,
                event_id:row.get(2)?,author:row.get(3)?,reply_to:row.get(4)?,
                origin_run:row.get(5)?,kind:row.get(6)?,created_at:row.get(7)?,
            })
        ).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn owner(store:&Store,id:&str,room:&str,session:&str){
        store.write_message(&json!({"id":id,"session_id":session,"role":"user",
            "content":"owner","metadata":{"roomId":room,"authorKind":"owner"}})).unwrap();
    }
    fn reply(store:&Store,id:&str,room:&str,session:&str,owner_id:&str){
        store.write_message(&json!({"id":id,"session_id":session,
            "role":"assistant","agent_id":"a","content":"agent",
            "metadata":{"roomId":room,"authorKind":"agent",
                "replyToMessageId":owner_id}})).unwrap();
        assert!(store.commit_room_agent_read(room,session,"a",owner_id,id,
            &[json!(owner_id)]).unwrap());
    }
    #[test]
    fn room_event_journal_survives_120_turns_restart_and_exactly_once_cursors(){
        let dir=std::env::temp_dir().join(format!("dominion-room-events-{}",uuid::Uuid::new_v4()));
        let db=dir.join("db.sqlite");
        {
            let store=Store::open(&db).unwrap();
            store.put("room",&json!({"id":"r","sessionId":"room:r"})).unwrap();
            store.put("session",&json!({"id":"room:r","roomId":"r"})).unwrap();
            for index in 0..120{
                let o=format!("owner-{index:03}");
                let a=format!("agent-{index:03}");
                owner(&store,&o,"r","room:r");
                reply(&store,&a,"r","room:r",&o);
                assert!(!store.commit_room_agent_read(
                    "r","room:r","a",&o,&a,&[]).unwrap());
            }
            let events=store.room_events("r",0,500).unwrap();
            assert_eq!(events.len(),240);
            for (i,e) in events.iter().enumerate(){
                assert_eq!(e.seq,i as u64+1);
                assert_eq!(e.kind,if i%2==0{"owner_message"}else{"agent_reply"});
                if i%2==1{
                    assert_eq!(e.reply_to.as_deref(),Some(events[i-1].event_id.as_str()));
                }
            }
            let first=store.room_events("r",0,31).unwrap();
            assert_eq!(first.len(),31);
            assert_eq!(store.room_events("r",31,400).unwrap().len(),209);
            assert!(store.room_events("another-room",0,400).unwrap().is_empty());
        }
        {
            let reopened=Store::open(&db).unwrap();
            assert_eq!(reopened.room_events("r",239,10).unwrap()[0].seq,240);
            assert!(reopened.room_events("r",240,10).unwrap().is_empty());
            assert!(!reopened.commit_room_agent_read(
                "r","room:r","a","owner-119","agent-119",&[]).unwrap());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn owner_room_event_rolls_back_if_job_insert_fails_and_other_rooms_never_leak(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("room",&json!({"id":"r","sessionId":"room:r"})).unwrap();
        store.put("session",&json!({"id":"room:r","roomId":"r"})).unwrap();
        let msg=json!({"id":"submitted","session_id":"room:r","role":"user",
            "content":"hello","metadata":{"authorKind":"owner","roomId":"r"}});
        assert!(store.write_message_with_record(&msg,"job",&json!({"status":"queued"})).is_none());
        assert!(store.message("submitted").is_none());
        assert!(store.room_events("r",0,10).unwrap().is_empty());
        assert!(store.write_message(&json!({"id":"foreign",
            "session_id":"room:r","role":"user","metadata":{
                "authorKind":"owner","roomId":"unrelated"}})).is_none());
        assert!(store.room_events("r",0,10).unwrap().is_empty());
        assert!(store.write_message_with_record(&msg,"job",
            &json!({"id":"work","status":"queued"})).is_some());
        assert_eq!(store.room_events("r",0,10).unwrap()[0].event_id,"submitted");
    }
}
