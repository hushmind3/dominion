use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalizeOutcome {
    Applied,
    AlreadyApplied,
    Conflict,
}

fn upsert_tx(
    tx: &rusqlite::Transaction<'_>,
    kind: &str,
    value: &Value,
) -> Result<(), String> {
    let id = value["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("{kind} record has no id"))?;
    tx.execute(
        "INSERT INTO records(kind, id, data) VALUES(?1, ?2, ?3)
         ON CONFLICT(kind, id) DO UPDATE SET data=excluded.data, updated_at=datetime('now')",
        params![kind, id, value.to_string()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn upsert_secret(conn: &Connection, name: &str, sealed: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO secrets(name, value) VALUES(?1, ?2)
         ON CONFLICT(name) DO UPDATE SET value=excluded.value, updated_at=datetime('now')",
        params![name, sealed],
    )
}

/// Merges a computed improvement update into the record currently stored in
/// the finalization transaction. Observation counters are applied as deltas so
/// two attempts finishing concurrently cannot lose an increment.
fn merge_improvement_update(current: &Value, update: &Value) -> Value {
    let mut merged = current.clone();
    for counter in ["uses", "successes", "failures"] {
        let delta = update["__counterDelta"][counter].as_i64().unwrap_or(0);
        let base = current[counter].as_i64().unwrap_or(0);
        merged[counter] = json!(base.saturating_add(delta).max(0));
    }
    let mut applied = current["appliedExecutionIds"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for id in update["appliedExecutionIds"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if !applied.contains(id) {
            applied.push(id.clone());
        }
    }
    if applied.len() > 200 {
        applied = applied.split_off(applied.len() - 200);
    }
    merged["appliedExecutionIds"] = json!(applied);
    if update.get("lastCompliance").is_some() {
        merged["lastCompliance"] = update["lastCompliance"].clone();
    }
    if update.get("lastObservedAt").is_some() {
        merged["lastObservedAt"] = update["lastObservedAt"].clone();
    }
    let rank = |status: &str| match status {
        "rolled_back" => 2,
        "validated" => 1,
        _ => 0,
    };
    let update_status = update["status"].as_str().unwrap_or("");
    let current_status = current["status"].as_str().unwrap_or("");
    if rank(update_status) > rank(current_status) {
        for field in [
            "status",
            "evidenceGrade",
            "validatedAt",
            "rolledBackAt",
            "reason",
        ] {
            if update.get(field).is_some() {
                merged[field] = update[field].clone();
            }
        }
    }
    merged
}

/// Durable store. Records are schemaless JSON documents grouped by `kind`,
/// which is exactly the contract the React client already speaks.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

// Remote compute leases and receipts share the same Store and SQLite transaction boundary.
mod remote_tasks;

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS records(
                 kind TEXT NOT NULL,
                 id TEXT NOT NULL,
                 data TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now')),
                 updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                 PRIMARY KEY(kind, id)
             );
             CREATE TABLE IF NOT EXISTS messages(
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL DEFAULT '',
                 reasoning TEXT NOT NULL DEFAULT '',
                 agent_id TEXT,
                 tool_calls TEXT,
                 tool_call_id TEXT,
                 metadata TEXT NOT NULL DEFAULT '{}',
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);
             -- An ordered RoomEvent is committed by the same SQLite INSERT
             -- that creates its source message, including owner and agents.
             CREATE TABLE IF NOT EXISTS room_events(
                 session_id TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 event_id TEXT NOT NULL UNIQUE,
                 author TEXT NOT NULL,
                 reply_to TEXT,
                 origin_run TEXT,
                 kind TEXT NOT NULL,
                 PRIMARY KEY(session_id,seq)
             );
             CREATE TRIGGER IF NOT EXISTS room_events_message_commit
             AFTER INSERT ON messages WHEN NEW.session_id LIKE 'room:%'
             BEGIN
                 INSERT INTO room_events(session_id,seq,event_id,author,reply_to,origin_run,kind)
                 VALUES(
                     NEW.session_id,
                     (SELECT COALESCE(MAX(seq),0)+1 FROM room_events
                      WHERE session_id=NEW.session_id),
                     NEW.id,COALESCE(NULLIF(NEW.agent_id,''),'owner'),
                     json_extract(NEW.metadata,'$.replyToMessageId'),
                     COALESCE(json_extract(NEW.metadata,'$.executionId'),
                              json_extract(NEW.metadata,'$.jobId')),
                     CASE WHEN NEW.role='user' THEN 'owner_message'
                          WHEN NEW.role='assistant' THEN 'agent_message'
                          ELSE 'message' END
                 );
             END;
             CREATE TABLE IF NOT EXISTS audit(
                 id TEXT PRIMARY KEY,
                 event TEXT NOT NULL,
                 detail TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS secrets(
                 name TEXT PRIMARY KEY,
                 value TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS event_log(
                 seq INTEGER PRIMARY KEY,
                 data TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             -- The durable sequence cannot be inferred from a process-local
             -- counter, especially after retention or process restart.
             CREATE TABLE IF NOT EXISTS event_clock(
                 id INTEGER PRIMARY KEY CHECK(id=1),
                 last_seq INTEGER NOT NULL CHECK(last_seq>=0)
             );
             CREATE TABLE IF NOT EXISTS adaptation_schema(
                 version INTEGER PRIMARY KEY,
                 applied_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS adaptation_finalizations(
                 attempt_id TEXT NOT NULL,
                 evaluation_revision TEXT NOT NULL,
                 payload_hash TEXT NOT NULL,
                 applied_at TEXT NOT NULL,
                 PRIMARY KEY(attempt_id, evaluation_revision)
             );
             CREATE TABLE IF NOT EXISTS adaptation_updates(
                 observation_id TEXT NOT NULL,
                 projection_version TEXT NOT NULL,
                 PRIMARY KEY(observation_id, projection_version)
             );
             CREATE INDEX IF NOT EXISTS idx_records_kind_updated
                 ON records(kind, updated_at DESC);
             -- Only current work is scanned by workers; historical job JSON
             -- cannot grow the polling cost.
             CREATE INDEX IF NOT EXISTS idx_job_active_status
                 ON records(kind, json_extract(data,'$.status'), json_extract(data,'$.queuedAt'))
                 WHERE kind='job';
             CREATE INDEX IF NOT EXISTS idx_operation_run
                 ON records(json_extract(data,'$.runId'))
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_operation_session
                 ON records(json_extract(data,'$.sessionId'))
                 WHERE kind='execution_operation';
             -- Startup recovery touches only unresolved effects and running
             -- receipts, not an ever-growing historical operation ledger.
             CREATE INDEX IF NOT EXISTS idx_operation_recovery_unknown
                 ON records(kind,json_extract(data,'$.sessionId'),json_extract(data,'$.runId'))
                 WHERE kind='execution_operation'
                   AND json_extract(data,'$.outcomeUnknown')=1;
             CREATE INDEX IF NOT EXISTS idx_operation_recovery_running
                 ON records(kind,json_extract(data,'$.status'))
                 WHERE kind='execution_operation'
                   AND json_extract(data,'$.status')='running';
             CREATE INDEX IF NOT EXISTS idx_native_approval_pending_run
                 ON records(json_extract(data,'$.runId'),json_extract(data,'$.sessionId'))
                 WHERE kind='approval' AND json_extract(data,'$.status')='pending';
             CREATE INDEX IF NOT EXISTS idx_execution_task
                 ON records(json_extract(data,'$.taskId'))
                 WHERE kind='execution';
             -- Indexed execution timeline and live-only operator views.
             CREATE INDEX IF NOT EXISTS idx_execution_page
                 ON records(COALESCE(json_extract(data,'$.startedAt'),created_at) DESC,
                     updated_at DESC,id DESC)
                 WHERE kind='execution';
             CREATE INDEX IF NOT EXISTS idx_execution_active_status
                 ON records(json_extract(data,'$.status'),updated_at DESC,id DESC)
                 WHERE kind='execution';
             CREATE INDEX IF NOT EXISTS idx_native_job_route_completed
                 ON records(json_extract(data,'$.nativeRoute.binding'))
                 WHERE kind='job' AND json_extract(data,'$.status')='completed';
             -- Indexed executable policy lookup, not full history/stat scans.
             CREATE INDEX IF NOT EXISTS idx_algorithm_kind_lookup
                 ON records(json_extract(data,'$.kind'),id)
                 WHERE kind='algorithm';
             CREATE INDEX IF NOT EXISTS idx_remote_active_lease
                 ON records(json_extract(data,'$.leaseExpiresAtMs'))
                 WHERE kind='remote_task'
                   AND json_extract(data,'$.status')='running';",
        )?;
        // Older databases predate reasoning/tool columns.
        for (column, decl) in [
            ("reasoning", "TEXT NOT NULL DEFAULT ''"),
            ("agent_id", "TEXT"),
            ("tool_calls", "TEXT"),
            ("tool_call_id", "TEXT"),
            ("metadata", "TEXT NOT NULL DEFAULT '{}'"),
        ] {
            let _ = conn.execute(
                &format!("ALTER TABLE messages ADD COLUMN {column} {decl}"),
                [],
            );
        }
        // Backfill pre-trigger transcripts once. The new trigger covers every
        // later writer without a process-local room counter.
        conn.execute_batch(
            "INSERT OR IGNORE INTO room_events(
                 session_id,seq,event_id,author,reply_to,origin_run,kind)
             SELECT session_id,
                    ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY rowid),
                    id,COALESCE(NULLIF(agent_id,''),'owner'),
                    json_extract(metadata,'$.replyToMessageId'),
                    COALESCE(json_extract(metadata,'$.executionId'),
                             json_extract(metadata,'$.jobId')),
                    CASE WHEN role='user' THEN 'owner_message'
                         WHEN role='assistant' THEN 'agent_message'
                         ELSE 'message' END
             FROM messages WHERE session_id LIKE 'room:%';"
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO adaptation_schema(version, applied_at) VALUES(1, datetime('now'))",
            [],
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path.to_path_buf(),
        })
    }

    pub fn all(&self, kind: &str) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 ORDER BY updated_at DESC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([kind], record_from_row) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    /// Indexed look-up of one executable kernel domain.
    /// The full historical counters are retained for diagnostics, not for
    /// automatic UCB-like exploration.
    pub fn algorithms_for_kind(&self, kind: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records INDEXED BY idx_algorithm_kind_lookup
             WHERE kind='algorithm' AND json_extract(data,'$.kind')=?1 ORDER BY id"
        ).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([kind], record_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error| error.to_string())
    }

    /// Indexed SQLite aggregation of settled, provider-reported cache usage.
    /// Reads only matching route records; does not materialize every job as
    /// JSON in the Rust router. Old databases gain the partial expression
    /// index through the existing idempotent Store::open migration.
    pub fn native_route_cache_aggregate(&self,binding:&str)
        -> Result<(u64,u64,u64,u64,u64),String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let sql="SELECT COUNT(*),
              COALESCE(SUM(CAST(json_extract(data,'$.nativeModelUsage.cacheReportedCalls') AS INTEGER)),0),
              COALESCE(SUM(CAST(json_extract(data,'$.nativeModelUsage.inputTokens') AS INTEGER)),0),
              COALESCE(SUM(MAX(0,COALESCE(CAST(json_extract(data,'$.nativeModelUsage.cacheReadTokens') AS INTEGER),0))),0),
              COALESCE(SUM(MAX(0,COALESCE(CAST(json_extract(data,'$.nativeModelUsage.cacheWriteTokens') AS INTEGER),0))),0)
             FROM records
             WHERE kind='job' AND json_extract(data,'$.status')='completed'
             AND json_extract(data,'$.nativeRoute.binding')=?1
             AND json_type(data,'$.nativeModelUsage.cacheReportedCalls')='integer'
             AND CAST(json_extract(data,'$.nativeModelUsage.cacheReportedCalls') AS INTEGER)>0
             AND COALESCE(CAST(json_extract(data,'$.nativeModelUsage.unreportedCalls') AS INTEGER),0)=0
             AND json_type(data,'$.nativeModelUsage.inputTokens')='integer'
             AND CAST(json_extract(data,'$.nativeModelUsage.inputTokens') AS INTEGER)>0";
        let raw=conn.query_row(sql,[binding],|row|Ok((
            row.get::<_,i64>(0)?,row.get::<_,i64>(1)?,
            row.get::<_,i64>(2)?,row.get::<_,i64>(3)?,row.get::<_,i64>(4)?
        ))).map_err(|e|e.to_string())?;
        Ok((raw.0.max(0) as u64,raw.1.max(0) as u64,
            raw.2.max(0) as u64,raw.3.max(0) as u64,raw.4.max(0) as u64))
    }

    /// Worker-only indexed view of queued/running jobs. Preserves old job
    /// JSON and room/learning concurrency policies without materializing
    /// every completed execution on every idle wakeup.
    pub fn active_jobs(&self)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='job' AND json_extract(data,'$.status') IN ('queued','running')
             ORDER BY json_extract(data,'$.queuedAt'),id"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// One indexed startup lookup for unresolved tool effects, with a legacy
    /// run->session fallback. The prior recovery loop scanned the entire
    /// operation ledger once per running job and fetched each execution.
    pub fn unknown_effect_sessions(&self)->Result<std::collections::HashSet<String>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT DISTINCT COALESCE(NULLIF(json_extract(op.data,'$.sessionId'),''),
                    json_extract(run.data,'$.sessionId'))
             FROM records op LEFT JOIN records run
               ON run.kind='execution' AND run.id=json_extract(op.data,'$.runId')
             WHERE op.kind='execution_operation'
               AND json_extract(op.data,'$.outcomeUnknown')=1"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],|row|row.get::<_,Option<String>>(0))
            .map_err(|e|e.to_string())?;
        let mut sessions=std::collections::HashSet::new();
        for row in rows{
            if let Some(id)=row.map_err(|e|e.to_string())?{
                if !id.is_empty(){sessions.insert(id);}
            }
        }
        Ok(sessions)
    }
    /// Public SDK status checks only the current job's uncertain tool effects,
    /// not the complete historical operation ledger for each visible run.
    pub fn run_has_uncertain_operation(&self,run_id:&str)->Result<bool,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.runId')=?1
               AND json_extract(data,'$.outcomeUnknown')=1)",
            [run_id],|row|row.get::<_,bool>(0)
        ).map_err(|e|e.to_string())
    }
    /// A single pending native approval for this run/session is at most
    /// a bounded subset. Historical or other-user approvals stay untouched.
    pub fn pending_run_approvals(
        &self,run_id:&str,session_id:&str,
    )->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='approval' AND json_extract(data,'$.status')='pending'
               AND json_extract(data,'$.runId')=?1
               AND json_extract(data,'$.sessionId')=?2
             ORDER BY updated_at DESC LIMIT 128"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![run_id,session_id],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Read running receipts only; completed receipts are permanent history,
    /// never part of reboot reconciliation or its CPU/DB cost.
    pub fn running_execution_operations(&self)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.status')='running'"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    pub fn execution_operations(&self, run_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records
             WHERE kind='execution_operation' AND json_extract(data,'$.runId')=?1
             ORDER BY CAST(json_extract(data,'$.opSeq') AS INTEGER), id",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map([run_id], record_from_row).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// Materialize only the current run's journal rows instead of parsing
    /// every historical tool receipt. Recent-first preserves the former
    /// `all("execution_operation")` caller ordering.
    pub fn execution_operations_recent(&self, run_id:&str) -> Result<Vec<Value>,String> {
        self.execution_operation_subset("$.runId",run_id)
    }

    /// Effect inspection is scoped to the owning chat session, not all past
    /// executions in the product. Retain recent-first ordering for callers
    /// which explicitly reverse the list into chronological order.
    pub fn session_execution_operations(&self, session_id:&str) -> Result<Vec<Value>,String> {
        self.execution_operation_subset("$.sessionId",session_id)
    }

    fn execution_operation_subset(&self,field:&str,key:&str)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let sql=match field {
            "$.runId" => "SELECT data,id,created_at,updated_at FROM records
                  WHERE kind='execution_operation' AND json_extract(data,'$.runId')=?1
                  ORDER BY updated_at DESC",
            "$.sessionId" => "SELECT data,id,created_at,updated_at FROM records
                  WHERE kind='execution_operation' AND json_extract(data,'$.sessionId')=?1
                  ORDER BY updated_at DESC",
            _ => return Err("Unsupported execution operation filter".into()),
        };
        let mut stmt=conn.prepare(sql).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([key],record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Indexed history for one owner task. Recovery must not load every
    /// execution JSON merely to find an interrupted checkpoint. The query
    /// returns newest-first and includes only the requested task's records.
    pub fn task_executions(&self,task_id:&str)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution' AND json_extract(data,'$.taskId')=?1
             ORDER BY updated_at DESC,id"
        ).map_err(|error|error.to_string())?;
        let rows=stmt.query_map([task_id],record_from_row)
            .map_err(|error|error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())
    }

    /// Paginate run history inside SQLite. The old API materialized and
    /// parsed every historical execution before keeping 200 rows.
    pub fn execution_page(&self,limit:usize,offset:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records INDEXED BY idx_execution_page
             WHERE kind='execution'
             ORDER BY COALESCE(json_extract(data,'$.startedAt'),created_at) DESC,
                      updated_at DESC,id DESC
             LIMIT ?1 OFFSET ?2"
        ).map_err(|error|error.to_string())?;
        let limit=i64::try_from(limit).unwrap_or(i64::MAX);
        let offset=i64::try_from(offset).unwrap_or(i64::MAX);
        let rows=stmt.query_map(rusqlite::params![limit,offset],record_from_row)
            .map_err(|error|error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())
    }

    /// A live dashboard should never deserialize terminated runs just to
    /// locate its handful of active execution records.
    pub fn active_executions_indexed(&self)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution' AND json_extract(data,'$.status')='running'
             ORDER BY updated_at DESC,id DESC"
        ).map_err(|error|error.to_string())?;
        let rows=stmt.query_map([],record_from_row).map_err(|error|error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())
    }

    /// Error-preserving variant for the new learning/finalization paths.
    /// `all` remains for React-facing compatibility, but new paths must not
    /// interpret a database error as an empty collection.
    pub fn try_all(&self, kind: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn
            .prepare(
                "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 ORDER BY updated_at DESC",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([kind], record_from_row)
            .map_err(|error| error.to_string())?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|error| error.to_string())?);
        }
        Ok(records)
    }

    pub fn count(&self, kind: &str) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM records WHERE kind=?1",
            [kind],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    pub fn get(&self, kind: &str, id: &str) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
            record_from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn put(&self, kind: &str, value: &Value) -> Option<Value> {
        let object = value.as_object()?;
        let id = object
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut data = value.clone();
        data["id"] = json!(id);
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO records(kind, id, data) VALUES(?1, ?2, ?3)
                 ON CONFLICT(kind, id) DO UPDATE SET data=excluded.data, updated_at=datetime('now')",
                params![kind, id, data.to_string()],
            )
            .ok()?;
        }
        self.get(kind, &id)
    }

    /// Atomically settle a disputed outbound effect. The owner explicitly
    /// identifies the observed outcome. No external send or cursor rewind
    /// happens as a consequence of reconciling this receipt.
    pub fn reconcile_channel_outbox(
        &self,binding_id:&str,delivery_id:&str,confirmed_delivered:bool
    )->Result<Value,String>{
        let mut conn=self.conn.lock().map_err(|error|error.to_string())?;
        let tx=conn.transaction().map_err(|error|error.to_string())?;
        let record:Option<Value>=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='channel_outbox' AND id=?1",
            params![delivery_id],record_from_row,
        ).optional().map_err(|error|error.to_string())?;
        let mut row=record.ok_or("전송 검토 기록을 찾을 수 없습니다.")?;
        if row["bindingId"].as_str()!=Some(binding_id){
            return Err("다른 채널의 전송 기록은 처리할 수 없습니다.".into());
        }
        if row["status"]!="needs_review"{
            return Err("이미 처리되었거나 검토 대상이 아닌 전송 기록입니다.".into());
        }
        row["status"]=json!(if confirmed_delivered{"sent"}else{"cancelled"});
        row["ownerResolution"]=json!(if confirmed_delivered{"confirmed_delivered"}else{"confirmed_not_delivered"});
        row["resolvedAt"]=json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos,true));
        let changed=tx.execute(
            "UPDATE records SET data=?1,updated_at=datetime('now') WHERE kind='channel_outbox' AND id=?2 AND json_extract(data,'$.bindingId')=?3 AND json_extract(data,'$.status')='needs_review'",
            params![row.to_string(),delivery_id,binding_id],
        ).map_err(|error|error.to_string())?;
        if changed!=1{return Err("전송 상태가 변경되었습니다. 새로고침 후 다시 확인하세요.".into());}
        tx.commit().map_err(|error|error.to_string())?;
        Ok(row)
    }

    pub fn save_channel(&self, value: &Value, secret: Option<(&str, &str)>) -> Option<Value> {
        let mut conn = self.conn.lock().ok()?;
        let tx = conn.transaction().ok()?;
        upsert_tx(&tx, "channel", value).ok()?;
        if let Some((name, sealed)) = secret {
            upsert_secret(&tx, name, sealed).ok()?;
        }
        let saved = tx.query_row(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind='channel' AND id=?1",
            params![value["id"].as_str()?],
            record_from_row,
        ).ok()?;
        tx.commit().ok()?;
        Some(saved)
    }

    /// Writes several records in one SQLite transaction. Any failure rolls
    /// back all of them, which is required for experiment promotion.
    pub fn transaction(&self, records: &[(String, Value)]) -> Result<Vec<Value>, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let mut saved = Vec::new();
        for (kind, value) in records {
            upsert_tx(&tx, kind, value)?;
            saved.push(value.clone());
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(saved)
    }

    /// Transaction that only commits when the current record still has the
    /// expected status value. Returns false on a CAS miss.
    pub fn transaction_if_status(
        &self,
        kind: &str,
        id: &str,
        expected: &str,
        records: &[(String, Value)],
    ) -> Result<bool, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let current: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let status = current
            .and_then(|data| serde_json::from_str::<Value>(&data).ok())
            .and_then(|value| value["status"].as_str().map(str::to_string));
        if status.as_deref() != Some(expected) {
            return Ok(false);
        }
        for (record_kind, value) in records {
            upsert_tx(&tx, record_kind, value)?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(true)
    }

    /// Atomically compare multiple live JSON fields before committing a
    /// multi-record transition. Used for program resume, whose source
    /// approval and destination execution must both still be current.
    /// No dynamic SQL identifiers are interpolated: fields are looked up
    /// only after reading stored JSON, and an unreadable row fails closed.
    pub fn transaction_if_fields(
        &self,
        conditions: &[(&str, &str, &str, Value)],
        records: &[(String, Value)],
    ) -> Result<bool, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        for (kind, id, field, expected) in conditions {
            let existing: Option<String> = tx.query_row(
                "SELECT data FROM records WHERE kind=?1 AND id=?2",
                params![kind, id], |row| row.get(0),
            ).optional().map_err(|e| e.to_string())?;
            let Some(actual) = existing
                .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            else {
                return Ok(false);
            };
            if actual.get(*field) != Some(expected) {
                return Ok(false);
            }
        }
        for (kind, value) in records {
            upsert_tx(&tx, kind, value)?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// Patch several existing records in one immediate SQLite transaction,
    /// comparing the relevant owner/attempt and operation fields first.
    /// Unlike transaction_if_fields, patches merge with the CURRENT record
    /// after locking, so unrelated concurrent checkpoint fields are retained.
    /// A conflict or failed update rolls back ALL patches.
    pub fn patch_records_if_fields(
        &self,
        conditions: &[(&str, &str, &str, Value)],
        patches: &[(&str, &str, Value)],
    ) -> Result<Option<Vec<Value>>, String> {
        if conditions.is_empty() || patches.is_empty() {
            return Err("Multi-record patch needs conditions and patches".into());
        }
        if patches.iter().any(|(_, _, patch)| patch.as_object()
            .is_none_or(|fields| fields.is_empty() || fields.contains_key("id"))) {
            return Err("Multi-record patches must be nonempty objects without id".into());
        }
        let mut conn=self.conn.lock().map_err(|error|error.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error|error.to_string())?;
        for (kind,id,field,expected) in conditions {
            let actual=tx.query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind,id],record_from_row,
            ).optional().map_err(|error|error.to_string())?;
            let Some(current)=actual else{return Ok(None);};
            if current.get(*field).unwrap_or(&Value::Null)!=expected {
                return Ok(None);
            }
        }
        let mut saved=Vec::with_capacity(patches.len());
        for (kind,id,patch) in patches {
            let current=tx.query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind,id],record_from_row,
            ).optional().map_err(|error|error.to_string())?;
            let Some(mut row)=current else{return Ok(None);};
            for (key,value) in patch.as_object().expect("validated object") {
                row[key]=value.clone();
            }
            let changed=tx.execute(
                "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
                params![kind,id,row.to_string()]
            ).map_err(|error|error.to_string())?;
            if changed!=1{return Ok(None);}
            saved.push(row);
        }
        tx.commit().map_err(|error|error.to_string())?;
        Ok(Some(saved))
    }

    /// Durable idempotency key. Unlike `put`, this never overwrites a record.
    pub fn put_if_absent(&self, kind: &str, value: &Value) -> Option<Value> {
        let id = value.get("id")?.as_str().filter(|id| !id.is_empty())?;
        let conn = self.conn.lock().unwrap();
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO records(kind, id, data) VALUES(?1, ?2, ?3)",
                params![kind, id, value.to_string()],
            )
            .ok()?;
        if changed != 1 {
            return None;
        }
        conn.query_row(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
            record_from_row,
        )
        .ok()
    }

    /// Atomically creates the root episode when absent, inserts the execution
    /// and appends the attempt link. A crash cannot leave an execution
    /// detached from its task, and retrying the same attempt is idempotent.
    pub fn create_attempt(
        &self,
        root: &Value,
        execution: &Value,
        attempt: &Value,
    ) -> Option<Value> {
        let root_id = root["id"].as_str()?.to_string();
        let execution_id = execution["id"].as_str()?.to_string();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().ok()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind='task_episode' AND id=?1",
                [&root_id],
                |row| row.get(0),
            )
            .optional()
            .ok()?;
        let mut episode = match existing {
            Some(text) => serde_json::from_str(&text).ok()?,
            None => {
                upsert_tx(&tx, "task_episode", root).ok()?;
                root.clone()
            }
        };
        upsert_tx(&tx, "execution", execution).ok()?;
        let attempts = episode["attempts"].as_array_mut()?;
        if !attempts
            .iter()
            .any(|item| item["executionId"].as_str() == Some(execution_id.as_str()))
        {
            attempts.push(attempt.clone());
        }
        upsert_tx(&tx, "task_episode", &episode).ok()?;
        tx.commit().ok()?;
        drop(conn);
        self.get("execution", &execution_id)
    }

    /// Compare-and-patch inside an IMMEDIATE SQLite transaction. Competing
    /// processes cannot simultaneously read an old status and both claim it.
    /// The same operation is used for cancellation, job claims and recovery.
    pub fn patch_if(
        &self,
        kind: &str,
        id: &str,
        field: &str,
        expected: &Value,
        patch: &Value,
    ) -> Option<Value> {
        self.patch_if_all(kind,id,&[(field,expected)],patch)
    }

    /// Multi-field compare-and-patch under one IMMEDIATE transaction.
    /// Lifecycle reducers must compare both durable status and phase;
    /// a concurrent cancellation or different worker's phase transition
    /// must not be reverted by a stale full-record write.
    pub fn patch_if_all(
        &self,kind:&str,id:&str,expected:&[(&str,&Value)],patch:&Value
    )->Option<Value>{
        let changes=patch.as_object()?;
        if changes.contains_key("id") || expected.is_empty() ||
            expected.iter().any(|(field,_)|field.is_empty()) {
            return None;
        }
        let mut conn=self.conn.lock().ok()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        let mut record=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind,id],record_from_row,
        ).ok()?;
        if expected.iter().any(|(field,value)|
            record.get(*field).unwrap_or(&Value::Null)!=*value){
            return None;
        }
        for (key,value) in changes {record[key]=value.clone();}
        tx.execute(
            "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
            params![kind,id,record.to_string()]
        ).ok()?;
        tx.commit().ok()?;
        Some(record)
    }

    /// Crash recovery must compare more than status. A new worker may have
    /// re-claimed the same job between the initial snapshot and this write,
    /// moving queued->running again. Attempts and owner cancellation are
    /// checked under the same IMMEDIATE transaction as the status transition.
    pub fn recover_running_job(
        &self,id:&str,expected_attempt:u64,expected_cancelled:bool,patch:&Value,
    )->Option<Value>{
        let changes=patch.as_object()?;
        if changes.contains_key("id") || changes.contains_key("attempts")
            || changes.contains_key("cancelRequested"){return None;}
        let mut conn=self.conn.lock().ok()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        let mut record=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='job' AND id=?1",[id],record_from_row).ok()?;
        if record["status"]!="running"
            || record["attempts"].as_u64().unwrap_or(0)!=expected_attempt
            ||(record["cancelRequested"]==true)!=expected_cancelled {
            return None;
        }
        for (key,value) in changes{record[key]=value.clone();}
        tx.execute(
            "UPDATE records SET data=?2,updated_at=datetime('now')
             WHERE kind='job' AND id=?1",params![id,record.to_string()]
        ).ok()?;
        tx.commit().ok()?;
        Some(record)
    }

    /// Single-owner job checkpoint. Validate running status, cancellation and
    /// worker attempt under the SAME SQLite mutex as the record update.
    /// A late tool/model reply from an earlier attempt cannot overwrite a
    /// requeued, completed or owner-cancelled job. This is the shared host
    /// contract used by every domain plugin and the native Rust SDK.
    pub fn checkpoint_running_job(
        &self, id: &str, expected_attempt: u64, patch: &Value,
    ) -> Option<Value> {
        let changes=patch.as_object()?;
        if changes.contains_key("id") || changes.contains_key("cancelRequested") {
            return None;
        }
        let mut conn=self.conn.lock().ok()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        let mut record=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='job' AND id=?1",
            [id],record_from_row,
        ).ok()?;
        if record["status"]!="running"
            || record["cancelRequested"]==true
            || record["attempts"].as_u64().unwrap_or(0)!=expected_attempt {
            return None;
        }
        for (key,value) in changes {
            record[key]=value.clone();
        }
        tx.execute(
            "UPDATE records SET data=?2,updated_at=datetime('now') WHERE kind='job' AND id=?1",
            params![id,record.to_string()],
        ).ok()?;
        tx.commit().ok()?;
        Some(record)
    }

    pub fn patch_records(&self, kind: &str, ids: &[String], patch: &Value) -> Option<Vec<Value>> {
        let patch = patch.as_object()?;
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn.transaction().ok()?;
        let mut records = Vec::new();
        for id in ids {
            let mut record = transaction
                .query_row(
                    "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                    params![kind, id],
                    record_from_row,
                )
                .ok()?;
            for (key, value) in patch {
                if key != "id" {
                    record[key] = value.clone();
                }
            }
            transaction
                .execute(
                    "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
                    params![kind, id, record.to_string()],
                )
                .ok()?;
            records.push(record);
        }
        transaction.commit().ok()?;
        Some(records)
    }

    /// Increment evidence counters under the same lock as their read. Parallel
    /// workers must not lose updates through separate get/put operations.
    pub fn increment_counters(&self, kind: &str, id: &str, fields: &[&str]) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let mut record = conn
            .query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                record_from_row,
            )
            .optional()
            .ok()?
            .unwrap_or_else(|| json!({"id":id}));
        for field in fields {
            record[*field] = json!(record[*field].as_u64().unwrap_or(0).saturating_add(1));
        }
        record["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        conn.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data,updated_at=datetime('now')",params![kind,id,record.to_string()]).ok()?;
        conn.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
            record_from_row,
        )
        .ok()
    }

    /// Atomically writes the terminal execution, improvement updates,
    /// observations and finalization marker. Duplicate identical payloads are
    /// AlreadyApplied; a different payload for the same attempt is a Conflict.
    pub fn finalize_once(
        &self,
        execution: &Value,
        marker: &Value,
        improvements: &[Value],
        observations: &[Value],
    ) -> Result<FinalizeOutcome, String> {
        let execution_id = execution["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("finalization execution id is missing")?;
        let marker_id = marker["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("finalization marker id is missing")?;
        let payload_hash = marker["payloadHash"]
            .as_str()
            .ok_or("finalization payload hash is missing")?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind='execution_finalization' AND id=?1",
                [marker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(existing) = existing {
            let value: Value = serde_json::from_str(&existing)
                .map_err(|error| format!("broken finalization marker: {error}"))?;
            if value["payloadHash"].as_str() != Some(payload_hash) {
                return Ok(FinalizeOutcome::Conflict);
            }
            if value["statsApplied"].as_bool() == Some(true) {
                return Ok(FinalizeOutcome::AlreadyApplied);
            }
        }
        // The owner may cancel while a worker is computing a final report.
        // Check the durable status UNDER this same transaction/SQLite mutex,
        // before inserting observations, counters or terminal markers. A
        // stale finalizer must never resurrect a cancelled/interrupted run.
        let stored_status: Option<String> = tx.query_row(
            "SELECT json_extract(data,'$.status') FROM records
             WHERE kind='execution' AND id=?1",
            [execution_id], |row|row.get(0)
        ).optional().map_err(|error|error.to_string())?;
        if stored_status.as_deref().is_some_and(|current|
            matches!(current,"cancelled"|"interrupted")
                && execution["status"].as_str()!=Some(current)
        ) {
            // Narrow exception: the SAME durable run was owner-cancelled
            // during an already dispatched, uncertain paid model/effect.
            // Only transition to manual review when BOTH the committed
            // checkpoint and the finalizer carry a live write-ahead fence.
            // Never allow completed/succeeded to override owner cancellation.
            let pending_in_finalizer=execution["kernelState"]["modelInFlight"]==true
                ||execution["kernelState"]["inFlight"].as_array()
                    .is_some_and(|actions|!actions.is_empty());
            let persisted_pending:i64=tx.query_row(
                "SELECT CASE WHEN
                   COALESCE(json_extract(data,'$.kernelState.modelInFlight'),0)=1
                   OR COALESCE(json_array_length(json_extract(data,'$.kernelState.inFlight')),0)>0
                 THEN 1 ELSE 0 END
                 FROM records WHERE kind='execution' AND id=?1",
                [execution_id],|row|row.get(0)
            ).map_err(|error|error.to_string())?;
            if execution["status"]!="needs_review" ||
                !pending_in_finalizer || persisted_pending!=1 ||
                execution["ownerCancellationRequested"]!=true {
                return Ok(FinalizeOutcome::Conflict);
            }
        }
        upsert_tx(&tx, "execution", execution)?;
        for improvement in improvements {
            let id = improvement["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("improvement id is missing")?;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT data FROM records WHERE kind='improvement' AND id=?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let mut record = match existing {
                Some(text) => {
                    let current: Value = serde_json::from_str(&text)
                        .map_err(|error| format!("broken improvement record: {error}"))?;
                    merge_improvement_update(&current, improvement)
                }
                None => improvement.clone(),
            };
            record["id"] = json!(id);
            if let Some(object) = record.as_object_mut() {
                object.remove("__counterDelta");
            }
            upsert_tx(&tx, "improvement", &record)?;
        }
        for observation in observations {
            upsert_tx(&tx, "strategy_observation", observation)?;
        }
        let mut applied_marker = marker.clone();
        applied_marker["statsApplied"] = json!(true);
        applied_marker["appliedAt"] = json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx, "execution_finalization", &applied_marker)?;
        tx.execute(
            "INSERT OR IGNORE INTO adaptation_finalizations(attempt_id, evaluation_revision, payload_hash, applied_at)
             VALUES(?1, ?2, ?3, datetime('now'))",
            params![execution_id, "evaluator:finalize-semantic-v1", payload_hash],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(FinalizeOutcome::Applied)
    }

    pub fn remove(&self, kind: &str, id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
        )
        .map(|changed| changed > 0)
        .unwrap_or(false)
    }

    /// Persist a native SDK answer only while the SAME job attempt still
    /// owns execution. The status read and INSERT happen under one SQLite
    /// mutex, so a concurrent owner cancellation cannot race the answer
    /// into conversation history after marking the job cancelled.
    pub fn write_native_answer_if_running(
        &self,job_id:&str,expected_attempt:u64,message:&Value,
    )->Result<Value,String>{
        let id=message["id"].as_str()
            .filter(|s|!s.is_empty()).ok_or("native answer id missing")?;
        if message["metadata"]["jobId"].as_str()!=Some(job_id)
            || message["role"]!="assistant"{
            return Err("native answer does not belong to this job".into());
        }
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='job' AND id=?1",
            [job_id],|r|r.get(0),
        ).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else{return Err("native answer job missing".into());};
        let current:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        if current["status"]!="running" || current["cancelRequested"]==true
            || current["attempts"].as_u64().unwrap_or(0)!=expected_attempt{
            return Err("native answer rejected: cancelled, finished or stale attempt".into());
        }
        if let Some(existing)=query_message(&tx,id){
            return Ok(existing);
        }
        insert_message(&tx,message).ok_or("could not write native answer")?;
        let stored=query_message(&tx,id).ok_or("saved native answer not found")?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(stored)
    }

    /// Atomically acknowledge the transcript records delivered to ONE room
    /// agent while committing that agent's final answer. A crashed turn may
    /// leave an unresolved execution, never an advanced delivery cursor.
    pub fn write_room_answer_with_cursor(
        &self,message:&Value,cursor:&Value
    )->Result<Value,String>{
        let session=cursor["sessionId"].as_str().filter(|v|!v.is_empty())
            .ok_or("Room cursor session missing")?;
        let agent=cursor["agentId"].as_str().filter(|v|!v.is_empty())
            .ok_or("Room cursor agent missing")?;
        let source=cursor["sourceMessageId"].as_str().filter(|v|!v.is_empty())
            .ok_or("Room cursor source missing")?;
        let through=cursor["throughMessageId"].as_str().filter(|v|!v.is_empty())
            .ok_or("Room cursor delivery anchor missing")?;
        if message["session_id"].as_str()!=Some(session)
            || message["agent_id"].as_str()!=Some(agent)
            || message["role"]!="assistant"
            || message["metadata"]["authorKind"]!="agent"
            || message["metadata"]["replyToMessageId"].as_str()!=Some(source) {
            return Err("Room reply and cursor binding mismatch".into());
        }
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        let source_valid:Option<i64>=tx.query_row(
            "SELECT rowid FROM messages WHERE session_id=?1 AND id=?2
             AND role='user' AND json_extract(metadata,'$.authorKind')='owner'",
            params![session,source],|row|row.get(0),
        ).optional().map_err(|e|e.to_string())?;
        if source_valid.is_none() {return Err("Room cursor owner source absent".into());}
        let current_index:Option<i64>=tx.query_row(
            "SELECT rowid FROM messages WHERE session_id=?1 AND id=?2",
            params![session,through],|row|row.get(0),
        ).optional().map_err(|e|e.to_string())?;
        let index=current_index.ok_or("Room cursor points to missing committed message")?;
        let record_id=cursor["id"].as_str().filter(|v|!v.is_empty())
            .ok_or("Room cursor record ID missing")?;
        let previous:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='room_read_cursor' AND id=?1",
            [record_id],|row|row.get(0),
        ).optional().map_err(|e|e.to_string())?;
        if let Some(previous)=previous {
            let old:Value=serde_json::from_str(&previous).map_err(|e|e.to_string())?;
            if old["sessionId"].as_str()!=Some(session)||old["agentId"].as_str()!=Some(agent){
                return Err("Room cursor record belongs to another binding".into());
            }
            let prior=old["throughMessageId"].as_str().ok_or("Previous cursor lacks anchor")?;
            let prior_index:Option<i64>=tx.query_row(
                "SELECT rowid FROM messages WHERE session_id=?1 AND id=?2",
                params![session,prior],|row|row.get(0),
            ).optional().map_err(|e|e.to_string())?;
            if prior_index.ok_or("Previous room cursor anchor was deleted")?>index {
                return Err("Room cursor cannot rewind after another committed turn".into());
            }
        }
        let inserted=insert_message(&tx,message)
            .ok_or("Room answer could not be persisted")?;
        let delivered_seq:i64=tx.query_row(
            "SELECT seq FROM room_events WHERE session_id=?1 AND event_id=?2",
            params![session,through],|r|r.get(0)
        ).map_err(|e|format!("Room read anchor has no event sequence: {e}"))?;
        let mut persisted_cursor=cursor.clone();
        persisted_cursor["throughSeq"]=json!(delivered_seq);
        upsert_tx(&tx,"room_read_cursor",&persisted_cursor)?;
        let saved=query_message(&tx,&inserted).ok_or("Persisted room answer unavailable")?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(saved)
    }

    pub fn write_message(&self, message: &Value) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let id = insert_message(&conn, message)?;
        query_message(&conn, &id)
    }

    pub fn message(&self, id: &str) -> Option<Value> {
        query_message(&self.conn.lock().unwrap(), id)
    }

    /// A human room message and its dispatch job are one durable operation.
    /// Insert-only IDs make retries observable and never overwrite prior work.
    pub fn write_message_with_record(
        &self,
        message: &Value,
        kind: &str,
        record: &Value,
    ) -> Option<(Value, Value)> {
        let record_id = record.get("id")?.as_str().filter(|id| !id.is_empty())?;
        record.as_object()?;
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn.transaction().ok()?;
        let message_id = insert_message(&transaction, message)?;
        transaction
            .execute(
                "INSERT INTO records(kind,id,data) VALUES(?1,?2,?3)",
                params![kind, record_id, record.to_string()],
            )
            .ok()?;
        let saved_message = query_message(&transaction, &message_id)?;
        let saved_record = transaction
            .query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind, record_id],
                record_from_row,
            )
            .ok()?;
        transaction.commit().ok()?;
        Some((saved_message, saved_record))
    }

    pub fn messages(&self, session_id: &str) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            if session_id.starts_with("room:"){
                "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY rowid ASC"
            }else{
                "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY created_at ASC,rowid ASC"
            },
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([session_id], message_from_row) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    /// Bounded per-room sequence lookup: reconstructable after restart and
    /// independent of timestamp ties, provider tokens and SSE delivery.
    pub fn room_events(&self,session_id:&str,after:u64,limit:usize)
        ->Result<Vec<Value>,String>{
        let Some(room_id)=session_id.strip_prefix("room:").filter(|s|!s.is_empty())
            else{return Err("Not a Room session".into());};
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT seq,event_id,author,reply_to,origin_run,kind
             FROM room_events WHERE session_id=?1 AND seq>?2
             ORDER BY seq ASC LIMIT ?3"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![
            session_id,after.min(i64::MAX as u64) as i64,limit.clamp(1,501) as i64
        ],|r|{
            let seq:i64=r.get(0)?;
            Ok(json!({"version":1,"roomId":room_id,"sessionId":session_id,
                "seq":seq as u64,"eventId":r.get::<_,String>(1)?,
                "author":r.get::<_,String>(2)?,
                "replyTo":r.get::<_,Option<String>>(3)?,
                "originRun":r.get::<_,Option<String>>(4)?,
                "kind":r.get::<_,String>(5)?}))
        }).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Resolve a pre-sequence legacy Room cursor from the historical message
    /// identity. Read-only: no old cursor or source transcript is rewritten.
    pub fn room_event_seq(&self,session_id:&str,event_id:&str)->Option<u64>{
        let conn=self.conn.lock().ok()?;
        let value:Option<i64>=conn.query_row(
            "SELECT seq FROM room_events WHERE session_id=?1 AND event_id=?2",
            params![session_id,event_id],|row|row.get(0)
        ).optional().ok()?;
        value.and_then(|seq|u64::try_from(seq).ok())
    }

    /// Error-preserving variant for model-context assembly. A database failure
    /// must abort the turn, not look like a session with no history.
    pub fn try_messages(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn
            .prepare(
                if session_id.starts_with("room:"){
                "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY rowid ASC"
            }else{
                "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY created_at ASC,rowid ASC"
            },
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([session_id], message_from_row)
            .map_err(|error| error.to_string())?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row.map_err(|error| error.to_string())?);
        }
        Ok(messages)
    }

    /// Increment a durable cancellation epoch under the same SQLite lock.
    pub fn cancel_session_epoch(&self, session_id: &str) -> Option<u64> {
        let conn = self.conn.lock().unwrap();
        let prior = conn
            .query_row(
                "SELECT data FROM records WHERE kind='session_cancel' AND id=?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()?;
        let prior = prior
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .unwrap_or(Value::Null);
        let generation = prior["generation"].as_u64().unwrap_or(0).checked_add(1)?;
        let data = json!({"id":session_id,"generation":generation,"cancelled":true,"cancelledAt":chrono::Utc::now().to_rfc3339()});
        conn.execute("INSERT INTO records(kind,id,data) VALUES('session_cancel',?1,?2) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data,updated_at=datetime('now')",params![session_id,data.to_string()]).ok()?;
        Some(generation)
    }

    pub fn resume_session_epoch(&self, session_id: &str, expected: u64) -> bool {
        let conn = self.conn.lock().unwrap();
        let prior = match conn
            .query_row(
                "SELECT data FROM records WHERE kind='session_cancel' AND id=?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        {
            Ok(prior) => prior,
            Err(_) => return false,
        };
        let mut data = prior
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .unwrap_or_else(|| json!({"id":session_id,"generation":0}));
        if data["generation"].as_u64().unwrap_or(0) != expected {
            return false;
        }
        data["cancelled"] = json!(false);
        conn.execute("INSERT INTO records(kind,id,data) VALUES('session_cancel',?1,?2) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data,updated_at=datetime('now')",params![session_id,data.to_string()]).is_ok()
    }

    pub fn clear_messages(&self, session_id: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM messages WHERE session_id=?1", [session_id]);
    }

    /// Durable, ordered event log backing `/api/events`. Bounded by trimming
    /// old rows in the background of normal writes so a slow or disconnected
    /// client cannot lose ordering entirely or grow the database forever.
    /// Cross-restart run lease. The owner instance must keep renewing it; a
    /// crashed process leaves a lease that startup recovery releases.
    pub fn acquire_lease(&self, id: &str, owner: &str, ttl_ms: i64) -> Option<Value> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind='run_lease' AND id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .ok()?;
        if let Some(text) = existing {
            let value: Value = serde_json::from_str(&text).ok()?;
            let active = value["status"].as_str() == Some("active");
            let same_owner = value["owner"].as_str() == Some(owner);
            let expires = value["expiresAtMs"].as_i64().unwrap_or(0);
            if active && !same_owner && expires > now {
                return None;
            }
        }
        let lease = json!({
            "id": id,
            "owner": owner,
            "status": "active",
            "acquiredAtMs": now,
            "heartbeatAtMs": now,
            "expiresAtMs": now.saturating_add(ttl_ms),
        });
        upsert_tx(&tx, "run_lease", &lease).ok()?;
        tx.commit().ok()?;
        Some(lease)
    }

    /// A heartbeat cannot resurrect an expired lease, or a lease that was
    /// replaced by another owner. Compare owner + active status + expiry in one
    /// SQL update. A stale runner's heartbeat must not refresh the new owner.
    /// Cross-process workspace read/write admission for Task Team workers.
    /// All readers can coexist; a writer excludes every reader and writer of
    /// the same canonical workspace. BEGIN IMMEDIATE serializes competing
    /// admissions on independent SQLite connections.
    pub fn acquire_workspace_access(
        &self,id:&str,owner:&str,ttl_ms:i64,scope:&str,mode:&str,
    )->Option<Value>{
        if id.is_empty()||owner.is_empty()||ttl_ms<=0
            ||scope.is_empty()||scope.len()>4096
            ||!matches!(mode,"read"|"write"){return None;}
        let now=chrono::Utc::now().timestamp_millis();
        let mut conn=self.conn.lock().ok()?;
        let tx=conn.transaction_with_behavior(
            rusqlite::TransactionBehavior::Immediate).ok()?;
        let exists:Option<i64>=tx.query_row(
            "SELECT 1 FROM records WHERE kind='run_lease' AND id=?1",
            [id],|row|row.get(0)).optional().ok()?;
        if exists.is_some(){return None;}
        let blockers:i64=tx.query_row(
            "SELECT COUNT(*) FROM records
             WHERE kind='run_lease'
               AND json_extract(data,'$.workspaceScope')=?1
               AND json_extract(data,'$.status')='active'
               AND json_extract(data,'$.expiresAtMs')>?3
               AND (json_extract(data,'$.accessMode')='write' OR ?2='write')",
            params![scope,mode,now],|row|row.get(0)).ok()?;
        if blockers!=0{return None;}
        let lease=json!({
            "id":id,"owner":owner,"status":"active",
            "workspaceScope":scope,"accessMode":mode,
            "acquiredAtMs":now,"heartbeatAtMs":now,
            "expiresAtMs":now.saturating_add(ttl_ms),
        });
        upsert_tx(&tx,"run_lease",&lease).ok()?;
        tx.commit().ok()?;
        Some(lease)
    }

    pub fn renew_lease(&self, id: &str, owner: &str, ttl_ms: i64) -> Option<Value> {
        if ttl_ms <= 0 { return None; }
        let now = chrono::Utc::now().timestamp_millis();
        let conn = self.conn.lock().ok()?;
        let changed = conn.execute(
            "UPDATE records
             SET data=json_set(data,'$.heartbeatAtMs',?3,'$.expiresAtMs',?4),
                 updated_at=datetime('now')
             WHERE kind='run_lease' AND id=?1
               AND json_extract(data,'$.owner')=?2
               AND json_extract(data,'$.status')='active'
               AND json_extract(data,'$.expiresAtMs') > ?3",
            params![id, owner, now, now.saturating_add(ttl_ms)],
        ).ok()?;
        if changed != 1 { return None; }
        let data: String = conn.query_row(
            "SELECT data FROM records WHERE kind='run_lease' AND id=?1",
            [id], |row| row.get(0),
        ).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Delete only a lease still owned by THIS precise turn. The previous
    /// two-step patch-then-remove could delete another owner's lease if a
    /// contender acquired it between those operations.
    pub fn release_lease(&self, id: &str, owner: &str) -> bool {
        let Ok(conn) = self.conn.lock() else { return false; };
        conn.execute(
            "DELETE FROM records
             WHERE kind='run_lease' AND id=?1
               AND json_extract(data,'$.owner')=?2
               AND json_extract(data,'$.status')='active'",
            params![id, owner],
        ).is_ok_and(|removed| removed == 1)
    }

    /// Startup recovery must NEVER delete another process's live lease.
    /// Use the expiry predicate within the deletion itself, otherwise a
    /// heartbeat between a read and the delete can revoke a live owner.
    /// The owner argument is retained for existing callers.
    pub fn release_stale_leases(&self, _current_owner: &str) -> usize {
        let now = chrono::Utc::now().timestamp_millis();
        let Ok(conn) = self.conn.lock() else { return 0; };
        conn.execute(
            "DELETE FROM records WHERE kind='run_lease'
               AND (json_extract(data,'$.status')!='active'
                    OR json_extract(data,'$.expiresAtMs') IS NULL
                    OR json_extract(data,'$.expiresAtMs') <= ?1)",
            params![now],
        ).unwrap_or(0)
    }

    #[allow(dead_code)] // used by lease tests and operational inspection
    pub fn active_lease(&self, id: &str) -> Option<Value> {
        self.get("run_lease", id)
            .filter(|lease| lease["status"].as_str() == Some("active"))
    }

    /// Replaces the live database with a consistent snapshot. Callers must put
    /// the engine into maintenance mode first; the connection is swapped under
    /// the same mutex so no query observes a half-written file.
    pub fn replace_database(&self, bytes: &[u8]) -> Result<(), String> {
        let temp = self.path.with_extension("restore.tmp");
        std::fs::write(&temp, bytes).map_err(|error| error.to_string())?;
        let mut guard = self.conn.lock().map_err(|error| error.to_string())?;
        let _ = guard.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        // Close the old handle before copying over the database file.
        *guard = Connection::open_in_memory().map_err(|error| error.to_string())?;
        let wal = PathBuf::from(format!("{}-wal", self.path.display()));
        let shm = PathBuf::from(format!("{}-shm", self.path.display()));
        let _ = std::fs::remove_file(wal);
        let _ = std::fs::remove_file(shm);
        std::fs::copy(&temp, &self.path).map_err(|error| error.to_string())?;
        let _ = std::fs::remove_file(&temp);
        *guard = Connection::open(&self.path).map_err(|error| error.to_string())?;
        let _ = guard.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        );
        Ok(())
    }

    /// Consistent online snapshot for a restore drill. `VACUUM INTO` includes
    /// committed WAL content and never exposes a half-written database.
    pub fn backup_to(&self, path: &Path) -> Result<u64, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let _ = std::fs::remove_file(path);
        let target = path.to_string_lossy().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM INTO ?1", params![target])
            .map_err(|error| format!("backup failed: {error}"))?;
        drop(conn);
        std::fs::metadata(path)
            .map(|meta| meta.len())
            .map_err(|error| error.to_string())
    }

    /// Allocate one durable global sequence and append its event in the SAME
    /// SQLite IMMEDIATE transaction. The clock is seeded from existing logs
    /// once for upgrades; it continues monotonically even after pruning.
    /// No process may replace an earlier event with a reused sequence number.
    pub fn append_event_next(&self, mut value: Value) -> Result<Value, String> {
        let mut conn = self.conn.lock().map_err(|error|error.to_string())?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error|error.to_string())?;
        tx.execute(
            "INSERT OR IGNORE INTO event_clock(id,last_seq)
             SELECT 1,COALESCE(MAX(seq),0) FROM event_log",
            [],
        ).map_err(|error|error.to_string())?;
        let last:i64=tx.query_row(
            "SELECT last_seq FROM event_clock WHERE id=1",[],|row|row.get(0),
        ).map_err(|error|error.to_string())?;
        let seq=last.checked_add(1).ok_or("Durable event sequence exhausted")?;
        value["seq"]=json!(seq as u64);
        if value["type"]=="record"&&value["runRef"].is_object(){
            let run_ref:dominion_agent_sdk::RunRef=
                serde_json::from_value(value["runRef"].clone()).map_err(|e|e.to_string())?;
            let phase:dominion_agent_sdk::RunPhase=
                serde_json::from_value(value["runState"]["phase"].clone())
                    .map_err(|e|e.to_string())?;
            let kind=value["kind"].as_str().ok_or("Missing run journal kind")?;
            let run_event=dominion_agent_sdk::RunEvent::new(
                seq as u64,run_ref,phase,"record",kind)?;
            value["runEvent"]=serde_json::to_value(run_event)
                .map_err(|e|e.to_string())?;
        }
        tx.execute("UPDATE event_clock SET last_seq=?1 WHERE id=1",params![seq])
            .map_err(|error|error.to_string())?;
        tx.execute(
            "INSERT INTO event_log(seq,data) VALUES(?1,?2)",
            params![seq,value.to_string()],
        ).map_err(|error|error.to_string())?;
        if seq % 256 == 0 {
            tx.execute("DELETE FROM event_log WHERE seq <= ?1",
                params![seq.saturating_sub(2_000)])
                .map_err(|error|error.to_string())?;
        }
        tx.commit().map_err(|error|error.to_string())?;
        Ok(value)
    }

    pub fn events_after(&self, after: u64, limit: usize) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT seq, data FROM event_log WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(
            params![after as i64, limit.min(1_000) as i64],
            |row| {
                let seq: i64 = row.get(0)?;
                let data: String = row.get(1)?;
                Ok((seq, data))
            },
        );
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|row| {
            let (seq, data) = row.ok()?;
            let mut value: Value = serde_json::from_str(&data).ok()?;
            value["seq"] = json!(seq as u64);
            Some(value)
        })
        .collect()
    }

    pub fn last_event_seq(&self) -> u64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM event_log", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| value.max(0) as u64)
        .unwrap_or(0)
    }

    pub fn audit(&self, event: &str, detail: Value) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO audit(id, event, detail) VALUES(?1, ?2, ?3)",
            params![uuid::Uuid::new_v4().to_string(), event, detail.to_string()],
        );
    }

    /// Aggregates recorded model usage by provider/model/purpose. Streaming
    /// calls may only know a total; `estimatedCalls` keeps that distinction.
    pub fn usage_summary(&self) -> Value {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT detail FROM audit WHERE event='model.usage' ORDER BY rowid DESC LIMIT 5000",
        ) else {
            return json!({"calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0,"groups":[]});
        };
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        let Ok(rows) = rows else {
            return json!({"calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0,"groups":[]});
        };
        let mut groups: std::collections::HashMap<String, Value> =
            std::collections::HashMap::new();
        let mut calls = 0u64;
        let mut prompt = 0u64;
        let mut completion = 0u64;
        let mut total = 0u64;
        let mut estimated = 0u64;
        for row in rows.flatten() {
            let Ok(detail) = serde_json::from_str::<Value>(&row) else {
                continue;
            };
            let provider = detail["providerId"].as_str().unwrap_or("");
            let model = detail["model"].as_str().unwrap_or("");
            let purpose = detail["purpose"].as_str().unwrap_or("");
            let key = format!("{provider}\u{1f}{model}\u{1f}{purpose}");
            let entry = groups.entry(key).or_insert_with(|| {
                json!({
                    "providerId": provider,
                    "model": model,
                    "purpose": purpose,
                    "calls": 0,
                    "promptTokens": 0,
                    "completionTokens": 0,
                    "totalTokens": 0,
                    "estimatedCalls": 0,
                })
            });
            calls += 1;
            let prompt_tokens = detail["promptTokens"].as_u64().unwrap_or(0);
            let completion_tokens = detail["completionTokens"].as_u64().unwrap_or(0);
            let total_tokens = detail["totalTokens"].as_u64().unwrap_or(0);
            prompt += prompt_tokens;
            completion += completion_tokens;
            total += total_tokens;
            entry["calls"] = json!(entry["calls"].as_u64().unwrap_or(0) + 1);
            entry["promptTokens"] =
                json!(entry["promptTokens"].as_u64().unwrap_or(0) + prompt_tokens);
            entry["completionTokens"] =
                json!(entry["completionTokens"].as_u64().unwrap_or(0) + completion_tokens);
            entry["totalTokens"] =
                json!(entry["totalTokens"].as_u64().unwrap_or(0) + total_tokens);
            if detail["estimated"].as_bool() == Some(true) {
                estimated += 1;
                entry["estimatedCalls"] =
                    json!(entry["estimatedCalls"].as_u64().unwrap_or(0) + 1);
            }
        }
        let mut groups = groups.into_values().collect::<Vec<_>>();
        groups.sort_by(|a, b| {
            b["totalTokens"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["totalTokens"].as_u64().unwrap_or(0))
                .then_with(|| a["providerId"].as_str().cmp(&b["providerId"].as_str()))
        });
        json!({
            "calls": calls,
            "promptTokens": prompt,
            "completionTokens": completion,
            "totalTokens": total,
            "estimatedCalls": estimated,
            "groups": groups,
        })
    }

    /// Token totals for one session, including calls made before usage was
    /// attached to messages. Audit is the durable source of truth.
    pub fn session_model_routes(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare("SELECT DISTINCT json_extract(detail,'$.providerId'), json_extract(detail,'$.model') FROM audit WHERE event='model.usage' AND json_extract(detail,'$.sessionId')=?1").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([session_id],|row|Ok(json!({"providerId":row.get::<_,String>(0)?,"model":row.get::<_,String>(1)?}))).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    pub fn session_usage(&self, session_id: &str) -> Value {
        let conn = self.conn.lock().unwrap();
        let summary = conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CAST(json_extract(detail,'$.promptTokens') AS INTEGER)),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.completionTokens') AS INTEGER)),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.totalTokens') AS INTEGER)),0),
                    COALESCE(SUM(CASE WHEN json_extract(detail,'$.estimated')=1 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer'
                        THEN CAST(json_extract(detail,'$.cachedPromptTokens') AS INTEGER) ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer'
                        THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer'
                        THEN CAST(json_extract(detail,'$.promptTokens') AS INTEGER)
                             - CAST(json_extract(detail,'$.cachedPromptTokens') AS INTEGER)
                        ELSE 0 END),0)
             FROM audit
             WHERE event='model.usage' AND json_extract(detail,'$.sessionId')=?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        );
        match summary {
            Ok((calls, prompt, completion, total, estimated, cached, reported, new_known)) => json!({
                "calls": calls.max(0),
                "promptTokens": prompt.max(0),
                "completionTokens": completion.max(0),
                "totalTokens": total.max(0),
                "estimatedCalls": estimated.max(0),
                "cachedPromptTokensKnown": cached.max(0),
                "newPromptTokensKnown": new_known.max(0),
                "cacheReportedCalls": reported.max(0),
                "cacheUnknownCalls": (calls-reported).max(0),
            }),
            Err(_) => json!({
                "calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,
                "estimatedCalls":0,"cachedPromptTokensKnown":0,"newPromptTokensKnown":0,
                "cacheReportedCalls":0,"cacheUnknownCalls":0,
            }),
        }
    }

    pub fn recent_audit(&self, limit: usize) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn
            .prepare("SELECT id, event, detail, created_at FROM audit ORDER BY rowid DESC LIMIT ?1")
        {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([limit.min(500) as i64], |row| {
            let detail: String = row.get(2)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "event": row.get::<_, String>(1)?,
                "detail": serde_json::from_str::<Value>(&detail).unwrap_or(Value::Null),
                "createdAt": row.get::<_, String>(3)?,
            }))
        }) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    pub fn set_secret(&self, name: &str, sealed: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = upsert_secret(&conn, name, sealed);
    }

    pub fn get_secret(&self, name: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM secrets WHERE name=?1", [name], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .ok()
        .flatten()
    }

    pub fn all_secrets(&self) -> Vec<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT name, value FROM secrets ORDER BY name") else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        });
        rows.map(|rows| rows.filter_map(|row| row.ok()).collect())
            .unwrap_or_default()
    }

    pub fn has_secret(&self, name: &str) -> bool {
        self.get_secret(name).is_some()
    }

    pub fn remove_secret(&self, name: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM secrets WHERE name=?1", [name]);
    }
}

