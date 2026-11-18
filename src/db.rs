use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod money;
mod money_implementation;
mod money_review;

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
                 WHERE kind='algorithm';",
        )?;
        // Additive Money indexes and FTS, without replacing sub3's schema.
        conn.execute_batch(include_str!("db/money_schema.sql"))?;
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
        let changes=patch.as_object()?;
        let mut conn=self.conn.lock().ok()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        let mut record=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind,id],record_from_row,
        ).ok()?;
        if record.get(field).unwrap_or(&Value::Null)!=expected{return None;}
        for (key,value) in changes {
            if key!="id"{record[key]=value.clone();}
        }
        tx.execute(
            "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
            params![kind,id,record.to_string()],
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
            return Ok(FinalizeOutcome::Conflict);
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
            "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY created_at ASC,rowid ASC",
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

    /// Error-preserving variant for model-context assembly. A database failure
    /// must abort the turn, not look like a session with no history.
    pub fn try_messages(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn
            .prepare(
                "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY created_at ASC,rowid ASC",
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
        let tx = conn.transaction().ok()?;
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

    pub fn renew_lease(&self, id: &str, owner: &str, ttl_ms: i64) -> Option<Value> {
        let now = chrono::Utc::now().timestamp_millis();
        self.patch_if(
            "run_lease",
            id,
            "owner",
            &json!(owner),
            &json!({
                "status": "active",
                "heartbeatAtMs": now,
                "expiresAtMs": now.saturating_add(ttl_ms),
            }),
        )
    }

    pub fn release_lease(&self, id: &str, owner: &str) -> bool {
        let patched = self
            .patch_if(
                "run_lease",
                id,
                "owner",
                &json!(owner),
                &json!({"status": "released"}),
            )
            .is_some();
        if patched {
            self.remove("run_lease", id);
        }
        patched
    }

    /// Startup recovery. This process owns the database (duplicate launches are
    /// rejected before migrations), so any other owner or expired lease is dead.
    pub fn release_stale_leases(&self, current_owner: &str) -> usize {
        let now = chrono::Utc::now().timestamp_millis();
        let mut released = 0;
        for lease in self.all("run_lease") {
            let Some(id) = lease["id"].as_str() else {
                continue;
            };
            let owner = lease["owner"].as_str().unwrap_or("");
            let expires = lease["expiresAtMs"].as_i64().unwrap_or(0);
            if owner != current_owner || expires <= now {
                if self.remove("run_lease", id) {
                    released += 1;
                }
            }
        }
        released
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

    pub fn append_event(&self, seq: u64, value: &Value) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR REPLACE INTO event_log(seq, data) VALUES(?1, ?2)",
            params![seq as i64, value.to_string()],
        );
        if seq % 256 == 0 {
            let cutoff = seq.saturating_sub(2_000) as i64;
            if cutoff > 0 {
                let _ = conn.execute(
                    "DELETE FROM event_log WHERE seq <= ?1",
                    params![cutoff],
                );
            }
        }
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
                    COALESCE(SUM(CASE WHEN json_extract(detail,'$.estimated')=1 THEN 1 ELSE 0 END),0)
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
                ))
            },
        );
        match summary {
            Ok((calls, prompt, completion, total, estimated)) => json!({
                "calls": calls.max(0),
                "promptTokens": prompt.max(0),
                "completionTokens": completion.max(0),
                "totalTokens": total.max(0),
                "estimatedCalls": estimated.max(0),
            }),
            Err(_) => json!({
                "calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0
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
mod tests {
    use super::*;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-db-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("dominion.sqlite")).unwrap();
        (store, dir)
    }

    #[test]
    fn native_route_usage_is_indexed_precise_and_excludes_invalid_jobs() {
        let (store,dir)=temp_store();
        for i in 0..150 {
            let route=if i%3==0 {"selected"}else{"unrelated"};
            store.put("job",&json!({"id":format!("job-{i}"),
                "status":"completed","nativeRoute":{"binding":route},
                "nativeModelUsage":{"cacheReportedCalls":1,"unreportedCalls":0,
                    "inputTokens":10000,"cacheReadTokens":8000,"cacheWriteTokens":0}})).unwrap();
        }
        store.put("job",&json!({"id":"unfinished","status":"running",
            "nativeRoute":{"binding":"selected"},
            "nativeModelUsage":{"cacheReportedCalls":1,"inputTokens":10000,
                "cacheReadTokens":8000}})).unwrap();
        store.put("job",&json!({"id":"unreported","status":"completed",
            "nativeRoute":{"binding":"selected"},
            "nativeModelUsage":{"cacheReportedCalls":1,"unreportedCalls":1,
                "inputTokens":10000,"cacheReadTokens":8000}})).unwrap();
        store.put("job",&json!({"id":"malformed","status":"completed",
            "nativeRoute":{"binding":"selected"},
            "nativeModelUsage":{"cacheReportedCalls":"1","unreportedCalls":0,
                "inputTokens":"10000"}})).unwrap();
        assert_eq!(store.native_route_cache_aggregate("selected").unwrap(),
            (50,50,500000,400000,0));
        assert_eq!(store.native_route_cache_aggregate("absent").unwrap(),
            (0,0,0,0,0));
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM records WHERE kind='job'
             AND json_extract(data,'$.status')='completed'
             AND json_extract(data,'$.nativeRoute.binding')=?1",
            ["selected"],|r|r.get(3)).unwrap();
        assert!(plan.contains("idx_native_job_route_completed"),"{plan}");
        drop(conn);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn indexed_execution_timeline_pages_without_loading_unrelated_json() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..120 {
            let status=if i%11==0 {"running"} else {"completed"};
            let started_at=format!("2026-11-17T{:02}:{:02}:00Z",i/60,i%60);
            store.put("execution",&json!({"id":format!("run-{i:03}"),
                "startedAt":started_at,"status":status,
                "evidence":format!("fixture-{i}")})).unwrap();
        }
        let page=store.execution_page(7,5).unwrap();
        assert_eq!(page.len(),7);
        assert_eq!(page[0]["id"],"run-114");
        assert_eq!(page[6]["id"],"run-108");
        assert!(store.execution_page(10,200).unwrap().is_empty());
        let active=store.active_executions_indexed().unwrap();
        assert_eq!(active.len(),11);
        assert!(active.iter().all(|record|record["status"]=="running"));
        let conn=store.conn.lock().unwrap();
        let page_plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records INDEXED BY idx_execution_page
             WHERE kind='execution'
             ORDER BY COALESCE(json_extract(data,'$.startedAt'),created_at) DESC,
                      updated_at DESC,id DESC LIMIT 7 OFFSET 5",[],|r|r.get(3)).unwrap();
        assert!(page_plan.contains("idx_execution_page"),"{page_plan}");
        let active_plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records
             WHERE kind='execution' AND json_extract(data,'$.status')='running'
             ORDER BY updated_at DESC,id DESC",[],|r|r.get(3)).unwrap();
        assert!(active_plan.contains("idx_execution_active_status"),"{active_plan}");
    }

    #[test]
    fn indexed_task_recovery_reads_only_the_owners_execution_history() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..200 {
            let task=if i==23 || i==107 {"requested-task"}else{"other-task"};
            store.put("execution",&json!({
                "id":format!("execution-{i:03}"),"taskId":task,
                "status":"interrupted","programResumeApproved":true,
                "programCheckpoint":{"programRevision":"p1"}
            })).unwrap();
        }
        let selected=store.task_executions("requested-task").unwrap();
        assert_eq!(selected.len(),2);
        assert!(selected.iter().all(|item|item["taskId"]=="requested-task"));
        assert_eq!(store.task_executions("absent-task").unwrap().len(),0);
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='execution'
             AND json_extract(data,'$.taskId')=?1",
            ["requested-task"],|row|row.get(3)).unwrap();
        assert!(plan.contains("idx_execution_task"),"{plan}");
    }

    #[test]
    fn indexed_operation_views_ignore_unrelated_history_and_preserve_recent_order() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..300 {
            let run=if i==80||i==120 {"run-focus"}else{"run-other"};
            let session=if i==80||i==120 {"session-focus"}else{"session-other"};
            store.put("execution_operation",&json!({
                "id":format!("op-{i:03}"),"runId":run,"sessionId":session,
                "opSeq":i,"status":"completed","tool":"workspace_read",
                "result":{"content":"x".repeat(1000)}
            })).unwrap();
        }
        let run=store.execution_operations_recent("run-focus").unwrap();
        let session=store.session_execution_operations("session-focus").unwrap();
        assert_eq!(run.len(),2);
        assert_eq!(session.len(),2);
        assert!(run.iter().all(|op|op["runId"]=="run-focus"));
        assert!(session.iter().all(|op|op["sessionId"]=="session-focus"));
        assert_eq!(store.execution_operations_recent("absent").unwrap().len(),0);
        let conn=store.conn.lock().unwrap();
        for (sql,key,index_name) in [
            ("EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='execution_operation' AND json_extract(data,'$.runId')=?1","run-focus","idx_operation_run"),
            ("EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='execution_operation' AND json_extract(data,'$.sessionId')=?1","session-focus","idx_operation_session")
        ] {
            let explain:String=conn.query_row(sql,[key],|row|row.get(3)).unwrap();
            assert!(explain.contains(index_name),"{explain}");
        }
    }

    #[test]
    fn indexed_active_job_view_excludes_terminated_history_and_keeps_order(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..500 {
            let status=if i==10||i==20 {"queued"}else if i==30 {"running"}else{"completed"};
            store.put("job",&json!({"id":format!("job-{i:03}"),
                "kind":"prompt","status":status,
                "queuedAt":format!("2026-11-16T{:04}Z",i)})).unwrap();
        }
        let active=store.active_jobs().unwrap();
        assert_eq!(active.len(),3);
        assert_eq!(active[0]["id"],"job-010");
        assert_eq!(active[1]["id"],"job-020");
        assert_eq!(active[2]["id"],"job-030");
        let conn=store.conn.lock().unwrap();
        let explain:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='job'
             AND json_extract(data,'$.status') IN ('queued','running')",
            [],|row|row.get(3)).unwrap();
        assert!(explain.contains("idx_job_active_status"),"{explain}");
    }

    #[test]
    fn run_leases_are_exclusive_and_recover_after_crash() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(store.acquire_lease("session:s", "owner-a", 60_000).is_some());
        assert!(
            store.acquire_lease("session:s", "owner-b", 60_000).is_none(),
            "a live lease must refuse a second owner"
        );
        assert!(store
            .acquire_lease("session:s", "owner-a", 60_000)
            .is_some());
        assert!(store.renew_lease("session:s", "owner-b", 60_000).is_none());
        assert!(store.renew_lease("session:s", "owner-a", 60_000).is_some());

        // A crashed owner stops renewing; a new process releases foreign and
        // expired leases before it resumes work.
        assert!(store.acquire_lease("session:t", "dead", -1).is_some());
        let released = store.release_stale_leases("new-instance");
        assert!(released >= 2, "released {released}");
        assert!(store.active_lease("session:s").is_none());
        assert!(store.acquire_lease("session:s", "owner-c", 60_000).is_some());
    }

    #[test]
    fn usage_summary_aggregates_calls_and_estimates() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.audit(
            "model.usage",
            json!({"providerId":"p1","model":"m","purpose":"chat","totalTokens":100,"estimated":true}),
        );
        store.audit(
            "model.usage",
            json!({"providerId":"p1","model":"m","purpose":"chat","totalTokens":50,"estimated":true}),
        );
        store.audit(
            "model.usage",
            json!({"providerId":"p2","model":"n","purpose":"helper","promptTokens":10,"completionTokens":5,"totalTokens":15}),
        );
        store.audit("tool.call", json!({"tool":"x"}));
        let summary = store.usage_summary();
        assert_eq!(summary["calls"], 3);
        assert_eq!(summary["totalTokens"], 165);
        assert_eq!(summary["promptTokens"], 10);
        assert_eq!(summary["completionTokens"], 5);
        assert_eq!(summary["estimatedCalls"], 2);
        let groups = summary["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["providerId"], "p1");
        assert_eq!(groups[0]["calls"], 2);
        assert_eq!(groups[1]["estimatedCalls"], 0);
    }

    #[test]
    fn attempt_creation_is_atomic_and_idempotent() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let root = json!({
            "id":"root-1","contract":{"revision":"r1"},
            "attempts":[],"rootOutcome":"running"
        });
        let execution = json!({"id":"exec-1","taskId":"root-1"});
        let attempt = json!({"executionId":"exec-1","status":"running"});
        assert_eq!(
            store.create_attempt(&root, &execution, &attempt).unwrap()["id"],
            "exec-1"
        );
        let episode = store.get("task_episode", "root-1").unwrap();
        assert_eq!(episode["attempts"].as_array().unwrap().len(), 1);
        store.create_attempt(&root, &execution, &attempt).unwrap();
        assert_eq!(
            store.get("task_episode", "root-1").unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "re-linking the same attempt must be idempotent"
        );
        store
            .create_attempt(
                &root,
                &json!({"id":"exec-2","taskId":"root-1"}),
                &json!({"executionId":"exec-2","status":"running"}),
            )
            .unwrap();
        assert_eq!(
            store.get("task_episode", "root-1").unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn attempt_creation_rolls_back_when_the_episode_is_corrupt() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put(
            "task_episode",
            &json!({"id":"root-2","attempts":"broken"}),
        );
        assert!(store
            .create_attempt(
                &json!({"id":"root-2"}),
                &json!({"id":"exec-3","taskId":"root-2"}),
                &json!({"executionId":"exec-3"}),
            )
            .is_none());
        assert!(
            store.get("execution", "exec-3").is_none(),
            "a failed attempt link must not leave a detached execution"
        );
    }

    #[test]
    fn records_round_trip_with_client_fields() {
        let (store, dir) = temp_store();
        let saved = store
            .put(
                "provider",
                &json!({ "name": "Local", "baseUrl": "http://127.0.0.1:4199/v1" }),
            )
            .expect("saved");
        let id = saved["id"].as_str().unwrap().to_string();
        assert_eq!(saved["name"], "Local");
        assert!(saved.get("createdAt").is_some());

        let fetched = store.get("provider", &id).unwrap();
        assert_eq!(fetched["baseUrl"], "http://127.0.0.1:4199/v1");

        let updated = store
            .put(
                "provider",
                &json!({ "id": id, "name": "Renamed", "baseUrl": "http://x" }),
            )
            .unwrap();
        assert_eq!(updated["name"], "Renamed");
        assert_eq!(store.all("provider").len(), 1);

        store.remove("provider", &id);
        assert!(store.get("provider", &id).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn messages_preserve_reasoning_and_order() {
        let (store, dir) = temp_store();
        store.write_message(&json!({
            "session_id": "s1", "role": "user", "content": "hello"
        }));
        store.write_message(&json!({
            "session_id": "s1", "role": "assistant", "content": "hi",
            "reasoning": "thought", "tool_calls": [{ "id": "c1" }]
        }));
        let messages = store.messages("s1");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["reasoning"], "thought");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        store.clear_messages("s1");
        assert!(store.messages("s1").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn try_messages_reads_the_same_rows_with_error_preservation() {
        let (store, dir) = temp_store();
        store.write_message(&json!({
            "session_id": "s1", "role": "user", "content": "hello"
        }));
        store.write_message(&json!({
            "session_id": "s1", "role": "assistant", "content": "hi"
        }));
        let strict = store.try_messages("s1").unwrap();
        assert_eq!(strict, store.messages("s1"));
        assert_eq!(strict.len(), 2);
        assert!(store.try_messages("missing").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_workers_claim_a_job_only_once() {
        let (store, dir) = temp_store();
        store
            .put_if_absent("job", &json!({"id":"one","status":"queued"}))
            .unwrap();
        assert!(store
            .put_if_absent("job", &json!({"id":"one","status":"overwritten"}))
            .is_none());
        let workers: Vec<_> = (0..12)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .patch_if(
                            "job",
                            "one",
                            "status",
                            &json!("queued"),
                            &json!({"status":"running"}),
                        )
                        .is_some()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .filter_map(|worker| worker.join().ok())
                .filter(|claimed| *claimed)
                .count(),
            1
        );
        assert_eq!(store.get("job", "one").unwrap()["status"], "running");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_finalizations_merge_improvement_counters() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put(
            "improvement",
            &json!({
                "id":"imp-1","status":"adopted",
                "uses":5,"successes":5,"failures":0,"appliedExecutionIds":[]
            }),
        );
        for (execution_id, status) in [("exec-a", "validated"), ("exec-b", "adopted")] {
            let execution = json!({"id":execution_id});
            let marker = json!({
                "id": format!("execution:{execution_id}"),
                "payloadHash": format!("hash-{execution_id}"),
            });
            let improvement = json!({
                "id":"imp-1","uses":6,"successes":6,"failures":0,"status":status,
                "appliedExecutionIds":[execution_id],
                "__counterDelta":{"uses":1,"successes":1,"failures":0},
            });
            assert_eq!(
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap(),
                FinalizeOutcome::Applied
            );
        }
        let merged = store.get("improvement", "imp-1").unwrap();
        assert_eq!(merged["uses"], 7, "one observation was lost: {merged}");
        assert_eq!(merged["successes"], 7);
        assert_eq!(merged["appliedExecutionIds"].as_array().unwrap().len(), 2);
        assert_eq!(merged["status"], "validated");
        assert!(merged.get("__counterDelta").is_none());
    }

    #[test]
    fn owner_dispatch_is_atomic_and_metadata_is_preserved() {
        let (store, dir) = temp_store();
        let message = json!({"id":"m1","session_id":"room:r","role":"user","content":"내 지시","metadata":{"authorKind":"owner","roomId":"r"}});
        let job = json!({"id":"j1","kind":"room","status":"queued"});
        let (saved, _) = store
            .write_message_with_record(&message, "job", &job)
            .unwrap();
        assert_eq!(saved["metadata"]["authorKind"], "owner");
        assert_eq!(store.message("m1").unwrap()["content"], "내 지시");
        let mut conflicting = message.clone();
        conflicting["id"] = json!("m2");
        assert!(store
            .write_message_with_record(&conflicting, "job", &job)
            .is_none());
        assert!(
            store.message("m2").is_none(),
            "queue failure must roll back owner insertion"
        );
        assert_eq!(store.messages("room:r").len(), 1);
        store.put("agent", &json!({"id":"a","model":"old"}));
        assert!(store
            .patch_records(
                "agent",
                &["a".into(), "missing".into()],
                &json!({"model":"new"})
            )
            .is_none());
        assert_eq!(store.get("agent", "a").unwrap()["model"], "old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancelled_or_interrupted_attempt_rejects_stale_finalization_atomically() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,status) in [("cancel-race","cancelled"),("restart-race","interrupted")] {
            store.put("execution",&json!({"id":id,"status":status,
                "reason":"owner terminal decision"})).unwrap();
            let stale=json!({"id":id,"status":"completed","taskOutcome":"succeeded"});
            let marker=json!({"id":format!("execution:{id}"),"payloadHash":"stale"});
            let counters=json!({"id":format!("imp:{id}"),"uses":1});
            assert_eq!(store.finalize_once(&stale,&marker,&[counters],&[]).unwrap(),
                FinalizeOutcome::Conflict);
            let current=store.get("execution",id).unwrap();
            assert_eq!(current["status"],status);
            assert_eq!(current["reason"],"owner terminal decision");
            assert!(store.get("execution_finalization",
                &format!("execution:{id}")).is_none());
            assert!(store.get("improvement",&format!("imp:{id}")).is_none());
        }
    }

    #[test]
    fn finalization_is_idempotent_and_conflict_aware() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-1","status":"completed"});
        let marker = json!({"id":"execution:exec-1","payloadHash":"hash-1"});
        let improvement = json!({"id":"imp-1","uses":0,"status":"adopted"});
        let observation = json!({"id":"obs-1"});
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::Applied
        );
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::AlreadyApplied
        );
        let conflicting = json!({"id":"execution:exec-1","payloadHash":"hash-2"});
        assert_eq!(
            store
                .finalize_once(&execution, &conflicting, &[], &[])
                .unwrap(),
            FinalizeOutcome::Conflict
        );
        assert_eq!(store.get("improvement", "imp-1").unwrap()["uses"], 0);
        assert_eq!(store.try_all("improvement").unwrap().len(), 1);
    }

    #[test]
    fn concurrent_finalizations_apply_once() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-2","status":"completed"});
        let marker = json!({"id":"execution:exec-2","payloadHash":"hash-2"});
        let improvement = json!({"id":"imp-2","uses":0});
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = store.clone();
            let execution = execution.clone();
            let marker = marker.clone();
            let improvement = improvement.clone();
            handles.push(std::thread::spawn(move || {
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap()
            }));
        }
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(outcomes.contains(&FinalizeOutcome::Applied));
        assert!(outcomes.contains(&FinalizeOutcome::AlreadyApplied));
        assert_eq!(store.try_all("execution_finalization").unwrap().len(), 1);
    }

    #[test]
    fn try_all_preserves_records_and_returns_them() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("memory", &json!({"id":"m1","text":"a"}));
        store.put("memory", &json!({"id":"m2","text":"b"}));
        let records = store.try_all("memory").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn broken_finalization_marker_is_an_error() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO records(kind,id,data) VALUES('execution_finalization','execution:broken','{bad')",
                [],
            )
            .unwrap();
        }
        let execution = json!({"id":"broken","status":"completed"});
        let marker = json!({"id":"execution:broken","payloadHash":"hash"});
        assert!(store.finalize_once(&execution, &marker, &[], &[]).is_err());
    }
}
