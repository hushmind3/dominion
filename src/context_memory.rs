//! Persistent, evidence-linked context retrieval and owner directive ledger.
//! This module indexes original conversation and tool-event text without
//! rewriting source messages. Model suggestions become usable checkpoint facts
//! only when their cited owner quote exactly matches persisted owner input.
use crate::store::Store;
use rusqlite::{params,OptionalExtension};
use serde_json::{json,Value};

fn words(query:&str)->Vec<String>{
    let mut terms=Vec::new();
    for word in query.split(|ch:char| !ch.is_alphanumeric()){
        if word.chars().count()<2 {continue;}
        let normalized=word.to_lowercase();
        if !terms.contains(&normalized) {
            terms.push(normalized);
        }
        if terms.len()==12{break;}
    }
    terms
}
fn excerpt(body:&str,terms:&[String])->String{
    let search=body.to_lowercase();
    let near=terms.iter().filter_map(|term|search.find(term)).min().unwrap_or(0);
    let mut start=near.saturating_sub(180).min(body.len());
    while !body.is_char_boundary(start){start-=1;}
    let mut end=start.saturating_add(750).min(body.len());
    while !body.is_char_boundary(end){end-=1;}
    format!("{}{}{}",if start>0{"…"}else{""},&body[start..end],
        if end<body.len(){"…"}else{""})
}
fn instruction_hint(text:&str)->bool{
    let lower=text.to_lowercase();
    [
        "반드시","절대","항상","하지 마","하지마","금지","잊지","우선",
        "원칙","기억해","기억하","수정하지","바꾸지","말고","제외",
        "must","never","always","don't","do not","instead","prefer",
        "require","without","keep","should not"
    ].iter().any(|term|lower.contains(term))
}
impl Store {
    /// Idempotent migration creates a retrieval corpus that follows native
    /// events and owner messages with SQLite triggers in the SAME transaction.
    pub(crate) fn context_init(&self)->Result<(),String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS context_sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                task_id TEXT,
                kind TEXT NOT NULL CHECK(kind IN ('owner','assistant','tool')),
                source_seq INTEGER NOT NULL,
                content TEXT NOT NULL,
                UNIQUE(kind,source_seq)
            );
            CREATE INDEX IF NOT EXISTS context_source_scope
                ON context_sources(session_id,kind,source_seq);
            CREATE VIRTUAL TABLE IF NOT EXISTS context_fts USING fts5(
                content,content='context_sources',content_rowid='id',tokenize='unicode61'
            );
            CREATE TRIGGER IF NOT EXISTS context_sources_ai
            AFTER INSERT ON context_sources BEGIN
                INSERT INTO context_fts(rowid,content)
                VALUES(new.id,new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS context_messages_ai
            AFTER INSERT ON messages
            WHEN new.role IN ('user','assistant') BEGIN
                INSERT OR IGNORE INTO context_sources(session_id,task_id,kind,source_seq,content)
                VALUES(new.session_id,NULL,
                    CASE WHEN new.role='user' THEN 'owner' ELSE 'assistant' END,
                    new.seq,new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS context_events_ai
            AFTER INSERT ON events WHEN new.kind='tool_result' BEGIN
                INSERT OR IGNORE INTO context_sources(session_id,task_id,kind,source_seq,content)
                SELECT COALESCE(NULLIF(json_extract(t.input,'$.sessionId'),''),t.id),
                    t.id,'tool',new.seq,new.data
                FROM tasks t WHERE t.id=new.task_id;
            END;
            CREATE TABLE IF NOT EXISTS context_facts(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                source_seq INTEGER NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN('constraint','decision','preference','open_issue')),
                topic TEXT NOT NULL,
                quote TEXT NOT NULL,
                superseded_by INTEGER,
                created_at TEXT NOT NULL,
                UNIQUE(session_id,source_seq,kind,quote)
            );
            CREATE INDEX IF NOT EXISTS context_fact_scope
                ON context_facts(session_id,kind,source_seq);
            CREATE TABLE IF NOT EXISTS context_reviewed(
                session_id TEXT NOT NULL,
                source_seq INTEGER NOT NULL,
                PRIMARY KEY(session_id,source_seq)
            );
            INSERT OR IGNORE INTO context_sources(session_id,task_id,kind,source_seq,content)
              SELECT session_id,NULL,
                     CASE WHEN role='user' THEN 'owner' ELSE 'assistant' END,
                     seq,content FROM messages
              WHERE role IN ('user','assistant');
            INSERT OR IGNORE INTO context_sources(session_id,task_id,kind,source_seq,content)
              SELECT COALESCE(NULLIF(json_extract(t.input,'$.sessionId'),''),t.id),
                     t.id,'tool',e.seq,e.data
                FROM events e JOIN tasks t ON t.id=e.task_id
               WHERE e.kind='tool_result';
        ").map_err(|e|format!("Native context index initialization failed: {e}"))?;
        let has_supersession={
            let mut stmt=conn.prepare("PRAGMA table_info(context_facts)")
                .map_err(|e|e.to_string())?;
            let cols=stmt.query_map([],|row|row.get::<_,String>(1))
                .map_err(|e|e.to_string())?;
            let present=cols.filter_map(Result::ok).any(|col|col=="superseded_by");
            present
        };
        if !has_supersession{
            conn.execute("ALTER TABLE context_facts ADD COLUMN superseded_by INTEGER",[])
                .map_err(|e|e.to_string())?;
        }
        Ok(())
    }

    /// Exact raw messages, each marked with an immutable SQLite sequence.
    /// Unreviewed owner directives are processed oldest-first to prevent a
    /// long history from permanently hiding early constraints.
    pub fn context_candidates(&self,session:&str,limit:usize)->Vec<Value>{
        let Ok(conn)=self.conn.lock() else{return vec![]};
        let Ok(mut query)=conn.prepare(
            "SELECT m.seq,m.content FROM messages m
             LEFT JOIN context_reviewed r ON r.session_id=m.session_id AND r.source_seq=m.seq
             WHERE m.session_id=?1 AND m.role='user' AND r.source_seq IS NULL
             ORDER BY m.seq ASC LIMIT ?2"
        ) else{return vec![]};
        let Ok(items)=query.query_map(params![session,limit.clamp(1,24) as i64],
            |row|Ok(json!({"seq":row.get::<_,i64>(0)?,
                "content":row.get::<_,String>(1)?}))) else{return vec![]};
        items.filter_map(Result::ok).collect()
    }
    /// Query-aware sampling from an arbitrarily old owner message, without
    /// restricting retrieval to an arbitrary 256-message session tail.
    pub fn context_owner_source(&self,session:&str,seq:u64)->Option<Value>{
        let record=self.history_message(session,seq)?;
        let conn=self.conn.lock().ok()?;
        let role:Option<String>=conn.query_row(
            "SELECT role FROM messages WHERE session_id=?1 AND seq=?2",
            params![session,seq.min(i64::MAX as u64) as i64],
            |row|row.get(0)).optional().ok()?;
        if role.as_deref()==Some("user"){Some(record)}else{None}
    }
    pub fn context_recent_candidates(&self,session:&str,limit:usize)->Vec<Value>{
        let Ok(conn)=self.conn.lock() else{return vec![]};
        let Ok(mut statement)=conn.prepare(
            "SELECT m.seq,m.content FROM messages m
             LEFT JOIN context_reviewed r ON r.session_id=m.session_id AND r.source_seq=m.seq
             WHERE m.session_id=?1 AND m.role='user' AND r.source_seq IS NULL
             ORDER BY m.seq DESC LIMIT ?2"
        ) else{return vec![]};
        let Ok(rows)=statement.query_map(params![session,limit.clamp(1,16) as i64],
            |row|Ok(json!({"seq":row.get::<_,i64>(0)?,
                "content":row.get::<_,String>(1)?}))) else{return vec![]};
        rows.filter_map(Result::ok).collect()
    }
    pub fn context_review(&self,session:&str,seqs:&[u64])->Result<(),String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        for seq in seqs {
            tx.execute("INSERT OR IGNORE INTO context_reviewed(session_id,source_seq)
                        VALUES(?1,?2)",
                params![session,(*seq).min(i64::MAX as u64) as i64])
                .map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e|e.to_string())
    }
    /// A model cannot create or change an owner directive: every active fact
    /// must match an EXACT substring of a genuine original user message.
    pub fn context_pin(&self,session:&str,seq:u64,kind:&str,topic:&str,quote:&str)
        ->Result<bool,String>{
        if !matches!(kind,"constraint"|"decision"|"preference"|"open_issue") ||
            topic.trim().is_empty() || topic.len()>100 ||
            quote.trim().is_empty() || quote.len()>480 || quote!=quote.trim() {
            return Err("Invalid bounded context fact".into());
        }
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let original:Option<String>=conn.query_row(
            "SELECT content FROM messages WHERE seq=?1 AND session_id=?2 AND role='user'",
            params![seq.min(i64::MAX as u64) as i64,session],
            |row|row.get(0)).optional().map_err(|e|e.to_string())?;
        if !original.is_some_and(|text|text.contains(quote)){
            return Err("Context quote is not present in the cited owner message".into());
        }
        conn.execute("INSERT OR IGNORE INTO context_facts
            (session_id,source_seq,kind,topic,quote,created_at)
            VALUES(?1,?2,?3,?4,?5,?6)",
            params![session,seq as i64,kind,topic.trim(),quote,
                chrono::Utc::now().to_rfc3339()])
            .map(|changes|changes==1).map_err(|e|e.to_string())
    }
    /// Supersession requires two real owner quotes, a newer explicit
    /// correction marker, and an identical semantic topic. Both originals
    /// remain durable; only the superseded quote leaves the active checkpoint.
    pub fn context_supersede(&self,session:&str,new_seq:u64,old_seq:u64,
        kind:&str,topic:&str,new_quote:&str)->Result<bool,String>{
        if new_seq<=old_seq || new_quote.len()>480{return Ok(false)}
        let correction=["정정","이제","대신","바꿔","변경","취소","앞으로",
            "더 이상","말고","actually","instead","no longer","from now","change to"];
        let owner=self.history_message(session,new_seq)
            .ok_or("New owner correction source missing")?;
        let text=owner["content"].as_str().unwrap_or("");
        if !text.contains(new_quote) ||
            !correction.iter().any(|word|text.to_lowercase().contains(word)){
            return Ok(false);
        }
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let exists:bool=conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM context_facts
             WHERE session_id=?1 AND source_seq=?2 AND kind=?3
             AND lower(topic)=lower(?4) AND quote=?5
             AND superseded_by IS NULL)",
            params![session,new_seq as i64,kind,topic,new_quote],
            |r|r.get(0)).map_err(|e|e.to_string())?;
        if !exists{return Ok(false)}
        conn.execute("UPDATE context_facts SET superseded_by=?3
            WHERE session_id=?1 AND source_seq=?2 AND kind=?4
              AND lower(topic)=lower(?5) AND superseded_by IS NULL",
            params![session,old_seq as i64,new_seq as i64,kind,topic])
            .map(|count|count>0).map_err(|e|e.to_string())
    }

    /// A bounded checkpoint is made only of verbatim, proven owner quotes.
    /// Constraint and correction-like material outranks optional preferences;
    /// question relevance breaks ties, then newer source message order.
    pub fn context_checkpoint(&self,session:&str,question:&str,max_bytes:usize)->Value{
        let Ok(conn)=self.conn.lock() else{return json!({"facts":[]})};
        let Ok(mut statement)=conn.prepare(
            "SELECT source_seq,kind,topic,quote FROM context_facts
             WHERE session_id=?1 AND superseded_by IS NULL
             ORDER BY source_seq DESC LIMIT 240"
        ) else{return json!({"facts":[]})};
        let Ok(rows)=statement.query_map([session],|row|Ok(json!({
            "sourceSeq":row.get::<_,i64>(0)?,"kind":row.get::<_,String>(1)?,
            "topic":row.get::<_,String>(2)?,"quote":row.get::<_,String>(3)?
        }))) else{return json!({"facts":[]})};
        let terms=words(question);
        let mut entries=rows.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by(|a,b|{
            let grade=|item:&Value|{
                let weight=match item["kind"].as_str().unwrap_or(""){
                    "constraint"=>500,"decision"=>380,
                    "open_issue"=>310,_=>210
                };
                let topic=item["topic"].as_str().unwrap_or("").to_lowercase();
                let quote=item["quote"].as_str().unwrap_or("").to_lowercase();
                let matches=terms.iter().filter(|term|topic.contains(*term)||quote.contains(*term))
                    .count() as i64;
                (weight+matches*80,item["sourceSeq"].as_i64().unwrap_or(0))
            };
            grade(b).cmp(&grade(a))
        });
        let mut retained=Vec::new();
        let mut size=128usize;
        for entry in entries{
            let bytes=entry.to_string().len();
            if size+bytes>max_bytes.clamp(512,12000){continue;}
            size+=bytes;retained.push(entry);
            if retained.len()>=40{break;}
        }
        json!({"facts":retained,"source":"verified_verbatim_owner_messages",
            "selection":"priority_relevance_recency","untrustedModelAnnotations":true})
    }

    /// Indexed search over the full persisted session, not just its newest
    /// 256 messages. Tool observations are filtered again against strict
    /// current-task / continuation-ancestor authorization after SQL access.
    pub fn context_search(&self,session:&str,task_id:&str,query:&str,limit:usize)->Vec<Value>{
        let terms=words(query);
        if terms.is_empty(){return vec![];}
        let fts=terms.iter()
            .map(|term|format!("\"{}\"",term.replace('"',"\"\"")))
            .collect::<Vec<_>>().join(" OR ");
        let candidates={
            let Ok(conn)=self.conn.lock() else{return vec![]};
            let Ok(mut stmt)=conn.prepare(
                "SELECT s.kind,s.task_id,s.source_seq,s.content
                   FROM context_fts
                   JOIN context_sources s ON s.id=context_fts.rowid
                  WHERE context_fts MATCH ?1 AND s.session_id=?2
                  ORDER BY bm25(context_fts),s.source_seq DESC LIMIT 120"
            ) else{return vec![]};
            let Ok(rows)=stmt.query_map(params![fts,session],|row|Ok((
                row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,
                row.get::<_,i64>(2)?,row.get::<_,String>(3)?
            ))) else{return vec![]};
            rows.filter_map(Result::ok).collect::<Vec<_>>()
        };
        let mut out=Vec::new();
        for (kind,source_task,seq,body) in candidates{
            if kind=="tool" && !source_task.as_deref()
                .is_some_and(|source|self.receipt_source_allowed(task_id,source)){
                continue;
            }
            let call=if kind=="tool" {
                serde_json::from_str::<Value>(&body).ok()
                    .and_then(|v|v["detail"]["callId"].as_str().map(str::to_string))
            }else{None};
            out.push(json!({
                "kind":kind,"sourceSeq":seq,"sourceTaskId":source_task,
                "messageSeq":if kind=="tool"{Value::Null}else{json!(seq)},
                "callId":call,
                "excerpt":excerpt(&body,&terms),
                "exactRetrieval":if kind=="tool"{"task_receipt"}else{"session_history"},
                "untrusted":kind!="owner"
            }));
            if out.len()>=limit.clamp(1,20){break;}
        }
        out
    }

    /// Reconstruct an auditable continuation snapshot from durable native
    /// events rather than a possibly hallucinated assistant summary. A
    /// started-but-unfinished tool remains uncertain: never auto-replay it.
    pub fn context_recovery_snapshot(&self,prior_id:&str)->Result<Value,String>{
        use std::collections::{BTreeMap,HashSet};
        let prior=self.get(prior_id).ok_or("Source task missing")?;
        let mut open=BTreeMap::<String,Value>::new();
        let mut receipts=Vec::<Value>::new();
        let mut verified=Vec::<Value>::new();
        let mut last_plan=Value::Null;
        let mut last_owner_correction=Value::Null;
        let mut scanned=0usize;
        let mut cursor=0_u64;
        let mut seen=HashSet::new();
        loop{
            let events=self.events_after(prior_id,cursor,500);
            if events.is_empty(){break;}
            let count=events.len();
            for entry in events{
                let seq=entry["seq"].as_u64().unwrap_or(cursor);
                cursor=cursor.max(seq);
                scanned+=1;
                let data=&entry["detail"];
                match entry["type"].as_str().unwrap_or(""){
                    "tool_start"=>{
                        let call=data["callId"].as_str().unwrap_or("");
                        if !call.is_empty(){
                            open.insert(call.into(),json!({"callId":call,
                                "tool":data["name"],"sourceSeq":seq,
                                "status":"effect_uncertain_until_receipt"}));
                        }
                    },
                    "tool_result"=>{
                        let call=data["callId"].as_str().unwrap_or("");
                        if !call.is_empty(){
                            open.remove(call);
                            if seen.insert(call.to_string()) {
                                receipts.push(json!({"callId":call,"tool":data["name"],
                                    "sourceSeq":seq,"ok":data["outcome"]["ok"],
                                    "readbackVerified":data["outcome"]["receipt"]["verified"],
                                    "sourceTaskId":prior_id}));
                            }
                        }
                        if data["outcome"]["ok"]==true &&
                            data["outcome"]["receipt"]["verified"]==true {
                            if let Some(changes)=data["outcome"]["receipt"]["changes"].as_array(){
                                for change in changes.iter().filter(|value|value["verified"]==true){
                                    verified.push(json!({"callId":call,
                                        "path":change["path"],
                                        "sha256":change["sha256"],"sourceSeq":seq,
                                        "batch":true}));
                                }
                            }else{
                                verified.push(json!({"callId":call,
                                    "path":data["outcome"]["receipt"]["path"],
                                    "sha256":data["outcome"]["receipt"]["sha256"],
                                    "sourceSeq":seq}));
                            }
                        }
                    },
                    "plan"=>last_plan=json!({"sourceSeq":seq,
                        "text":data["text"].as_str().unwrap_or("")
                            .chars().take(700).collect::<String>()}),
                    "instruction_applied"=>last_owner_correction=json!({
                        "sourceSeq":seq,
                        "text":data["text"].as_str().unwrap_or("")
                            .chars().take(1000).collect::<String>()
                    }),
                    _=>{}
                }
            }
            if count<500||scanned>=5000{break;}
        }
        let uncertain=open.into_values().take(24).collect::<Vec<_>>();
        let last_receipts=receipts.iter().rev().take(16).cloned()
            .collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>();
        let trusted_writes=verified.iter().rev().take(16).cloned()
            .collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>();
        Ok(json!({
            "priorTaskId":prior_id,"priorStatus":prior["status"],
            "lastCursor":cursor,"partial":scanned>=5000,
            "eventCountScanned":scanned,
            "uncertainToolCalls":uncertain,
            "recentReceiptPointers":last_receipts,
            "verifiedSourceChanges":trusted_writes,
            "latestOwnerCorrection":last_owner_correction,
            "previousProvisionalPlan":last_plan,
            "priorResult":prior["result"],
            "priorError":prior["error"],
            "effectReplayAllowed":false,
            "evidenceLookup":"context_search or task_receipt(sourceTaskId,callId)",
            "source":"read_only_verified_native_event_log"
        }))
    }

    /// Deterministic fallback: preserve explicit owner constraints even when
    /// the summarizing provider fails, never synthesizing an instruction.
    pub fn context_fallback_owner(&self,session:&str,items:&[Value])->usize{
        let mut added=0;
        for item in items{
            let Some(seq)=item["seq"].as_u64() else{continue};
            let Some(source)=item["content"].as_str() else{continue};
            if !instruction_hint(source){continue;}
            for line in source.split(['\n','.','。','!','?']).map(str::trim) {
                if !instruction_hint(line)||line.is_empty(){continue;}
                let quote=line.chars().take(145).collect::<String>();
                if self.context_pin(session,seq,"constraint","owner instruction",&quote)
                    .unwrap_or(false) {added+=1;}
                if added>=12{return added;}
            }
        }
        added
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn searchable_receipts_survive_restart_and_keep_task_boundaries(){
        let root=std::env::temp_dir().join(format!("dominion-search-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path=root.join("run.sqlite");
        let task_id={
            let db=Store::open(&path).unwrap();
            db.add_message("shared","user","초기 설계에서는 빨간색 버튼을 쓰지 마세요.");
            let (task,_)=db.submit("search-test-26",&json!({
                "prompt":"inspect","sessionId":"shared"
            })).unwrap();
            let task_id=task["id"].as_str().unwrap().to_string();
            db.append_event(&task_id,"tool_result",json!({
                "callId":"look-up-1","outcome":{"ok":true,
                    "receipt":{"content":"PomegranateSearchMarker verified run observation"}}
            })).unwrap();
            task_id
        };
        let db=Store::open(&path).unwrap();
        let user=db.context_search("shared",&task_id,"빨간색",5);
        assert!(user.iter().any(|r|r["kind"]=="owner"));
        let tool=db.context_search("shared",&task_id,"PomegranateSearchMarker",5);
        assert_eq!(tool[0]["callId"],"look-up-1");
        assert_eq!(tool[0]["sourceTaskId"],task_id);
        let (outsider,_)=db.submit("other-run-26",&json!({
            "prompt":"elsewhere","sessionId":"shared"
        })).unwrap();
        let outside=outsider["id"].as_str().unwrap();
        assert!(db.context_search("shared",outside,"PomegranateSearchMarker",5).is_empty());
        drop(db);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn multi_file_batch_receipts_are_reconstructed_as_distinct_verified_changes(){
        let db=Store::open(std::path::Path::new(":memory:")).unwrap();
        let (record,_)=db.submit("multi-file-snapshot-2026",
            &json!({"prompt":"edit source"})).unwrap();
        let id=record["id"].as_str().unwrap();
        db.append_event(id,"tool_start",json!({
            "callId":"batch-1","name":"workspace_replace_many"
        })).unwrap();
        db.append_event(id,"tool_result",json!({
            "callId":"batch-1","name":"workspace_replace_many",
            "outcome":{"ok":true,"receipt":{"verified":true,"fileCount":2,
                "changes":[{"path":"a.rs","sha256":"one","verified":true},
                    {"path":"b.rs","sha256":"two","verified":true}]}}
        })).unwrap();
        let snapshot=db.context_recovery_snapshot(id).unwrap();
        assert_eq!(snapshot["verifiedSourceChanges"].as_array().unwrap().len(),2);
        assert_eq!(snapshot["verifiedSourceChanges"][0]["path"],"a.rs");
        assert_eq!(snapshot["verifiedSourceChanges"][1]["sha256"],"two");
        assert_eq!(snapshot["uncertainToolCalls"].as_array().unwrap().len(),0);
    }

    #[test]
    fn recovery_snapshot_marks_uncertain_effects_without_claiming_success(){
        let db=Store::open(std::path::Path::new(":memory:")).unwrap();
        let (task,_)=db.submit("recovery-snapshot-2026",
            &json!({"prompt":"change files"})).unwrap();
        let id=task["id"].as_str().unwrap();
        db.append_event(id,"plan",json!({"text":"Read and verify first"})).unwrap();
        db.append_event(id,"tool_start",json!({"callId":"changed","name":"workspace_write"})).unwrap();
        db.append_event(id,"tool_result",json!({"callId":"changed",
            "name":"workspace_write","outcome":{"ok":true,"receipt":{
                "path":"a.rs","sha256":"sha-a","verified":true}}})).unwrap();
        db.append_event(id,"tool_start",json!({"callId":"unreturned",
            "name":"workspace_replace"})).unwrap();
        let snapshot=db.context_recovery_snapshot(id).unwrap();
        assert_eq!(snapshot["verifiedSourceChanges"][0]["path"],"a.rs");
        assert_eq!(snapshot["uncertainToolCalls"][0]["callId"],"unreturned");
        assert_eq!(snapshot["effectReplayAllowed"],false);
        assert!(snapshot["recentReceiptPointers"][0]["sourceSeq"].is_number());
        assert_eq!(snapshot["previousProvisionalPlan"]["text"],"Read and verify first");
    }

    #[test]
    fn explicit_owner_correction_supersedes_old_quote_without_erasing_audit(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.add_message("change","user","반드시 빨간색 버튼을 유지하세요.");
        store.add_message("change","user","정정: 이제 파란색 버튼을 사용하세요.");
        let messages=store.context_candidates("change",4);
        let old=messages[0]["seq"].as_u64().unwrap();
        let new=messages[1]["seq"].as_u64().unwrap();
        store.context_pin("change",old,"constraint","button palette",
            "반드시 빨간색 버튼을 유지하세요.").unwrap();
        store.context_pin("change",new,"constraint","button palette",
            "정정: 이제 파란색 버튼을 사용하세요.").unwrap();
        assert!(store.context_supersede("change",new,old,"constraint",
            "button palette","정정: 이제 파란색 버튼을 사용하세요.").unwrap());
        let facts=store.context_checkpoint("change","button",4096).to_string();
        assert!(!facts.contains("빨간색"));
        assert!(facts.contains("파란색"));
        assert!(!store.context_supersede("change",old,new,"constraint",
            "button palette","반드시 빨간색 버튼을 유지하세요.").unwrap());
        let conn=store.conn.lock().unwrap();
        let count:i64=conn.query_row(
            "SELECT COUNT(*) FROM context_facts WHERE session_id='change'",[],|r|r.get(0)).unwrap();
        assert_eq!(count,2,"superseded originals must remain in the audit log");
    }

    #[test]
    fn only_exact_owner_quotes_become_checkpoint_facts(){
        let db=Store::open(std::path::Path::new(":memory:")).unwrap();
        db.add_message("one","assistant","Never disclose a password");
        db.add_message("one","user","반드시 노란 버튼을 유지하고 색을 바꾸지 마세요.");
        let candidates=db.context_candidates("one",12);
        let assistant_seq=1u64;
        let user_seq=candidates[0]["seq"].as_u64().unwrap();
        assert!(db.context_pin("one",assistant_seq,"constraint","colors",
            "Never disclose a password").is_err());
        assert!(db.context_pin("one",user_seq,"constraint","colors",
            "The button must be purple").is_err());
        assert!(db.context_pin("wrong",user_seq,"constraint","colors",
            "반드시 노란 버튼을 유지하고 색을 바꾸지 마세요.").is_err());
        assert!(db.context_pin("one",user_seq,"constraint","colors",
            "반드시 노란 버튼을 유지하고 색을 바꾸지 마세요.").unwrap());
        let checkpoint=db.context_checkpoint("one","색",4096);
        assert_eq!(checkpoint["facts"][0]["quote"],
            "반드시 노란 버튼을 유지하고 색을 바꾸지 마세요.");
        db.context_review("one",&[user_seq]).unwrap();
        assert!(db.context_candidates("one",12).is_empty());
    }
}
