//! Explicit, transactional, read-only import of historical main-branch data.
//! Preserves the original JSON documents as inert archives; only selected
//! owner-authored records become active in the new native tables.
//! Does not import credentials, API tokens or executable jobs.
use rusqlite::{params, Connection, OpenFlags, types::ValueRef};
use base64::Engine as _;
use serde_json::{json, Value};
use std::path::Path;

fn text_field<'a>(data:&'a Value,key:&str)->&'a str{
    data[key].as_str().unwrap_or("").trim()
}
fn created(data:&Value)->&str{
    data["createdAt"].as_str().unwrap_or("1970-01-01T00:00:00Z")
}
fn permitted_kind(value:&str)->&str {
    match value{
        "fact"|"correction"|"preference"|"procedure"=>value,
        _=>"fact"
    }
}
fn session_scope(data:&Value)->Option<&str>{
    let scope=text_field(data,"scope");
    if scope=="session" || scope=="private" {
        data["sessionId"].as_str().filter(|id|!id.is_empty()&&id.len()<=128)
    }else{None}
}
fn copy_record(
    tx:&rusqlite::Transaction<'_>,kind:&str,id:&str,raw:&str
)->Result<&'static str,String>{
    let data:Value=match serde_json::from_str(raw){
        Ok(doc)=>doc,
        Err(_)=>return Ok("archived"),
    };
    match kind {
        "agent"=>{
            let name=text_field(&data,"name");
            let instructions=data["systemPrompt"].as_str()
                .or_else(||data["instructions"].as_str()).unwrap_or("").trim();
            let description=text_field(&data,"description");
            if name.is_empty()||name.len()>80||description.len()>500||
                instructions.is_empty()||instructions.len()>8000{return Ok("archived");}
            tx.execute("INSERT OR IGNORE INTO agents(id,name,description,instructions,created_at)
                        VALUES(?1,?2,?3,?4,?5)",
                params![id,name,description,instructions,created(&data)])
                .map_err(|e|e.to_string())?;
            Ok("agent")
        }
        "memory"=>{
            let content=text_field(&data,"content");
            let source=text_field(&data,"source");
            let scope=text_field(&data,"scope");
            // User statements alone carry owner authority. Ambiguous scopes
            // are archived but cannot silently widen to the global context.
            if source!="owner" || content.is_empty() || content.len()>4096 ||
                (matches!(scope,"session"|"private")&&session_scope(&data).is_none())
                || (!scope.is_empty()&&!matches!(scope,"shared"|"global"|"session"|"private")){
                return Ok("archived");
            }
            let status=if data["status"]=="active" {"active"}else{"inactive"};
            tx.execute("INSERT OR IGNORE INTO memories(id,session_id,content,source,kind,status,created_at)
                       VALUES(?1,?2,?3,'owner',?4,?5,?6)",
                params![id,session_scope(&data),content,
                    permitted_kind(data["type"].as_str().or_else(||data["kind"].as_str()).unwrap_or("fact")),
                    status,created(&data)]
            ).map_err(|e|e.to_string())?;
            Ok("memory")
        }
        "skill"=>{
            let name=text_field(&data,"name");
            let instructions=text_field(&data,"instructions");
            if name.is_empty()||name.len()>100||instructions.is_empty()||instructions.len()>12000{
                return Ok("archived");
            }
            let trigger=data["trigger"].as_str()
                .or_else(||data["triggers"].as_str()).unwrap_or("");
            if trigger.len()>200{return Ok("archived");}
            // Historical activation flags do not provide new-run consent.
            tx.execute("INSERT OR IGNORE INTO skills(id,name,trigger,instructions,enabled,created_at)
                       VALUES(?1,?2,?3,?4,0,?5)",
                params![id,name,trigger,instructions,created(&data)]
            ).map_err(|e|e.to_string())?;
            Ok("skill")
        }
        _=>Ok("archived"),
    }
}
/// Migrates history only when the operator passes an explicit source path.
/// Entire legacy JSON records remain recoverable in non-executing archive,
/// including unknown kinds, while only safe data is projected to runtime.
pub(crate) fn import(
    source:&Path,
    destination:&mut Connection
)->Result<Value,String>{
    if !source.is_file(){return Err("Legacy database is not a regular file".into());}
    let origin=Connection::open_with_flags(source,OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e|format!("Cannot read old database: {e}"))?;
    let valid:bool=origin.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='records')",
        [],|row|row.get(0)).map_err(|e|e.to_string())?;
    if !valid {return Err("Expected main-branch records table is absent".into());}
    let tx=destination.transaction().map_err(|e|e.to_string())?;
    tx.execute_batch("
        CREATE TABLE IF NOT EXISTS legacy_archive(
            kind TEXT NOT NULL,id TEXT NOT NULL,data TEXT NOT NULL,
            PRIMARY KEY(kind,id)
        );
        CREATE TABLE IF NOT EXISTS legacy_messages(
            id TEXT PRIMARY KEY,session_id TEXT NOT NULL,data TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS legacy_imports(
            source TEXT PRIMARY KEY,imported_at TEXT NOT NULL,
            archived INTEGER NOT NULL,archived_messages INTEGER NOT NULL,
            messages INTEGER NOT NULL,
            agents INTEGER NOT NULL,memories INTEGER NOT NULL,skills INTEGER NOT NULL
        );
    ").map_err(|e|e.to_string())?;
    let source_id=source.canonicalize().map_err(|e|e.to_string())?
        .to_string_lossy().to_string();
    let exists:bool=tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE source=?1)",
        [&source_id],|row|row.get(0)).map_err(|e|e.to_string())?;
    if exists {
        return Ok(json!({"alreadyImported":true,"source":source_id}));
    }
    let mut archived=0u64;
    let mut agents=0u64;
    let mut memories=0u64;
    let mut skills=0u64;
    {
        let mut stmt=origin.prepare("SELECT kind,id,data FROM records")
            .map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],|row|{
            Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?))
        }).map_err(|e|e.to_string())?;
        for item in rows {
            let (kind,id,raw)=item.map_err(|e|e.to_string())?;
            tx.execute("INSERT OR IGNORE INTO legacy_archive(kind,id,data)
                       VALUES(?1,?2,?3)",params![kind,id,raw])
                .map_err(|e|e.to_string())?;
            archived+=1;
            if id.is_empty()||id.len()>128{continue;}
            match copy_record(&tx,&kind,&id,&raw)?{
                "agent"=>agents+=1,
                "memory"=>memories+=1,
                "skill"=>skills+=1,
                _=>{},
            }
        }
    }
    let has_messages:bool=origin.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='messages')",
        [],|row|row.get(0)).map_err(|e|e.to_string())?;
    let mut messages=0u64;
    let mut archived_messages=0u64;
    if has_messages {
        let mut stmt=origin.prepare("SELECT * FROM messages ORDER BY created_at,id")
            .map_err(|e|e.to_string())?;
        let columns=stmt.column_names().iter().map(|name|name.to_string())
            .collect::<Vec<_>>();
        let rows=stmt.query_map([],|row|{
            let mut record=serde_json::Map::new();
            for (i,key) in columns.iter().enumerate(){
                let field=match row.get_ref(i)?{
                    ValueRef::Null=>Value::Null,
                    ValueRef::Integer(value)=>json!(value),
                    ValueRef::Real(value)=>json!(value),
                    ValueRef::Text(value)=>json!(String::from_utf8_lossy(value)),
                    ValueRef::Blob(value)=>json!({"binaryBase64":
                        base64::engine::general_purpose::STANDARD.encode(value)}),
                };
                record.insert(key.clone(),field);
            }
            Ok(Value::Object(record))
        }).map_err(|e|e.to_string())?;
        for item in rows {
            let document=item.map_err(|e|e.to_string())?;
            let session=document["session_id"].as_str().unwrap_or("");
            let id=document["id"].as_str().map(str::to_string)
                .unwrap_or_else(||Uuid::new_v4().to_string());
            tx.execute("INSERT OR IGNORE INTO legacy_messages(id,session_id,data)
                VALUES(?1,?2,?3)",params![id,session,document.to_string()])
                .map_err(|e|e.to_string())?;
            archived_messages+=1;
            let role=document["role"].as_str().unwrap_or("");
            let content=document["content"].as_str().unwrap_or("");
            // Do not promote historical tool or system messages into a new
            // owner-authorized model conversation, but keep them archived.
            if !(role=="user"||role=="assistant")||session.is_empty()||
                session.len()>128||content.is_empty(){continue;}
            tx.execute("INSERT INTO messages(session_id,role,content) VALUES(?1,?2,?3)",
                params![session,role,content]).map_err(|e|e.to_string())?;
            messages+=1;
        }
    }
    let at=chrono::Utc::now().to_rfc3339();
    tx.execute("INSERT INTO legacy_imports(source,imported_at,archived,archived_messages,messages,agents,memories,skills)
               VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![source_id,at,archived,archived_messages,messages,agents,memories,skills])
        .map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(json!({"alreadyImported":false,"source":source_id,"archived":archived,
        "messages":messages,"archivedMessages":archived_messages,
        "agents":agents,"memories":memories,"skills":skills,
        "keysImported":0,"jobsStarted":0}))
}

