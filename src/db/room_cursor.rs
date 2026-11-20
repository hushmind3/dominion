//! Durable per-participant room delivery position.
//!
//! Room chat stays a shared transcript. Each agent has its own *committed*
//! response position. Model-visible selections are recorded as exact IDs,
//! never as an assertion that every prior message entered the token window.
use super::*;
impl Store {
    /// Commit a completed reply as the participant's new durable position.
    /// Validation and monotonic advancement share one SQLite transaction.
    /// Repeated rounds under one owner message are valid: the reply rowid
    /// must grow, while older owners cannot overwrite newer delivery.
    pub fn commit_room_agent_read(
        &self,room_id:&str,session_id:&str,agent_id:&str,
        owner_id:&str,reply_id:&str,selected:&[Value],
    )->Result<bool,String>{
        for (key,val) in [("room",room_id),("session",session_id),
            ("agent",agent_id),("owner",owner_id),("reply",reply_id)]{
            if val.is_empty()||val.len()>256 {return Err(format!("invalid {key} cursor ID"));}
        }
        if selected.len()>128{return Err("room read selection exceeds bound".into());}
        let ids=selected.iter().map(|row|row.as_str().filter(|id|!id.is_empty()&&id.len()<=256)
            .ok_or("room read selection contains an invalid message ID")
            .map(str::to_string)).collect::<Result<Vec<_>,_>>()?;
        let selected_json=serde_json::to_string(&ids).map_err(|e|e.to_string())?;
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let session_room=tx.query_row(
            "SELECT json_extract(data,'$.roomId') FROM records
             WHERE kind='session' AND id=?1",
            [session_id],|row|row.get::<_,Option<String>>(0)
        ).optional().map_err(|e|e.to_string())?.flatten();
        if session_room.as_deref()!=Some(room_id) {
            return Err("room cursor references an unbound session".into());
        }
        let get_message=|id:&str|->Result<(i64,String,Option<String>,Value),String>{
            tx.query_row(
                "SELECT rowid,role,agent_id,metadata FROM messages
                 WHERE id=?1 AND session_id=?2",
                params![id,session_id],|row|{
                    let raw:String=row.get(3)?;
                    Ok((row.get(0)?,row.get(1)?,row.get(2)?,
                        serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)))
                }
            ).optional().map_err(|e|e.to_string())?
             .ok_or_else(||format!("room cursor message missing: {id}"))
        };
        let (owner_row,owner_role,owner_agent,owner_meta)=get_message(owner_id)?;
        if owner_role!="user"||owner_agent.is_some()||owner_meta["authorKind"]!="owner"{
            return Err("room cursor owner source is not a committed owner message".into());
        }
        let (reply_row,reply_role,reply_agent,reply_meta)=get_message(reply_id)?;
        if reply_row<=owner_row||reply_role!="assistant"||
           reply_agent.as_deref()!=Some(agent_id)||
           reply_meta["replyToMessageId"]!=owner_id||
           reply_meta["authorKind"]!="agent" {
            return Err("room cursor reply lacks matching committed provenance".into());
        }
        // Only the exact selected context message IDs may be claimed as seen.
        // All IDs must predate the committed reply and belong to this session.
        for id in &ids{
            let (rowid,_,_,_)=get_message(id)?;
            if rowid>=reply_row {
                return Err("room cursor selection cannot include future messages".into());
            }
        }
        // A stale worker cannot move the participant back to an earlier
        // owner or retry an already committed reply.
        let changed=tx.execute(
            "INSERT INTO room_agent_read_cursors(
                room_id,session_id,agent_id,source_message_id,reply_message_id,
                source_rowid,reply_rowid,selected_message_ids
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(room_id,session_id,agent_id) DO UPDATE SET
                source_message_id=excluded.source_message_id,
                reply_message_id=excluded.reply_message_id,
                source_rowid=excluded.source_rowid,
                reply_rowid=excluded.reply_rowid,
                selected_message_ids=excluded.selected_message_ids,
                updated_at=datetime('now')
             WHERE excluded.source_rowid>=room_agent_read_cursors.source_rowid
               AND excluded.reply_rowid>room_agent_read_cursors.reply_rowid",
            params![room_id,session_id,agent_id,owner_id,reply_id,
                owner_row,reply_row,selected_json]
        ).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(changed==1)
    }

    /// Read one participant's last committed reply without scanning history.
    pub fn room_agent_read(
        &self,room_id:&str,session_id:&str,agent_id:&str,
    )->Result<Option<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT source_message_id,reply_message_id,source_rowid,reply_rowid,
                    selected_message_ids,updated_at
               FROM room_agent_read_cursors
              WHERE room_id=?1 AND session_id=?2 AND agent_id=?3",
            params![room_id,session_id,agent_id],|row|{
                let selected:String=row.get(4)?;
                Ok(json!({"roomId":room_id,"sessionId":session_id,"agentId":agent_id,
                    "sourceMessageId":row.get::<_,String>(0)?,
                    "replyMessageId":row.get::<_,String>(1)?,
                    "sourceCursor":row.get::<_,i64>(2)?,
                    "replyCursor":row.get::<_,i64>(3)?,
                    "selectedMessageIds":serde_json::from_str::<Value>(&selected)
                        .unwrap_or_else(|_|json!([])),
                    "updatedAt":row.get::<_,String>(5)?}))
            }
        ).optional().map_err(|e|e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup()->Store{
        let store=Store::open(Path::new(":memory:")).unwrap();
        store.put("room",&json!({"id":"r","sessionId":"room:r"})).unwrap();
        store.put("session",&json!({"id":"room:r","roomId":"r"})).unwrap();
        store.put("session",&json!({"id":"room:x","roomId":"x"})).unwrap();
        store
    }
    fn msg(store:&Store,id:&str,session:&str,role:&str,agent:Option<&str>,reply:Option<&str>){
        store.write_message(&json!({"id":id,"session_id":session,"role":role,
            "agent_id":agent,"content":"test",
            "metadata":{"authorKind":if role=="user"{"owner"}else{"agent"},
                "replyToMessageId":reply}})).unwrap();
    }
    #[test]
    fn per_agent_cursor_survives_duplicate_rounds_and_refuses_regression(){
        let store=setup();
        msg(&store,"owner1","room:r","user",None,None);
        msg(&store,"a1","room:r","assistant",Some("a"),Some("owner1"));
        assert!(store.commit_room_agent_read("r","room:r","a","owner1","a1",
            &[json!("owner1")]).unwrap());
        assert!(!store.commit_room_agent_read("r","room:r","a","owner1","a1",
            &[json!("owner1")]).unwrap());
        msg(&store,"b1","room:r","assistant",Some("b"),Some("owner1"));
        assert!(store.commit_room_agent_read("r","room:r","b","owner1","b1",
            &[json!("owner1")]).unwrap());
        msg(&store,"a2","room:r","assistant",Some("a"),Some("owner1"));
        assert!(store.commit_room_agent_read("r","room:r","a","owner1","a2",
            &[json!("owner1"),json!("b1")]).unwrap());
        msg(&store,"owner2","room:r","user",None,None);
        msg(&store,"a3","room:r","assistant",Some("a"),Some("owner2"));
        assert!(store.commit_room_agent_read("r","room:r","a","owner2","a3",
            &[json!("owner2")]).unwrap());
        assert!(!store.commit_room_agent_read("r","room:r","a","owner1","a2",&[]).unwrap());
        let cursor=store.room_agent_read("r","room:r","a").unwrap().unwrap();
        assert_eq!(cursor["replyMessageId"],"a3");
        assert_eq!(cursor["sourceMessageId"],"owner2");
        assert_eq!(cursor["selectedMessageIds"],json!(["owner2"]));
        assert_eq!(store.room_agent_read("r","room:r","b").unwrap().unwrap()["replyMessageId"],"b1");
    }
    #[test]
    fn cursor_rejects_cross_room_agent_and_uncommitted_or_future_messages(){
        let store=setup();
        msg(&store,"owner1","room:r","user",None,None);
        msg(&store,"a1","room:r","assistant",Some("a"),Some("owner1"));
        msg(&store,"owner2","room:r","user",None,None);
        msg(&store,"foreign","room:x","assistant",Some("a"),Some("owner1"));
        for (room,session,agent,owner,reply,selected) in [
            ("x","room:r","a","owner1","a1",vec![]),
            ("r","room:x","a","owner1","foreign",vec![]),
            ("r","room:r","b","owner1","a1",vec![]),
            ("r","room:r","a","owner1","a1",vec![json!("owner2")]),
        ]{
            assert!(store.commit_room_agent_read(room,session,agent,owner,reply,&selected).is_err());
        }
        assert!(store.room_agent_read("r","room:r","a").unwrap().is_none());
    }
}