fn insert_message(conn: &Connection, message: &Value) -> Option<String> {
    let id = message
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let session_id = message.get("session_id")?.as_str()?;
    let role = message.get("role")?.as_str()?;
    let calls = message
        .get("tool_calls")
        .filter(|value| !value.is_null())
        .map(Value::to_string);
    let metadata = message
        .get("metadata")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    conn.execute("INSERT INTO messages(id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,metadata) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![
        id,session_id,role,message["content"].as_str().unwrap_or(""),message["reasoning"].as_str().unwrap_or(""),message["agent_id"].as_str(),calls,message["tool_call_id"].as_str(),metadata.to_string()
    ]).ok()?;
    Some(id)
}

fn query_message(conn: &Connection, id: &str) -> Option<Value> {
    conn.query_row("SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE id=?1",[id],message_from_row).ok()
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let calls: Option<String> = row.get(6)?;
    let metadata: String = row.get(9)?;
    Ok(
        json!({"id":row.get::<_,String>(0)?,"session_id":row.get::<_,String>(1)?,"role":row.get::<_,String>(2)?,"content":row.get::<_,String>(3)?,"reasoning":row.get::<_,String>(4)?,"agent_id":row.get::<_,Option<String>>(5)?,"tool_calls":calls.and_then(|value|serde_json::from_str::<Value>(&value).ok()),"tool_call_id":row.get::<_,Option<String>>(7)?,"created_at":row.get::<_,String>(8)?,"metadata":serde_json::from_str::<Value>(&metadata).unwrap_or_else(|_|json!({}))}),
    )
}

fn record_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let data: String = row.get(0)?;
    let id: String = row.get(1)?;
    let created_at: String = row.get(2)?;
    let updated_at: String = row.get(3)?;
    let mut value: Value = serde_json::from_str(&data).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut map) = value {
        map.insert("id".into(), json!(id));
        map.insert("createdAt".into(), json!(created_at));
        map.insert("updatedAt".into(), json!(updated_at));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "sqlite/tests.rs"]
mod tests;