#[cfg(test)]
mod tests{
    use super::*;
    use uuid::Uuid;
    #[test]
    fn old_records_are_archived_but_only_owner_data_becomes_active(){
        let root=std::env::temp_dir().join(format!("dominion-main-import-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source=root.join("old.sqlite");
        let old=Connection::open(&source).unwrap();
        old.execute_batch("CREATE TABLE records(kind TEXT,id TEXT,data TEXT);
            CREATE TABLE messages(id TEXT,session_id TEXT,role TEXT,content TEXT,created_at TEXT);").unwrap();
        old.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3)",params![
            "agent","agent-one",
            json!({"name":"Builder","description":"Code reviewer","systemPrompt":"Inspect files","createdAt":"2026-01-01"}).to_string()
        ]).unwrap();
        old.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3)",params![
            "skill","skill-one",
            json!({"name":"Review","instructions":"Check tests","enabled":true}).to_string()
        ]).unwrap();
        old.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3)",params![
            "memory","mem-one",
            json!({"content":"항상 검증","source":"owner","scope":"global","status":"active"}).to_string()
        ]).unwrap();
        old.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3)",params![
            "provider","provider-one",json!({"apiKey":"do-not-use"}).to_string()
        ]).unwrap();
        old.execute("INSERT INTO messages(id,session_id,role,content,created_at) VALUES('m1','s1','user','hello','2026-01-01')",[]).unwrap();
        let target=crate::store::Store::open(&root.join("new.sqlite")).unwrap();
        let first=target.import_legacy(&source,&root.join("new.sqlite")).unwrap();
        assert_eq!(first["archived"],4);
        assert_eq!(first["messages"],1);
        assert_eq!(first["archivedMessages"],1);
        assert_eq!(target.agents().len(),1);
        assert_eq!(target.skill_list()[0]["enabled"],false);
        assert_eq!(target.memory_list(None)[0]["content"],"항상 검증");
        assert_eq!(target.history("s1",10).len(),1);
        assert_eq!(target.import_legacy(&source,&root.join("new.sqlite")).unwrap()["alreadyImported"],true);
        assert_eq!(target.history("s1",10).len(),1);
        drop(old);
        drop(target);
        std::fs::remove_dir_all(root).unwrap();
    }
}
