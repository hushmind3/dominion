use rusqlite::{params, Connection, Row};
use serde_json::{json, Value};
use std::{path::Path, sync::{Arc, Mutex}};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

fn task_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let input: String = row.get(2)?;
    let result: Option<String> = row.get(4)?;
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "requestId": row.get::<_, String>(1)?,
        "input": serde_json::from_str::<Value>(&input).unwrap_or(Value::Null),
        "status": row.get::<_, String>(3)?,
        "result": result.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
        "error": row.get::<_, Option<String>>(5)?,
        "cancelRequested": row.get::<_, i64>(6)? != 0,
        "steps": row.get::<_, i64>(7)?,
        "tokens": row.get::<_, i64>(8)?,
        "createdAt": row.get::<_, String>(9)?,
        "costKrw": row.get::<_, f64>(10)?
    }))
}

const TASK_FIELDS: &str = "id, request_id, input, status, result, error, cancelled, steps, tokens, created_at, cost_krw";

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch("
            PRAGMA journal_mode=WAL;
            PRAGMA busy_timeout=5000;
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                request_id TEXT UNIQUE NOT NULL,
                input TEXT NOT NULL,
                status TEXT NOT NULL,
                result TEXT,
                error TEXT,
                cancelled INTEGER NOT NULL DEFAULT 0,
                steer TEXT,
                steps INTEGER NOT NULL DEFAULT 0,
                tokens INTEGER NOT NULL DEFAULT 0,
                cost_krw REAL NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                data TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS event_task_seq ON events(task_id, seq);
            CREATE TABLE IF NOT EXISTS messages (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS message_session_seq ON messages(session_id, seq);
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'fact',
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS memory_scope ON memories(session_id,status);
            CREATE TABLE IF NOT EXISTS owner_profile (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                name TEXT NOT NULL,
                avatar TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                trigger TEXT NOT NULL DEFAULT '',
                instructions TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
        ").map_err(|e| e.to_string())?;
        // Preserve data from pre-metering native SQLite installations.
        let has_cost={
            let mut stmt=conn.prepare("PRAGMA table_info(tasks)").map_err(|e|e.to_string())?;
            let cols=stmt.query_map([],|row|row.get::<_,String>(1))
                .map_err(|e|e.to_string())?;
            cols.filter_map(Result::ok).any(|name|name=="cost_krw")
        };
        if !has_cost {
            conn.execute("ALTER TABLE tasks ADD COLUMN cost_krw REAL NOT NULL DEFAULT 0",[])
                .map_err(|e|e.to_string())?;
        }
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Store owner-provided evidence. A session scope never widens implicitly.
    /// Persist the one local owner's visible identity for the main React shell.
    pub fn owner_profile(&self) -> Value {
        let fallback = || json!({"id":"owner","name":"나","avatar":"","updatedAt":null});
        let Ok(conn) = self.conn.lock() else { return fallback() };
        conn.query_row(
            "SELECT name,avatar,updated_at FROM owner_profile WHERE id=1",[],
            |row| Ok(json!({
                "id":"owner",
                "name":row.get::<_,String>(0)?,
                "avatar":row.get::<_,String>(1)?,
                "updatedAt":row.get::<_,String>(2)?
            }))
        ).unwrap_or_else(|_| fallback())
    }

    pub fn set_owner_profile(&self, name: &str, avatar: &str) -> Result<Value,String> {
        let name = name.trim();
        if name.is_empty() || name.len()>100 {
            return Err("Owner name must contain 1..100 bytes".into());
        }
        let avatar=avatar.trim();
        if avatar.len()>1_500_000 {return Err("Avatar exceeds 1.5 MB".into());}
        if !avatar.is_empty() {
            let encoded=["data:image/png;base64,","data:image/jpeg;base64,",
                "data:image/webp;base64,"].iter().find_map(|prefix|avatar.strip_prefix(prefix))
                .ok_or("Avatar must be a PNG, JPEG or WebP data URI")?;
            if encoded.is_empty() || !encoded.bytes().all(|byte|
                byte.is_ascii_alphanumeric() || matches!(byte,b'+'|b'/'|b'=')) {
                return Err("Invalid avatar image encoding".into());
            }
        }
        let at=chrono::Utc::now().to_rfc3339();
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute(
            "INSERT INTO owner_profile(id,name,avatar,updated_at) VALUES(1,?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name,
                avatar=excluded.avatar,updated_at=excluded.updated_at",
            params![name,avatar,at]
        ).map_err(|e|e.to_string())?;
        Ok(json!({"id":"owner","name":name,"avatar":avatar,"updatedAt":at}))
    }

    pub fn remember(&self, session: Option<&str>, content: &str, kind: &str)
        -> Result<Value,String> {
        let content=content.trim();
        if content.is_empty() || content.len()>4096 {return Err("Memory must contain 1..4096 bytes".into());}
        if session.is_some_and(|id|id.is_empty()||id.len()>128) {
            return Err("Invalid memory session".into());
        }
        if !matches!(kind,"fact"|"correction"|"preference"|"procedure") {
            return Err("Invalid memory kind".into());
        }
        let id=Uuid::new_v4().to_string();
        let at=chrono::Utc::now().to_rfc3339();
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute("INSERT INTO memories(id,session_id,content,source,kind,status,created_at)
                      VALUES(?1,?2,?3,'owner',?4,'active',?5)",
            params![id,session,content,kind,at]).map_err(|e|e.to_string())?;
        Ok(json!({"id":id,"sessionId":session,"content":content,
            "source":"owner","kind":kind,"status":"active","createdAt":at}))
    }

    pub fn memory_list(&self, session: Option<&str>)->Vec<Value> {
        let Ok(conn)=self.conn.lock() else { return vec![] };
        let Ok(mut query)=conn.prepare("SELECT id,session_id,content,source,kind,status,created_at
               FROM memories WHERE session_id IS NULL OR session_id=?1
               ORDER BY created_at DESC LIMIT 500") else {return vec![]};
        let Ok(rows)=query.query_map(params![session],|row|{
            Ok(json!({"id":row.get::<_,String>(0)?,
                "sessionId":row.get::<_,Option<String>>(1)?,
                "content":row.get::<_,String>(2)?,
                "source":row.get::<_,String>(3)?,
                "kind":row.get::<_,String>(4)?,
                "status":row.get::<_,String>(5)?,
                "createdAt":row.get::<_,String>(6)?}))
        }) else { return vec![] };
        rows.filter_map(Result::ok).collect()
    }

    pub fn memory_disable(&self,id:&str)->Result<bool,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute("UPDATE memories SET status='inactive' WHERE id=?1 AND status='active'",
            [id]).map(|changed|changed>0).map_err(|e|e.to_string())
    }

    pub fn skill_save(&self,name:&str,trigger:&str,instructions:&str,enabled:bool)
        -> Result<Value,String> {
        let (name,trigger,instructions)=(name.trim(),trigger.trim(),instructions.trim());
        if name.is_empty() || name.len()>100 || trigger.len()>200 ||
            instructions.is_empty() || instructions.len()>12_000 {
            return Err("Skill fields exceed allowed limits".into());
        }
        let id=Uuid::new_v4().to_string();
        let at=chrono::Utc::now().to_rfc3339();
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute("INSERT INTO skills(id,name,trigger,instructions,enabled,created_at)
               VALUES(?1,?2,?3,?4,?5,?6)",
            params![id,name,trigger,instructions,i64::from(enabled),at])
            .map_err(|e|e.to_string())?;
        Ok(json!({"id":id,"name":name,"trigger":trigger,"instructions":instructions,
            "enabled":enabled,"createdAt":at}))
    }

    pub fn skill_list(&self)->Vec<Value>{
        let Ok(conn)=self.conn.lock() else {return vec![]};
        let Ok(mut query)=conn.prepare("SELECT id,name,trigger,instructions,enabled,created_at
            FROM skills ORDER BY created_at DESC LIMIT 200") else {return vec![]};
        let Ok(rows)=query.query_map([],|row|Ok(json!({
            "id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,
            "trigger":row.get::<_,String>(2)?,"instructions":row.get::<_,String>(3)?,
            "enabled":row.get::<_,i64>(4)?!=0,"createdAt":row.get::<_,String>(5)?
        }))) else {return vec![]};
        rows.filter_map(Result::ok).collect()
    }

    pub fn skill_enable(&self,id:&str,enabled:bool)->Result<bool,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute("UPDATE skills SET enabled=?2 WHERE id=?1",
            params![id,i64::from(enabled)])
            .map(|changed|changed>0).map_err(|e|e.to_string())
    }

    pub fn submit(&self, request_id: &str, input: &Value) -> Result<(Value, bool), String> {
        let payload = serde_json::to_string(input).map_err(|e| e.to_string())?;
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO tasks(id, request_id, input, status, created_at) VALUES(?1, ?2, ?3, 'queued', ?4)",
            params![id, request_id, payload, created_at]
        ).map_err(|e| e.to_string())? == 1;
        let task = conn.query_row(
            &format!("SELECT {TASK_FIELDS} FROM tasks WHERE request_id=?1"),
            [request_id], task_row
        ).map_err(|e| e.to_string())?;
        if task["input"] != *input {
            return Err("같은 요청 ID에 서로 다른 작업을 제출할 수 없습니다.".into());
        }
        Ok((task, inserted))
    }

    /// Child tasks belong to a persisted parent, never to an SDK-side process.
    pub fn child_tasks(&self,parent_id:&str)->Vec<Value>{
        let Ok(conn)=self.conn.lock() else {return vec![]};
        let Ok(mut stmt)=conn.prepare(&format!(
            "SELECT {TASK_FIELDS} FROM tasks
             WHERE json_extract(input,'$.parentId')=?1
             ORDER BY created_at LIMIT 16"
        )) else {return vec![]};
        let Ok(rows)=stmt.query_map([parent_id],task_row) else {return vec![]};
        rows.filter_map(Result::ok).collect()
    }

    /// Parent and direct children share one resource envelope.
    pub fn family_token_usage(&self,parent_id:&str)->u64{
        let Ok(conn)=self.conn.lock() else {return u64::MAX};
        let result:rusqlite::Result<i64>=conn.query_row(
            "SELECT COALESCE(SUM(tokens),0) FROM tasks
             WHERE id=?1 OR json_extract(input,'$.parentId')=?1",
            [parent_id],|row|row.get(0)
        );
        result.map(|n|n.max(0) as u64).unwrap_or(u64::MAX)
    }

    pub fn get(&self, id: &str) -> Option<Value> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(&format!("SELECT {TASK_FIELDS} FROM tasks WHERE id=?1"),
            [id], task_row).ok()
    }

    pub fn list(&self) -> Vec<Value> {
        let Ok(conn) = self.conn.lock() else { return Vec::new() };
        let Ok(mut statement) = conn.prepare(&format!(
            "SELECT {TASK_FIELDS} FROM tasks ORDER BY created_at DESC LIMIT 100"
        )) else { return Vec::new() };
        statement.query_map([], task_row).ok().into_iter().flatten()
            .filter_map(Result::ok).collect()
    }

    pub fn recover(&self) -> Result<Vec<String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        // A stopped process may have performed a write before losing its receipt.
        // Never replay an uncertain external effect without a new owner decision.
        conn.execute(
            "UPDATE tasks SET status='needs_review', error='실행 중 종료되었습니다. 기록 확인 후 새 작업으로 재시작하세요.' WHERE status='running'",
            []
        ).map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT id FROM tasks WHERE status='queued' AND cancelled=0")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Only one task may own a transcript at a time. Null session IDs are
    /// isolated by task ID; callers with the same explicit session are ordered.
    /// The SQLite update is atomic across concurrent submitters/workers.
    pub fn claim(&self, id: &str) -> bool {
        let Ok(conn) = self.conn.lock() else { return false };
        conn.execute(
            "UPDATE tasks SET status='running'
             WHERE id=?1 AND status='queued' AND cancelled=0
               AND NOT EXISTS (
                 SELECT 1 FROM tasks AS active
                 WHERE active.status='running'
                   AND COALESCE(json_extract(active.input,'$.sessionId'),active.id)
                       = COALESCE(json_extract(tasks.input,'$.sessionId'),tasks.id)
               )",
            [id]
        ).is_ok_and(|n| n == 1)
    }

    /// Runnable tasks are rediscovered after another task finishes or the
    /// process restarts. This is a read-only query; claim() owns admission.
    pub fn runnable_ids(&self, limit: usize) -> Vec<String> {
        let Ok(conn) = self.conn.lock() else { return Vec::new() };
        let Ok(mut stmt) = conn.prepare(
            "SELECT pending.id FROM tasks AS pending
             WHERE pending.status='queued' AND pending.cancelled=0
               AND NOT EXISTS (
                 SELECT 1 FROM tasks AS active
                 WHERE active.status='running'
                   AND COALESCE(json_extract(active.input,'$.sessionId'),active.id)
                       = COALESCE(json_extract(pending.input,'$.sessionId'),pending.id)
               )
             ORDER BY pending.created_at, pending.rowid LIMIT ?1"
        ) else { return Vec::new() };
        let Ok(rows) = stmt.query_map([limit.clamp(1, 64) as i64],
            |row| row.get::<_, String>(0)) else { return Vec::new() };
        rows.filter_map(Result::ok).collect()
    }

    pub fn cancel(&self, id: &str) -> Option<Value> {
        let conn = self.conn.lock().ok()?;
        conn.execute(
            "UPDATE tasks SET cancelled=1, status=CASE WHEN status='queued' THEN 'cancelled' ELSE status END WHERE id=?1 AND status IN ('queued','running')",
            [id]
        ).ok()?;
        drop(conn);
        self.get(id)
    }

    pub fn cancelled(&self, id: &str) -> bool {
        self.get(id).is_none_or(|v| v["cancelRequested"] == true ||
            v["status"] == "cancelled" || v["status"] == "needs_review")
    }

    pub fn steer(&self, id: &str, instruction: &str) -> Result<Value, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let changed = conn.execute(
            "UPDATE tasks SET steer=CASE WHEN steer IS NULL THEN ?2 ELSE steer || char(10) || ?2 END WHERE id=?1 AND status IN ('queued','running') AND cancelled=0",
            params![id, instruction]
        ).map_err(|e| e.to_string())?;
        drop(conn);
        if changed == 0 { return Err("진행 중인 작업만 수정할 수 있습니다.".into()); }
        self.get(id).ok_or("작업을 찾을 수 없습니다.".into())
    }

    pub fn take_steer(&self, id: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        let text: Option<String> = conn.query_row(
            "SELECT steer FROM tasks WHERE id=?1", [id], |row| row.get(0)
        ).ok()?;
        if text.is_some() {
            conn.execute("UPDATE tasks SET steer=NULL WHERE id=?1", [id]).ok()?;
        }
        text
    }

    /// Plan and independent critique requests consume model tokens but are
    /// not counted as tool-loop steps. Every call contributes to the budget.
    pub fn account_aux_tokens(&self, id: &str, tokens: u64) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "UPDATE tasks SET tokens=tokens+?2 WHERE id=?1",
                params![id,tokens.min(i64::MAX as u64) as i64]
            );
        }
    }

    /// Persist measured provider spend (configured KRW rates, not a fabricated
    /// invoice). Validated nonnegative costs accumulate across plan, execute,
    /// verification and delegated calls.
    pub fn account_cost(&self,id:&str,krw:f64) -> Result<(),String> {
        if !krw.is_finite() || krw<0.0 {return Err("Invalid provider cost".into());}
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute("UPDATE tasks SET cost_krw=cost_krw+?2 WHERE id=?1",
            params![id,krw]).map_err(|e|e.to_string())?;
        Ok(())
    }

    pub fn family_cost_krw(&self,id:&str)->f64 {
        let Ok(conn)=self.conn.lock() else {return f64::INFINITY};
        conn.query_row("SELECT COALESCE(SUM(cost_krw),0) FROM tasks
            WHERE id=?1 OR json_extract(input,'$.parentId')=?1",
            [id],|row|row.get::<_,f64>(0)).unwrap_or(f64::INFINITY)
    }

    pub fn measure(&self, id: &str, tokens: u64) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute("UPDATE tasks SET steps=steps+1,tokens=tokens+?2 WHERE id=?1",
                params![id, tokens.min(i64::MAX as u64) as i64]);
        }
    }

    pub fn finish(&self, id: &str, status: &str, result: Value, error: Option<String>) -> Option<Value> {
        let conn = self.conn.lock().ok()?;
        conn.execute(
            "UPDATE tasks SET status=CASE WHEN cancelled=1 THEN 'cancelled' ELSE ?2 END, result=?3, error=?4 WHERE id=?1 AND status IN ('running','queued')",
            params![id, status, result.to_string(), error]
        ).ok()?;
        drop(conn);
        self.get(id)
    }

    pub fn append_event(&self, task_id: &str, kind: &str, detail: Value) -> Result<Value, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let base = json!({"taskId":task_id,"type":kind,"detail":detail});
        conn.execute("INSERT INTO events(task_id,kind,data) VALUES(?1,?2,?3)",
            params![task_id, kind, base.to_string()]).map_err(|e| e.to_string())?;
        let seq = conn.last_insert_rowid();
        let mut event = base;
        event["seq"] = json!(seq);
        Ok(event)
    }

    pub fn events_after(&self, id: &str, after: u64, limit: usize) -> Vec<Value> {
        let Ok(conn) = self.conn.lock() else { return Vec::new() };
        let Ok(mut stmt) = conn.prepare(
            "SELECT seq,data FROM events WHERE task_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3"
        ) else { return Vec::new() };
        let Ok(rows) = stmt.query_map(
            params![id, after.min(i64::MAX as u64) as i64, limit.min(500) as i64],
            |row| Ok((row.get::<_, i64>(0)?,row.get::<_, String>(1)?))
        ) else { return Vec::new() };
        rows.filter_map(|row| {
            let (seq,raw) = row.ok()?;
            let mut value: Value = serde_json::from_str(&raw).ok()?;
            value["seq"] = json!(seq);
            Some(value)
        }).collect()
    }

    pub fn history(&self, session_id: &str, limit: usize) -> Vec<Value> {
        let Ok(conn) = self.conn.lock() else { return Vec::new() };
        let Ok(mut stmt) = conn.prepare(
            "SELECT role,content FROM messages WHERE session_id=?1 ORDER BY seq DESC LIMIT ?2"
        ) else { return Vec::new() };
        let Ok(rows) = stmt.query_map(params![session_id,limit.min(40) as i64],
            |row| Ok(json!({"role":row.get::<_,String>(0)?,"content":row.get::<_,String>(1)?}))
        ) else { return Vec::new() };
        let mut result: Vec<Value> = rows.filter_map(Result::ok).collect();
        result.reverse();
        result
    }

    pub fn add_message(&self, session_id: &str, role: &str, content: &str) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO messages(session_id,role,content) VALUES(?1,?2,?3)",
                params![session_id,role,content]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_store() -> Store {
        Store::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn retried_submit_is_one_durable_task() {
        let db = new_store();
        let input = json!({"prompt":"write file","allowWrite":true,"maxSteps":4});
        let (first, created) = db.submit("same-key-2026", &input).unwrap();
        assert!(created);
        let (again, created) = db.submit("same-key-2026", &input).unwrap();
        assert!(!created);
        assert_eq!(first["id"], again["id"]);
        assert!(db.submit("same-key-2026", &json!({"prompt":"different"})).is_err());
        assert_eq!(db.list().len(), 1);
    }

    #[test]
    fn owner_correction_is_once_and_cancellation_is_terminal() {
        let db = new_store();
        let (task, _) = db.submit("steering-2026", &json!({"prompt":"x"})).unwrap();
        let id = task["id"].as_str().unwrap();
        db.steer(id,"edit first").unwrap();
        assert_eq!(db.take_steer(id).as_deref(), Some("edit first"));
        assert!(db.take_steer(id).is_none());
        db.cancel(id).unwrap();
        assert!(!db.claim(id));
    }

    #[test]
    fn usage_cost_is_accumulated_and_invalid_cost_refused() {
        let db=new_store();
        let (task,_) = db.submit("metered-task-2026",
            &json!({"prompt":"inspect repo"})).unwrap();
        let id=task["id"].as_str().unwrap();
        db.account_cost(id,1.25).unwrap();
        db.account_cost(id,2.75).unwrap();
        assert_eq!(db.get(id).unwrap()["costKrw"],json!(4.0));
        assert_eq!(db.family_cost_krw(id),4.0);
        assert!(db.account_cost(id,f64::NAN).is_err());
        assert!(db.account_cost(id,-1.0).is_err());
        assert_eq!(db.get(id).unwrap()["costKrw"],json!(4.0));
    }

    #[test]
    fn event_cursor_does_not_repeat_prior_records() {
        let db = new_store();
        let a = db.append_event("task-1","step",json!({"n":1})).unwrap();
        db.append_event("task-2","step",json!({"n":2})).unwrap();
        let c = db.append_event("task-1","step",json!({"n":3})).unwrap();
        assert_eq!(db.events_after("task-1",a["seq"].as_u64().unwrap(),10),vec![c]);
    }
}
