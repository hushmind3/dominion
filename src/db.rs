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
             -- Session usage is read repeatedly during active jobs. A filtered
             -- expression index avoids scanning the
             -- entire lifetime audit log on every progress poll. Corrupt
             -- legacy entries stay untouched and are excluded safely.
             CREATE INDEX IF NOT EXISTS idx_audit_usage_session
                 ON audit(json_extract(detail,'$.sessionId'))
                 WHERE event='model.usage' AND json_valid(detail);
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
             CREATE INDEX IF NOT EXISTS idx_job_status
                 ON records(json_extract(data,'$.status'))
                 WHERE kind='job' AND json_extract(data,'$.status') IN ('queued','running');
             CREATE INDEX IF NOT EXISTS idx_operation_run
                 ON records(json_extract(data,'$.runId'))
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_operation_session
                 ON records(json_extract(data,'$.sessionId'))
                 WHERE kind='execution_operation';
             -- Effect admission is called on every mutating tool. Read only
             -- same-resource version history and exact effect retries instead
             -- of deserializing every operation in a long-lived session.
             CREATE INDEX IF NOT EXISTS idx_operation_session_resource
                 ON records(json_extract(data,'$.sessionId'),
                            json_extract(data,'$.resourceKey'), updated_at DESC)
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_operation_session_effect
                 ON records(json_extract(data,'$.sessionId'),
                            json_extract(data,'$.effectId'), updated_at DESC)
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_algorithm_kind
                 ON records(json_extract(data,'$.kind'))
                 WHERE kind='algorithm';
             CREATE INDEX IF NOT EXISTS idx_execution_task
                 ON records(json_extract(data,'$.taskId'))
                 WHERE kind='execution';",
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

    /// Hot-path domain-scoped algorithm lookup. Historical statistics and
    /// unrelated kinds are not deserialized for every agent/tool decision.
    /// The partial expression index preserves existing JSON record schema.
    pub fn algorithms_for_kind(&self, kind: &str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let mut statement=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='algorithm' AND json_extract(data,'$.kind')=?1
             ORDER BY id",
        ).map_err(|error|error.to_string())?;
        let rows=statement.query_map([kind],record_from_row)
            .map_err(|error|error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())
    }

    /// Queue worker read: the indexed active subset, not every historical
    /// completed/cancelled/failed job. Old SQLite data remains untouched.
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

    /// Query running checkpoints only; historical records remain untouched.
    pub fn running_recovery_records(&self,kind:&str)->Result<Vec<Value>,String>{
        let sql=match kind{
            "execution"=>"SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution' AND json_extract(data,'$.status')='running'",
            "execution_operation"=>"SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution_operation' AND json_extract(data,'$.status')='running'",
            _=>return Err(format!("unsupported recovery kind: {kind}")),
        };
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(sql).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// A tool's sessionId can be absent: inherit it from the owning execution.
    /// Uncertain external effects must block unsafe replay after restart.
    pub fn uncertain_effect_sessions(&self)
        ->Result<std::collections::HashSet<String>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "WITH uncertain AS (
                SELECT data FROM records WHERE kind='execution_operation'
                  AND (json_extract(data,'$.outcomeUnknown')=1
                    OR json_extract(data,'$.status')='running'
                    OR json_extract(data,'$.effectState') IN
                        ('prepared','started','effect_unknown','needs_review'))
             )
             SELECT session FROM (
                SELECT json_extract(data,'$.sessionId') AS session FROM uncertain
                UNION
                SELECT json_extract(ex.data,'$.sessionId') AS session
                FROM uncertain AS op
                JOIN records AS ex ON ex.kind='execution'
                    AND ex.id=json_extract(op.data,'$.runId')
             ) WHERE session IS NOT NULL AND session!=''"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],|row|row.get::<_,String>(0))
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<std::collections::HashSet<_>,_>>()
            .map_err(|e|e.to_string())
    }

    /// Main Home UI polls per-job progress frequently. Read only executions
    /// for the one owning session instead of materializing all engine runs for
    /// every card, and propagate database failures rather than fabricating
    /// an empty progress state.
    pub fn session_executions(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records
             WHERE kind='execution' AND json_extract(data,'$.sessionId')=?1
             ORDER BY created_at ASC, id ASC",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map([session_id], record_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// Only this session's execution effects, in newest-first order.
    pub fn session_operations(&self, session_id:&str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation' AND json_extract(data,'$.sessionId')=?1
             ORDER BY updated_at DESC,id DESC"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([session_id],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Exact resource-version and effect-retry scope, newest first.
    /// UNION ALL keeps both partial expression indexes eligible, while the
    /// second arm excludes resources already returned by the first. This
    /// preserves every earlier unresolved effect without a lossy LIMIT.
    pub fn related_effect_operations(
        &self,
        session_id: &str,
        resource_key: Option<&str>,
        effect_id: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        if resource_key.is_none() && effect_id.is_none() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn.prepare(
            "SELECT data,id,created_at,updated_at FROM (
                SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution_operation'
                  AND json_extract(data,'$.sessionId')=?1
                  AND json_extract(data,'$.resourceKey')=?2
                UNION ALL
                SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution_operation'
                  AND json_extract(data,'$.sessionId')=?1
                  AND json_extract(data,'$.effectId')=?3
                  AND (?2 IS NULL OR json_extract(data,'$.resourceKey') IS NOT ?2)
             ) ORDER BY updated_at DESC,id DESC"
        ).map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![session_id, resource_key, effect_id], record_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// Resume only snapshots of the requested task, not all executions.
    pub fn task_executions(&self, task_id:&str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution' AND json_extract(data,'$.taskId')=?1
             ORDER BY created_at DESC,id DESC"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([task_id],record_from_row)
            .map_err(|e|e.to_string())?;
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

    /// Bounded native trace read for 2026 HarnessFix-style failure provenance.
    /// Uses the existing partial index idx_operation_run rather than scanning
    /// all session operations or materializing every execution record.
    pub fn recent_execution_operations(
        &self,
        run_id: &str,
        limit: usize,
    ) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut statement = conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation' AND json_extract(data,'$.runId')=?1
             ORDER BY CAST(json_extract(data,'$.opSeq') AS INTEGER) DESC, id DESC
             LIMIT ?2",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map(params![run_id, limit.clamp(1, 96) as i64], record_from_row)
            .map_err(|error| error.to_string())?;
        let mut operations = rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        operations.reverse();
        Ok(operations)
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

    /// Compare-and-patch under the connection lock, used to claim queued work.
    /// SQLite IMMEDIATE transaction makes the compare+write linearizable
    /// across independently opened database connections, not just threads
    /// sharing one Store mutex. The same primitive owns claims, cancellation,
    /// steering and restart reconciliation.
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
        if record.get(field).unwrap_or(&Value::Null)!=expected {return None;}
        for (key,value) in changes{
            if key!="id"{record[key]=value.clone();}
        }
        tx.execute(
            "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
            params![kind,id,record.to_string()],
        ).ok()?;
        tx.commit().ok()?;
        Some(record)
    }

    /// Advance a provider route and participant cursor as one durable event.
    /// The second guard must not leave a half-committed first row behind:
    /// SQLite rolls back both writes on any failed compare or update.
    pub fn patch_pair_if(
        &self,
        first: (&str, &str, &Value, &Value),
        second: (&str, &str, &Value, &Value),
    ) -> Option<()> {
        let mut conn = self.conn.lock().ok()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        for (kind, id, guards, patch) in [first, second] {
            let guards = guards.as_object()?;
            let changes = patch.as_object()?;
            // The method is for cursor/progress patches, not record reparenting.
            if changes.keys().any(|key| matches!(
                key.as_str(), "id" | "kind" | "sessionId" | "agentId"
            )) {
                return None;
            }
            let mut record = tx.query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                record_from_row,
            ).ok()?;
            if guards.iter().any(|(field, expected)| {
                record.get(field).unwrap_or(&Value::Null) != expected
            }) {
                return None;
            }
            for (field, value) in changes {
                record[field] = value.clone();
            }
            tx.execute(
                "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
                params![kind, id, record.to_string()],
            ).ok()?;
        }
        tx.commit().ok()
    }

    /// The running job attempt, not merely the job ID, owns all progress
    /// and terminal transitions. A stale async provider/tool worker cannot
    /// write after cancellation or after the dispatcher requeues an attempt.
    /// Keeps the existing nested videoState merge for legacy record fidelity.
    pub fn checkpoint_running_job(
        &self,id:&str,expected_attempt:u64,expected_epoch:u64,patch:&Value,
    )->Option<Value>{
        let changes=patch.as_object()?;
        if changes.contains_key("id")||changes.contains_key("cancelRequested")
            ||changes.contains_key("kind")||changes.contains_key("input")
            ||changes.contains_key("runEpoch"){
            return None;
        }
        // Only the native deferred-job path may release a running attempt
        // back to queued and refund its not-yet-executed admission count.
        // Other patches may not silently rewrite the owner attempt number.
        if let Some(proposed)=changes.get("attempts"){
            if changes.get("status").and_then(Value::as_str)!=Some("queued")
                ||proposed.as_u64()!=Some(expected_attempt.saturating_sub(1)){
                return None;
            }
        }
        let mut conn=self.conn.lock().ok()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        let mut record=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='job' AND id=?1",
            [id],record_from_row,
        ).ok()?;
        if record["status"]!="running" || record["cancelRequested"]==true
            || record["attempts"].as_u64().unwrap_or(0)!=expected_attempt
            || record["runEpoch"].as_u64().unwrap_or(0)!=expected_epoch{
            return None;
        }
        for (key,value) in changes {
            if key=="videoState"{
                if let (Some(current),Some(update))=(
                    record.get_mut(key).and_then(Value::as_object_mut),
                    value.as_object()
                ){
                    for (field,next) in update{
                        current.insert(field.clone(),next.clone());
                    }
                    continue;
                }
            }
            record[key]=value.clone();
        }
        tx.execute(
            "UPDATE records SET data=?2,updated_at=datetime('now') WHERE kind='job' AND id=?1",
            params![id,record.to_string()],
        ).ok()?;
        tx.commit().ok()?;
        Some(record)
    }

    /// Atomically merge a checkpoint against the latest persisted record.
    /// Guards are checked inside the same SQLite connection lock that writes
    /// the result. Nested object keys merge against the *fresh* record, so
    /// concurrent workers cannot drop a scene plan, receipt, or progress field.
    /// Callers own the allowed-state policy; the database only enforces it.
    pub fn patch_if_all_merge(
        &self,
        kind: &str,
        id: &str,
        guards: &[(&str, &Value)],
        patch: &Value,
        nested_objects: &[&str],
    ) -> Option<Value> {
        let changes=patch.as_object()?;
        let mut conn=self.conn.lock().ok()?;
        // A process-local mutex cannot serialize a separate application
        // process running recovery/cancellation against this connection.
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).ok()?;
        let mut record=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind,id],record_from_row,
        ).ok()?;
        if guards.iter().any(|(key,expected)|
            record.get(*key).unwrap_or(&Value::Null)!=*expected){
            return None;
        }
        for (key,value) in changes{
            if key=="id"{continue;}
            if nested_objects.iter().any(|nested|*nested==key.as_str()){
                if let (Some(current),Some(update))=(
                    record.get_mut(key).and_then(Value::as_object_mut),value.as_object()){
                    for (field,next) in update {
                        current.insert(field.clone(),next.clone());
                    }
                    continue;
                }
            }
            record[key]=value.clone();
        }
        tx.execute(
            "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
            params![kind,id,record.to_string()],
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

    /// Read just the rows the conversation model can use. Old full tool
    /// outputs remain in SQLite and can be opened by scoped source ID.
    /// The DESC index walk keeps long-lived sessions cheap to resume.
    pub fn try_recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata
             FROM messages WHERE session_id=?1 ORDER BY created_at DESC,rowid DESC LIMIT ?2"
        ).map_err(|error|error.to_string())?;
        let rows=stmt.query_map(rusqlite::params![session_id,limit.min(256) as i64],
            message_from_row).map_err(|error|error.to_string())?;
        let mut selected=Vec::new();
        for row in rows{selected.push(row.map_err(|error|error.to_string())?);}
        selected.reverse();
        Ok(selected)
    }

    /// Restore owner-authored task goals without ever decoding old HTML,
    /// model reasoning or tool arguments from the full session archive.
    pub fn try_recent_user_messages(&self, session_id: &str, limit: usize) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata
             FROM messages WHERE session_id=?1 AND role='user'
             ORDER BY created_at DESC,rowid DESC LIMIT ?2"
        ).map_err(|error|error.to_string())?;
        let rows=stmt.query_map(rusqlite::params![session_id,limit.min(64) as i64],
            message_from_row).map_err(|error|error.to_string())?;
        let mut selected=Vec::new();
        for row in rows{selected.push(row.map_err(|error|error.to_string())?);}
        selected.reverse();
        Ok(selected)
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
            return json!({"calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0,"cachedPromptTokens":0,"cacheObservedCalls":0,"groups":[]});
        };
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        let Ok(rows) = rows else {
            return json!({"calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0,"cachedPromptTokens":0,"cacheObservedCalls":0,"groups":[]});
        };
        let mut groups: std::collections::HashMap<String, Value> =
            std::collections::HashMap::new();
        let mut calls = 0u64;
        let mut prompt = 0u64;
        let mut completion = 0u64;
        let mut total = 0u64;
        let mut estimated = 0u64;
        let mut cached = 0u64;
        let mut cache_observed = 0u64;
        let mut priced_calls = 0u64;
        let mut cost_subtotal = 0.0f64;
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
                    "cachedPromptTokens": 0,
                    "cacheObservedCalls": 0,
                    "pricedCalls":0,
                    "pricedSubtotalUsd":0.0,
                })
            });
            calls += 1;
            if let Some(amount)=detail["configuredCost"]["amountUsd"].as_f64()
                .filter(|v|v.is_finite() && *v>=0.0){
                priced_calls+=1;
                cost_subtotal+=amount;
                entry["pricedCalls"]=json!(entry["pricedCalls"].as_u64().unwrap_or(0)+1);
                entry["pricedSubtotalUsd"]=json!(
                    entry["pricedSubtotalUsd"].as_f64().unwrap_or(0.0)+amount);
            }
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
            // Absent telemetry is not a cache miss. Never infer a hit from
            // stable prompts or count the same tokens twice.
            if detail["estimated"]!=true {
                if let Some(hits)=detail["cachedPromptTokens"].as_u64() {
                    let bounded=hits.min(prompt_tokens);
                    cached=cached.saturating_add(bounded);
                    cache_observed=cache_observed.saturating_add(1);
                    entry["cachedPromptTokens"]=json!(
                        entry["cachedPromptTokens"].as_u64().unwrap_or(0).saturating_add(bounded));
                    entry["cacheObservedCalls"]=json!(
                        entry["cacheObservedCalls"].as_u64().unwrap_or(0).saturating_add(1));
                }
            }
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
            "cachedPromptTokens": cached,
            "cacheObservedCalls": cache_observed,
            "pricedCalls":priced_calls,
            "unpricedCalls":calls.saturating_sub(priced_calls),
            "pricedSubtotalUsd":cost_subtotal,
            "configuredCostUsd":if priced_calls==calls {json!(cost_subtotal)} else {Value::Null},
            "costEvidence":"owner_configured_rates_only_excludes_gpu_and_provider_invoices",
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
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer' AND json_extract(detail,'$.estimated')!=1
                        THEN CAST(json_extract(detail,'$.cachedPromptTokens') AS INTEGER) ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer' AND json_extract(detail,'$.estimated')!=1
                        THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.configuredCost.amountUsd') IN ('real','integer')
                        THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.configuredCost.amountUsd') AS REAL)),0)
             FROM audit
             WHERE event='model.usage' AND json_valid(detail)
                 AND json_extract(detail,'$.sessionId')=?1",
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
                    row.get::<_, f64>(8)?,
                ))
            },
        );
        match summary {
            Ok((calls, prompt, completion, total, estimated, cached, cache_observed, priced, cost)) => json!({
                "calls": calls.max(0),
                "promptTokens": prompt.max(0),
                "completionTokens": completion.max(0),
                "totalTokens": total.max(0),
                "estimatedCalls": estimated.max(0),
                "cachedPromptTokens":cached.max(0),
                "cacheObservedCalls":cache_observed.max(0),
                "pricedCalls":priced.max(0),
                "unpricedCalls":(calls-priced).max(0),
                "pricedSubtotalUsd":cost.max(0.0),
                "configuredCostUsd":if priced==calls {json!(cost.max(0.0))} else {Value::Null},
            }),
            Err(_) => json!({
                "calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0,
                "cachedPromptTokens":0,"cacheObservedCalls":0,
                "pricedCalls":0,"unpricedCalls":0,"pricedSubtotalUsd":0.0,"configuredCostUsd":Value::Null
            }),
        }
    }


    /// Decomposes the audit ledger without ever copying prompts, HTML or tool
    /// arguments into telemetry. The lifetime counter includes prior cleared
    /// transcripts and helper calls; the view since the last clear is separate.
    pub fn session_usage_breakdown(&self, session_id:&str)->Value{
        let conn=self.conn.lock().unwrap();
        let last_clear=conn.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM audit
             WHERE event='chat.session_cleared' AND json_valid(detail)
               AND json_extract(detail,'$.sessionId')=?1",
            [session_id],|row|row.get::<_,i64>(0)
        ).unwrap_or(0);
        let groups=conn.prepare(
            "SELECT COALESCE(json_extract(detail,'$.purpose'),'unknown'),COUNT(*),
                    COALESCE(SUM(CAST(json_extract(detail,'$.promptTokens') AS INTEGER)),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.completionTokens') AS INTEGER)),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.totalTokens') AS INTEGER)),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer'
                        AND json_extract(detail,'$.estimated')!=1
                        THEN CAST(json_extract(detail,'$.cachedPromptTokens') AS INTEGER) ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer'
                        AND json_extract(detail,'$.estimated')!=1 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_extract(detail,'$.estimated')=1 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN rowid>?2 THEN CAST(json_extract(detail,'$.totalTokens') AS INTEGER) ELSE 0 END),0)
             FROM audit WHERE event='model.usage' AND json_valid(detail)
               AND json_extract(detail,'$.sessionId')=?1
             GROUP BY COALESCE(json_extract(detail,'$.purpose'),'unknown') ORDER BY COUNT(*) DESC"
        ).and_then(|mut stmt|{
            stmt.query_map(rusqlite::params![session_id,last_clear],|row|{
                Ok(json!({
                    "purpose":row.get::<_,String>(0)?,"calls":row.get::<_,i64>(1)?,
                    "promptTokens":row.get::<_,i64>(2)?,"completionTokens":row.get::<_,i64>(3)?,
                    "totalTokens":row.get::<_,i64>(4)?,"observedCachedTokens":row.get::<_,i64>(5)?,
                    "cacheObservedCalls":row.get::<_,i64>(6)?,"estimatedCalls":row.get::<_,i64>(7)?,
                    "tokensSinceClear":row.get::<_,i64>(8)?
                }))
            }).and_then(|rows|rows.collect::<rusqlite::Result<Vec<_>>>())
        }).unwrap_or_default();
        let recent=conn.prepare(
            "SELECT created_at,detail FROM audit
             WHERE event='model.usage' AND json_valid(detail)
               AND json_extract(detail,'$.sessionId')=?1 ORDER BY rowid DESC LIMIT 12"
        ).and_then(|mut stmt|{
            stmt.query_map([session_id],|row|{
                let at:String=row.get(0)?;
                let raw:String=row.get(1)?;
                let v=serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
                Ok(json!({"recordedAt":at,"purpose":v["purpose"],"model":v["model"],
                    "promptTokens":v["promptTokens"],"completionTokens":v["completionTokens"],
                    "totalTokens":v["totalTokens"],"cachedPromptTokens":v["cachedPromptTokens"],
                    "estimated":v["estimated"]}))
            }).and_then(|rows|rows.collect::<rusqlite::Result<Vec<_>>>())
        }).unwrap_or_default();
        let wire_profiles=conn.prepare(
            "SELECT detail FROM audit
             WHERE event='chat.provider_prompt_profile' AND json_valid(detail)
               AND json_extract(detail,'$.sessionId')=?1 ORDER BY rowid DESC LIMIT 8"
        ).and_then(|mut stmt|{
            stmt.query_map([session_id],|row|{
                let raw:String=row.get(0)?;
                Ok(serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null))
            }).and_then(|rows|rows.collect::<rusqlite::Result<Vec<_>>>())
        }).unwrap_or_default();
        let latest_sources=conn.query_row(
            "SELECT json_extract(data,'$.inputContext.sourceCharacterCounts')
             FROM records WHERE kind='execution' AND json_valid(data)
               AND json_extract(data,'$.sessionId')=?1
               AND json_type(data,'$.inputContext.sourceCharacterCounts')='object'
             ORDER BY updated_at DESC,rowid DESC LIMIT 1",
            [session_id],|row|row.get::<_,String>(0)
        ).ok().and_then(|raw|serde_json::from_str::<Value>(&raw).ok());
        json!({"byPurpose":groups,"recentCalls":recent,
            "recentPromptProfiles":wire_profiles,"latestSourceCharacterCounts":latest_sources,
            "hasClearMarker":last_clear>0,
            "scope":"lifetime session audit including earlier cleared transcripts and helper calls",
            "cachePolicy":"cached tokens are included in promptTokens; unreported cache remains unknown"})
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
    fn checkpoint_merges_concurrent_scene_updates_without_lost_progress() {
        let (store,dir)=temp_store();
        let initial=store.put("job",&json!({
            "status":"running","cancelRequested":false,
            "videoState":{"scenes":["scene-1","scene-2"],"sceneTotal":2}
        })).unwrap();
        let id=initial["id"].as_str().unwrap().to_string();
        let guard_status=json!("running");
        let guard_cancel=json!(false);
        let conditions=[("status",&guard_status),("cancelRequested",&guard_cancel)];
        std::thread::scope(|scope|{
            let s1=store.clone();
            let id1=id.clone();
            let g1=guard_status.clone();
            let c1=guard_cancel.clone();
            scope.spawn(move||{
                let guards=[("status",&g1),("cancelRequested",&c1)];
                s1.patch_if_all_merge("job",&id1,&guards,
                    &json!({"videoState":{"promptId":"prompt-1","inFlight":true}}),
                    &["videoState"]).unwrap();
            });
            let s2=store.clone();
            let id2=id.clone();
            let g2=guard_status.clone();
            let c2=guard_cancel.clone();
            scope.spawn(move||{
                let guards=[("status",&g2),("cancelRequested",&c2)];
                s2.patch_if_all_merge("job",&id2,&guards,
                    &json!({"videoState":{"renderedScenes":1,"sceneIndex":1}}),
                    &["videoState"]).unwrap();
            });
        });
        let saved=store.get("job",&id).unwrap();
        assert_eq!(saved["videoState"]["scenes"],json!(["scene-1","scene-2"]));
        assert_eq!(saved["videoState"]["promptId"],"prompt-1");
        assert_eq!(saved["videoState"]["renderedScenes"],1);
        assert_eq!(saved["videoState"]["sceneIndex"],1);
        assert_eq!(saved["videoState"]["sceneTotal"],2);
        assert!(store.patch_if_all_merge("job",&id,&conditions,
            &json!({"videoState":{"promptId":null}}),&["videoState"]).is_some());
        assert!(store.get("job",&id).unwrap()["videoState"]["promptId"].is_null());
        store.patch_if("job",&id,"status",&json!("running"),
            &json!({"status":"cancelled","cancelRequested":true})).unwrap();
        assert!(store.patch_if_all_merge("job",&id,&conditions,
            &json!({"progress":100}),&["videoState"]).is_none());
        assert!(store.get("job",&id).unwrap()["progress"].is_null());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
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
    fn recovery_merge_guards_reject_stale_running_claim_across_connections(){
        let (first,dir)=temp_store();
        let second=Store::open(&dir.join("dominion.sqlite")).unwrap();
        first.put("job",&json!({"id":"recovery-gate","status":"running",
            "attempts":1,"runEpoch":1,"cancelRequested":false})).unwrap();
        second.patch_if("job","recovery-gate","status",&json!("running"),
            &json!({"status":"queued"})).unwrap();
        second.patch_if("job","recovery-gate","status",&json!("queued"),
            &json!({"status":"running","attempts":2,"runEpoch":2})).unwrap();
        let status=json!("running");
        let attempts=json!(1);
        let epoch=json!(1);
        let cancelled=json!(false);
        let guards=[("status",&status),("attempts",&attempts),
            ("runEpoch",&epoch),("cancelRequested",&cancelled)];
        assert!(first.patch_if_all_merge("job","recovery-gate",&guards,
            &json!({"status":"needs_review","recovered":true}),&[]).is_none());
        let saved=second.get("job","recovery-gate").unwrap();
        assert_eq!(saved["status"],"running");
        assert_eq!(saved["runEpoch"],2);
        assert!(saved["recovered"].is_null());
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn independent_sqlite_connections_cannot_claim_same_queued_job(){
        let (first,dir)=temp_store();
        let second=Store::open(&dir.join("dominion.sqlite")).unwrap();
        first.put("job",&json!({"id":"claimed-once","status":"queued","attempts":0})).unwrap();
        let a=first.clone();
        let b=second.clone();
        let joined=std::thread::spawn(move||{
            a.patch_if("job","claimed-once","status",&json!("queued"),
                &json!({"status":"running","attempts":1})).is_some()
        });
        let other=std::thread::spawn(move||{
            b.patch_if("job","claimed-once","status",&json!("queued"),
                &json!({"status":"running","attempts":1})).is_some()
        });
        let winners=usize::from(joined.join().unwrap())
            +usize::from(other.join().unwrap());
        assert_eq!(winners,1,"a queued job must have exactly one owner");
        assert_eq!(second.get("job","claimed-once").unwrap()["attempts"],1);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_attempt_checkpoint_cannot_mutate_new_attempt_or_cancelled_job(){
        let (first,dir)=temp_store();
        let second=Store::open(&dir.join("dominion.sqlite")).unwrap();
        first.put("job",&json!({"id":"attempt-fence","status":"running",
            "attempts":1,"promptState":{"inFlight":true},"result":null})).unwrap();
        assert!(first.checkpoint_running_job("attempt-fence",1,0,
            &json!({"promptState":{"inFlight":false},"progress":"old"})).is_some());
        second.patch_if("job","attempt-fence","status",&json!("running"),
            &json!({"status":"queued"})).unwrap();
        second.patch_if("job","attempt-fence","status",&json!("queued"),
            &json!({"status":"running","attempts":2})).unwrap();
        assert!(first.checkpoint_running_job("attempt-fence",1,0,
            &json!({"status":"completed","result":"stale response"})).is_none());
        assert!(first.checkpoint_running_job("attempt-fence",2,0,
            &json!({"status":"running","cancelRequested":false})).is_none());
        assert!(first.checkpoint_running_job("attempt-fence",2,0,
            &json!({"attempts":0,"status":"completed"})).is_none());
        assert!(second.checkpoint_running_job("attempt-fence",2,0,
            &json!({"progress":"new"})).is_some());
        first.patch_if("job","attempt-fence","status",&json!("running"),
            &json!({"status":"cancelled","cancelRequested":true})).unwrap();
        assert!(second.checkpoint_running_job("attempt-fence",2,0,
            &json!({"status":"completed","result":"late success"})).is_none());
        let saved=first.get("job","attempt-fence").unwrap();
        assert_eq!(saved["status"],"cancelled");
        assert_eq!(saved["progress"],"new");
        assert!(saved["result"].is_null());
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refunded_attempts_do_not_reauthorize_old_workers_from_prior_epochs(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("job",&json!({"id":"refunded",
            "status":"running","attempts":1,"runEpoch":1})).unwrap();
        assert!(store.checkpoint_running_job("refunded",1,1,
            &json!({"status":"queued","attempts":0,"availableAt":"now"})).is_some());
        store.patch_if("job","refunded","status",&json!("queued"),
            &json!({"status":"running","attempts":1,"runEpoch":2})).unwrap();
        assert!(store.checkpoint_running_job("refunded",1,1,
            &json!({"status":"completed","result":"old attempt"})).is_none());
        assert!(store.checkpoint_running_job("refunded",1,2,
            &json!({"status":"completed","result":"new attempt"})).is_some());
        assert_eq!(store.get("job","refunded").unwrap()["result"],"new attempt");
    }

    #[test]
    fn deferred_job_checkpoint_can_requeue_without_replaying_old_owner(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("job",&json!({"id":"defer","status":"running","attempts":1})).unwrap();
        let queued=store.checkpoint_running_job("defer",1,0,
            &json!({"status":"queued","attempts":0,"availableAt":"2026-11-18T00:00:00Z"}))
            .unwrap();
        assert_eq!(queued["status"],"queued");
        assert_eq!(queued["attempts"],0);
        assert!(store.checkpoint_running_job("defer",1,0,
            &json!({"status":"completed"})).is_none());
        let new=store.patch_if("job","defer","status",&json!("queued"),
            &json!({"status":"running","attempts":1})).unwrap();
        assert_eq!(new["attempts"],1);
        // The previous worker could otherwise be mistaken for this attempt.
        // A fresh admission epoch is needed when the counter is refunded.
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
    fn indexed_recent_history_skips_archived_tool_dumps_and_preserves_order(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.write_message(&json!({"session_id":"recent","role":"user",
            "content":"Original owner task"})).unwrap();
        for index in 0..70 {
            store.write_message(&json!({"session_id":"recent","role":"tool",
                "content":format!("{index}:{}","OLD_HTML_".repeat(2000)),
                "tool_call_id":format!("call-{index}")})).unwrap();
        }
        store.write_message(&json!({"session_id":"recent","role":"user",
            "content":"이어서해"})).unwrap();
        store.write_message(&json!({"session_id":"other","role":"user",
            "content":"ISOLATED_PRIVATE"})).unwrap();
        let latest=store.try_recent_messages("recent",4).unwrap();
        assert_eq!(latest.len(),4);
        assert_eq!(latest.last().unwrap()["content"],"이어서해");
        assert!(!json!(latest).to_string().contains("0:OLD_HTML_"));
        let owners=store.try_recent_user_messages("recent",16).unwrap();
        assert_eq!(owners.len(),2);
        assert_eq!(owners[0]["content"],"Original owner task");
        assert_eq!(owners[1]["content"],"이어서해");
        assert!(!json!(owners).to_string().contains("OLD_HTML_"));
        assert!(!json!(owners).to_string().contains("ISOLATED_PRIVATE"));
        assert_eq!(store.messages("recent").len(),72,
            "indexed retrieval must never mutate or drop archived evidence");
        let empty=store.try_recent_messages("not-this-session",10).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn audit_usage_session_index_is_scoped_and_ignores_unrelated_records(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.audit("model.usage",json!({"sessionId":"usage-s","promptTokens":350,
            "completionTokens":150,"totalTokens":500,"cachedPromptTokens":100,
            "estimated":false}));
        store.audit("model.usage",json!({"sessionId":"other","promptTokens":10_000,
            "completionTokens":1000,"totalTokens":11_000,"estimated":false}));
        store.audit("irrelevant",json!({"sessionId":"usage-s","totalTokens":90_000}));
        let usage=store.session_usage("usage-s");
        assert_eq!(usage["calls"],1);
        assert_eq!(usage["totalTokens"],500);
        assert_eq!(usage["cachedPromptTokens"],100);
        assert_eq!(usage["unpricedCalls"],1);
        assert!(usage["configuredCostUsd"].is_null());
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM audit
                WHERE event='model.usage' AND json_valid(detail)
                  AND json_extract(detail,'$.sessionId')='usage-s'",
            [],|row|row.get(3)).unwrap();
        assert!(plan.contains("idx_audit_usage_session"),"{plan}");
    }

    #[test]
    fn session_usage_breakdown_keeps_lifetime_and_since_clear_separate(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.audit("model.usage",json!({"sessionId":"s","purpose":"chat",
            "promptTokens":56000,"completionTokens":100,"totalTokens":56100,
            "cachedPromptTokens":50000,"estimated":false}));
        store.audit("model.usage",json!({"sessionId":"s","purpose":"helper",
            "promptTokens":1000,"completionTokens":20,"totalTokens":1020,
            "estimated":true}));
        store.audit("chat.session_cleared",json!({"sessionId":"s"}));
        store.audit("model.usage",json!({"sessionId":"s","purpose":"chat",
            "promptTokens":250,"completionTokens":50,"totalTokens":300,"estimated":false}));
        store.audit("model.usage",json!({"sessionId":"other","purpose":"chat",
            "promptTokens":500000,"completionTokens":0,"totalTokens":500000}));
        assert_eq!(store.session_usage("s")["totalTokens"],57420);
        let report=store.session_usage_breakdown("s");
        let groups=report["byPurpose"].as_array().unwrap();
        let chat=groups.iter().find(|g|g["purpose"]=="chat").unwrap();
        let helper=groups.iter().find(|g|g["purpose"]=="helper").unwrap();
        assert_eq!(chat["calls"],2);
        assert_eq!(chat["totalTokens"],56400);
        assert_eq!(chat["tokensSinceClear"],300);
        assert_eq!(chat["observedCachedTokens"],50000);
        assert_eq!(helper["totalTokens"],1020);
        assert_eq!(helper["tokensSinceClear"],0);
        assert_eq!(report["recentCalls"].as_array().unwrap().len(),3);
        assert_eq!(report["hasClearMarker"],true);
        store.put("execution",&json!({"id":"exam","sessionId":"s","inputContext":{
            "sourceCharacterCounts":{"fixedSystemPrompt":54000,"agentPersona":52000,
                "ownerInstructions":1000,"selectedHistory":120}}})).unwrap();
        let explained=store.session_usage_breakdown("s");
        assert_eq!(explained["latestSourceCharacterCounts"]["agentPersona"],52000);
        assert_eq!(explained["latestSourceCharacterCounts"]["selectedHistory"],120);
        assert!(explained["recentPromptProfiles"].as_array().unwrap().is_empty());
    }

    #[test]
    fn algorithm_kind_query_is_indexed_and_excludes_unrelated_live_policies(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,kind,status) in [
            ("policy-a","memory.importance","active"),
            ("policy-b","strategy.selection","active"),
            ("policy-c","memory.importance","disabled"),
        ] {
            store.put("algorithm",&json!({
                "id":id,"kind":kind,"status":status,
                "implementation":"fixed",
            })).unwrap();
        }
        let queried=store.algorithms_for_kind("memory.importance").unwrap();
        assert_eq!(queried.len(),2);
        assert!(queried.iter().all(|row|row["kind"]=="memory.importance"));
        assert!(store.algorithms_for_kind("missing").unwrap().is_empty());
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            // With only three test rows SQLite legitimately prefers the
            // primary index. Force the partial expression index here to
            // verify its existence and usability; production retains the
            // query planner's cost-based choice.
            "EXPLAIN QUERY PLAN SELECT data FROM records INDEXED BY idx_algorithm_kind
             WHERE kind='algorithm'
                AND json_extract(data,'$.kind')='memory.importance' ORDER BY id",
            [],|row|row.get(3),
        ).unwrap();
        assert!(plan.contains("idx_algorithm_kind"),"{plan}");
    }

    #[test]
    fn bounded_provenance_query_observes_only_requested_run_and_order(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,run,seq) in [
            ("r1","run-a",1),("r2","run-a",2),
            ("noise","run-b",700),("r3","run-a",3)
        ] {
            store.put("execution_operation",&json!({
                "id":id,"runId":run,"opSeq":seq,
                "status":"completed","tool":"workspace_read"
            })).unwrap();
        }
        let rows=store.recent_execution_operations("run-a",2).unwrap();
        assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["id"],"r2");
        assert_eq!(rows[1]["id"],"r3");
        assert_eq!(store.recent_execution_operations("run-b",2).unwrap().len(),1);
        let conn=store.conn.lock().unwrap();
        let detail:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.runId')='run-a'
             ORDER BY CAST(json_extract(data,'$.opSeq') AS INTEGER) DESC,id DESC LIMIT 2",
            [],|row|row.get(3)
        ).unwrap();
        assert!(detail.contains("idx_operation_run"),"{detail}");
    }

    #[test]
    fn recovery_queries_keep_unknown_tool_effects_scoped_to_owning_session(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("job",&json!({"id":"live","status":"running","queuedAt":"2026-11-17T00:00:00Z"}));
        store.put("job",&json!({"id":"done","status":"completed"}));
        assert_eq!(store.active_jobs().unwrap().len(),1);
        store.put("execution",&json!({"id":"run-a","sessionId":"s-a","status":"interrupted"}));
        store.put("execution",&json!({"id":"run-b","sessionId":"s-b","status":"running"}));
        store.put("execution_operation",&json!({"id":"tool-a","runId":"run-a",
            "status":"interrupted","outcomeUnknown":true}));
        store.put("execution_operation",&json!({"id":"tool-b","sessionId":"s-b",
            "status":"running","effectState":"started"}));
        store.put("execution_operation",&json!({"id":"safe","sessionId":"s-safe",
            "status":"completed","effectState":"committed","outcomeUnknown":false}));
        let sessions=store.uncertain_effect_sessions().unwrap();
        assert!(sessions.contains("s-a"));
        assert!(sessions.contains("s-b"));
        assert!(!sessions.contains("s-safe"));
        assert_eq!(store.running_recovery_records("execution").unwrap().len(),1);
        assert_eq!(store.running_recovery_records("execution_operation").unwrap().len(),1);
        assert!(store.running_recovery_records("job").is_err());
    }

    #[test]
    fn recovery_queue_read_does_not_treat_corrupt_json_as_idle(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        {
            let conn=store.conn.lock().unwrap();
            // This partial index validates JSON during INSERT. Disable it
            // only in this corruption fixture so the read path sees damage.
            conn.execute_batch("DROP INDEX idx_job_status").unwrap();
            conn.execute("INSERT INTO records(kind,id,data) VALUES('job','damaged','{bad')",[])
                .unwrap();
        }
        assert!(store.active_jobs().is_err());
    }

    #[test]
    fn scoped_effect_history_preserves_pending_replay_and_resource_inspection() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        let resource="github:example";
        let effect_id="effect-repeat";
        store.put("execution_operation", &json!({
            "id":"pending", "sessionId":"owner", "resourceKey":resource,
            "effectId":effect_id, "effectState":"effect_unknown",
            "observedVersion":"sha256:old"
        })).unwrap();
        for n in 0..320 {
            store.put("execution_operation", &json!({
                "id":format!("unrelated-{n}"), "sessionId":"owner",
                "resourceKey":format!("other-{n}"), "effectId":format!("noise-{n}"),
                "effectState":"completed"
            })).unwrap();
        }
        store.put("execution_operation", &json!({
            "id":"wrong-session", "sessionId":"different",
            "resourceKey":resource, "effectId":effect_id,
            "effectState":"effect_unknown"
        })).unwrap();
        store.put("execution_operation", &json!({
            "id":"same-resource", "sessionId":"owner", "resourceKey":resource,
            "effectId":"different-effect", "effectState":"verified",
            "observedVersion":"sha256:new"
        })).unwrap();
        store.put("execution_operation", &json!({
            "id":"same-effect", "sessionId":"owner", "resourceKey":"unrelated-resource",
            "effectId":effect_id, "effectState":"started"
        })).unwrap();

        let scoped=store.related_effect_operations(
            "owner",Some(resource),Some(effect_id)
        ).unwrap();
        assert_eq!(scoped.len(),3, "{scoped:?}");
        assert!(scoped.iter().any(|op|op["id"]=="pending"));
        assert!(scoped.iter().any(|op|op["id"]=="same-resource"));
        assert!(scoped.iter().any(|op|op["id"]=="same-effect"));
        let all=store.session_operations("owner").unwrap();
        assert_eq!(all.len(),323);
        let expected=all.iter()
            .filter(|op|op["resourceKey"]==resource || op["effectId"]==effect_id)
            .map(|op|op["id"].clone()).collect::<Vec<_>>();
        let actual=scoped.iter().map(|op|op["id"].clone()).collect::<Vec<_>>();
        assert_eq!(actual,expected,"scoped results must retain full-session order");
        let effect=json!({"effectId":effect_id,"gated":true,"tool":"github_api"});
        let scoped_ordered=scoped.iter().rev().cloned().collect::<Vec<_>>();
        let all_ordered=all.iter().rev().cloned().collect::<Vec<_>>();
        assert_eq!(
            crate::engine::effect::unresolved(&effect,&scoped_ordered),
            crate::engine::effect::unresolved(&effect,&all_ordered)
        );
        assert_eq!(
            crate::engine::effect::prepare("owner","github_api",
                &json!({"path":"example","method":"POST"}),&scoped_ordered)
                ["beforeVersion"]["version"],
            crate::engine::effect::prepare("owner","github_api",
                &json!({"path":"example","method":"POST"}),&all_ordered)
                ["beforeVersion"]["version"]
        );
        assert_eq!(
            store.related_effect_operations("owner",None,Some(effect_id))
                .unwrap().len(),2
        );
        assert_eq!(
            store.related_effect_operations("owner",None,None).unwrap().len(),0
        );
        assert_eq!(
            store.related_effect_operations("owner",Some(resource),None)
                .unwrap().len(),2
        );
    }

    #[test]
    fn scoped_effect_history_query_uses_both_partial_indexes() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        let conn=store.conn.lock().unwrap();
        let mut statement=conn.prepare(
            "EXPLAIN QUERY PLAN SELECT data,id,created_at,updated_at FROM (
                SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution_operation'
                  AND json_extract(data,'$.sessionId')=?1
                  AND json_extract(data,'$.resourceKey')=?2
                UNION ALL
                SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution_operation'
                  AND json_extract(data,'$.sessionId')=?1
                  AND json_extract(data,'$.effectId')=?3
                  AND (?2 IS NULL OR json_extract(data,'$.resourceKey') IS NOT ?2)
             ) ORDER BY updated_at DESC,id DESC"
        ).unwrap();
        let details=statement.query_map(params!["owner","resource","effect"],
            |row|row.get::<_,String>(3)).unwrap()
            .collect::<Result<Vec<_>,_>>().unwrap().join("\\n");
        assert!(details.contains("idx_operation_session_resource"),"{details}");
        assert!(details.contains("idx_operation_session_effect"),"{details}");
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
