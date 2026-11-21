use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod sdk_sources;
mod room_cursor;
mod room_events;
mod team_dag;

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
    /// Count records of any kind using the shared primary index, without
    /// materializing their JSON. This generic SDK helper must not depend on
    /// a removed optional feature module.
    pub fn count_kind(&self, kind: &str) -> usize {
        let Ok(conn) = self.conn.lock() else { return 0; };
        conn.query_row("SELECT COUNT(*) FROM records WHERE kind=?1", [kind],
            |row| row.get::<_, i64>(0))
            .map(|n| n.max(0) as usize).unwrap_or(0)
    }

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
             -- Append-only RoomEvent facts with room-local sequence. This
             -- is distinct from the global event_log SSE cursor.
             CREATE TABLE IF NOT EXISTS room_events(
                 room_id TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 event_id TEXT NOT NULL,
                 author TEXT NOT NULL,
                 reply_to TEXT,
                 origin_run TEXT,
                 kind TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now')),
                 PRIMARY KEY(room_id,seq),
                 UNIQUE(room_id,event_id)
             );
             CREATE INDEX IF NOT EXISTS idx_room_event_delivery
                 ON room_events(room_id,seq);
             -- Per-room participant progress is independent of the shared
             -- transcript and cannot be inferred from model output.
             CREATE TABLE IF NOT EXISTS room_agent_read_cursors(
                 room_id TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 agent_id TEXT NOT NULL,
                 source_message_id TEXT NOT NULL,
                 reply_message_id TEXT NOT NULL,
                 source_rowid INTEGER NOT NULL,
                 reply_rowid INTEGER NOT NULL,
                 selected_message_ids TEXT NOT NULL DEFAULT '[]',
                 updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                 PRIMARY KEY(room_id,session_id,agent_id)
             );
             CREATE INDEX IF NOT EXISTS idx_room_agent_cursor_session
                 ON room_agent_read_cursors(room_id,session_id,source_rowid);
             -- Structured, receipt-derived room collaboration. Not a chat copy.
             CREATE TABLE IF NOT EXISTS room_work_receipts(
                 message_id TEXT PRIMARY KEY,
                 room_id TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 source_message_id TEXT NOT NULL,
                 agent_id TEXT NOT NULL,
                 execution_id TEXT NOT NULL,
                 summary TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE INDEX IF NOT EXISTS idx_room_work_source
                 ON room_work_receipts(room_id,session_id,source_message_id,created_at DESC,message_id DESC);
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
             CREATE INDEX IF NOT EXISTS idx_job_active_status
                 ON records(kind, json_extract(data,'$.status'), json_extract(data,'$.queuedAt'))
                 WHERE kind='job';
             CREATE INDEX IF NOT EXISTS idx_team_parent_jobs
                 ON records(json_extract(data,'$.input.parentSessionId'), updated_at DESC)
                 WHERE kind='job';
             CREATE INDEX IF NOT EXISTS idx_records_kind_updated
                 ON records(kind, updated_at DESC);
             CREATE INDEX IF NOT EXISTS idx_operation_run
                 ON records(json_extract(data,'$.runId'))
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_algorithm_usage_execution
                 ON records(json_extract(data,'$.executionId'))
                 WHERE kind='algorithm_usage';
             CREATE INDEX IF NOT EXISTS idx_aux_model_call_execution
                 ON records(json_extract(data,'$.executionId'))
                 WHERE kind='aux_model_call';
             CREATE INDEX IF NOT EXISTS idx_algorithm_kind
                 ON records(json_extract(data,'$.kind'))
                 WHERE kind='algorithm';
             DROP INDEX IF EXISTS idx_improvement_active_scope;
             CREATE INDEX IF NOT EXISTS idx_improvement_active_v2
                 ON records(kind, updated_at DESC,
                            json_extract(data,'$.partitionKey'),
                            json_extract(data,'$.scope'),
                            json_extract(data,'$.agentId'))
                 WHERE kind='improvement'
                   AND json_extract(data,'$.status') IN ('adopted','validated')
                   AND json_extract(data,'$.kind') IN ('strategy','tool_policy');
             CREATE INDEX IF NOT EXISTS idx_execution_task
                 ON records(json_extract(data,'$.taskId'))
                 WHERE kind='execution';
             CREATE INDEX IF NOT EXISTS idx_approval_pending
                 ON records(kind, json_extract(data,'$.status'))
                 WHERE kind='approval';
             CREATE INDEX IF NOT EXISTS idx_approval_resume
                 ON records(json_extract(data,'$.sessionId'),
                            json_extract(data,'$.taskId'),
                            json_extract(data,'$.taskRevision'),
                            json_extract(data,'$.operationKey'))
                 WHERE kind='approval'
                   AND json_extract(data,'$.status')='approved'
                   AND json_extract(data,'$.orphaned')=1;
             CREATE INDEX IF NOT EXISTS idx_automation_enabled_due
                 ON records(kind, json_extract(data,'$.enabled'), json_extract(data,'$.nextAt'))
                 WHERE kind='automation';
             CREATE INDEX IF NOT EXISTS idx_running_execution
                 ON records(json_extract(data,'$.status'))
                 WHERE kind='execution';
             CREATE INDEX IF NOT EXISTS idx_active_execution_session
                 ON records(json_extract(data,'$.sessionId'))
                 WHERE kind='execution'
                   AND json_extract(data,'$.status')='running';
             CREATE INDEX IF NOT EXISTS idx_running_operation
                 ON records(json_extract(data,'$.status'))
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_running_external_agent
                 ON records(json_extract(data,'$.status'))
                 WHERE kind='external_agent_run';
             CREATE INDEX IF NOT EXISTS idx_remote_job_queued
                 ON records(created_at ASC,id ASC)
                 WHERE kind='remote_job' AND json_extract(data,'$.status')='queued';
             CREATE INDEX IF NOT EXISTS idx_sdk_execution_session
                 ON records(json_extract(data,'$.sessionId'),json_extract(data,'$.startedAt') DESC)
                 WHERE kind='execution';
             CREATE INDEX IF NOT EXISTS idx_sdk_run_session
                 ON records(json_extract(data,'$.sessionId'))
                 WHERE kind='sdk_run';
             CREATE INDEX IF NOT EXISTS idx_sdk_run_source
                 ON records(json_extract(data,'$.source.kind'),created_at DESC)
                 WHERE kind='sdk_run';
             CREATE INDEX IF NOT EXISTS idx_delegated_session_parent
                 ON records(json_extract(data,'$.parentSessionId'))
                 WHERE kind='session' AND json_extract(data,'$.delegated')=1;
             CREATE INDEX IF NOT EXISTS idx_sdk_native_frame_execution
                 ON records(json_extract(data,'$.parentExecutionId'))
                 WHERE kind='native_frame';
             CREATE INDEX IF NOT EXISTS idx_operation_session
                 ON records(json_extract(data,'$.sessionId'), updated_at DESC)
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_completed_operation_session_recent
                 ON records(json_extract(data,'$.sessionId'), updated_at DESC, id DESC)
                 WHERE kind='execution_operation'
                   AND json_extract(data,'$.status')='completed';
             CREATE INDEX IF NOT EXISTS idx_operation_resource
                 ON records(json_extract(data,'$.resourceKey'))
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_sdk_unknown_operations
                 ON records(json_extract(data,'$.runId'))
                 WHERE kind='execution_operation'
                   AND json_extract(data,'$.outcomeUnknown')=1;
             CREATE INDEX IF NOT EXISTS idx_sdk_pending_approval_session
                 ON records(json_extract(data,'$.sessionId'))
                 WHERE kind='approval' AND json_extract(data,'$.status')='pending';
             CREATE INDEX IF NOT EXISTS idx_records_kind_created
                 ON records(kind, created_at DESC);
",
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

    /// Remote queue discovery is indexed by immutable creation order and
    /// excludes historical reports; the per-job patch_if is the lease CAS.
    pub fn queued_remote_jobs(&self)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='remote_job' AND json_extract(data,'$.status')='queued'
             ORDER BY created_at ASC,id ASC"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Only current work reaches dispatch; historical payloads stay in SQLite.
    /// UI status polling counts live jobs through the indexed status column,
    /// without repeatedly parsing years of historical job JSON.
    pub fn active_job_count(&self) -> Result<usize, String> {
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let count=conn.query_row(
            "SELECT COUNT(*) FROM records
             WHERE kind='job'
               AND json_extract(data,'$.status') IN ('queued','running')",
            [],|row|row.get::<_,i64>(0)
        ).map_err(|error|error.to_string())?;
        Ok(count.max(0) as usize)
    }

    /// Session cancellation must not materialize the global execution
    /// archive. The existing session index bounds the active-run probe.
    pub fn has_running_execution_for_session(&self, session_id: &str) -> Result<bool, String> {
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let running=conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM records
             WHERE kind='execution'
               AND json_extract(data,'$.sessionId')=?1
               AND json_extract(data,'$.status')='running'
             LIMIT 1)",
            [session_id],|row|row.get::<_,i64>(0),
        ).map_err(|error|error.to_string())?;
        Ok(running!=0)
    }

    pub fn active_jobs(&self) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare("SELECT data,id,created_at,updated_at FROM records
            WHERE kind='job' AND json_extract(data,'$.status') IN ('queued','running')
            ORDER BY json_extract(data,'$.queuedAt'),id").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Delegate cancellation must not parse all historical sessions.
    /// Filter child relationships in SQLite with the partial parent index.
    pub fn delegated_child_sessions(&self, parent_session_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn
            .prepare(
                "SELECT data,id,created_at,updated_at FROM records
                 WHERE kind='session' AND json_extract(data,'$.delegated')=1
                   AND json_extract(data,'$.parentSessionId')=?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([parent_session_id], record_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// Recovery only deserializes work that was still running at restart.
    pub fn running_recovery_records(&self, kind: &str) -> Result<Vec<Value>, String> {
        let sql = match kind {
            "execution" => "SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution' AND json_extract(data,'$.status')='running'",
            "execution_operation" => "SELECT data,id,created_at,updated_at FROM records
                WHERE kind='execution_operation' AND json_extract(data,'$.status')='running'",
            "external_agent_run" => "SELECT data,id,created_at,updated_at FROM records
                WHERE kind='external_agent_run' AND json_extract(data,'$.status')='running'",
            _ => return Err(format!("unsupported recovery kind: {kind}")),
        };
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut stmt = conn.prepare(sql).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], record_from_row).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// A single SQL join replaces N per-job scans of the complete tool log.
    /// A tool left in flight is uncertain even before outcomeUnknown is saved.
    pub fn uncertain_effect_sessions(&self) -> Result<std::collections::HashSet<String>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT json_extract(ex.data,'$.sessionId')
             FROM records AS op JOIN records AS ex
               ON ex.kind='execution' AND ex.id=json_extract(op.data,'$.runId')
             WHERE op.kind='execution_operation'
               AND (json_extract(op.data,'$.outcomeUnknown')=1
                    OR json_extract(op.data,'$.status')='running')
               AND json_extract(ex.data,'$.sessionId') IS NOT NULL",
        ).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<std::collections::HashSet<_>, _>>()
            .map_err(|error| error.to_string())
    }

    /// Frequent single-SDK-run polls must not load other sessions' history.
    pub fn sdk_evidence_for_session(&self, session: &str)
        -> Result<(Vec<Value>,Vec<Value>,Vec<Value>),String> {
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let mut execution=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution' AND json_extract(data,'$.sessionId')=?1
             ORDER BY json_extract(data,'$.startedAt') DESC,id DESC LIMIT 1",
        ).map_err(|error|error.to_string())?;
        let runs=execution.query_map([session],record_from_row)
            .map_err(|error|error.to_string())?
            .collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())?;
        let mut approvals=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='approval' AND json_extract(data,'$.sessionId')=?1
               AND json_extract(data,'$.status')='pending'
             ORDER BY updated_at DESC,id DESC",
        ).map_err(|error|error.to_string())?;
        let pending=approvals.query_map([session],record_from_row)
            .map_err(|error|error.to_string())?
            .collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())?;
        let Some(run_id)=runs.first().and_then(|run|run["id"].as_str()) else{
            return Ok((runs,pending,Vec::new()));
        };
        let mut effects=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.runId')=?1
               AND json_extract(data,'$.outcomeUnknown')=1",
        ).map_err(|error|error.to_string())?;
        let unknown=effects.query_map([run_id],record_from_row)
            .map_err(|error|error.to_string())?
            .collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())?;
        Ok((runs,pending,unknown))
    }

    /// SDK status polling is scoped to sessions actually owned by SDK runs.
    /// The latest execution is selected in SQLite, so a 1.5-second UI refresh
    /// does not deserialize the whole execution history and tool log.
    pub fn sdk_evidence_rows(&self) -> Result<(Vec<Value>, Vec<Value>), String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut latest = conn.prepare(
            "SELECT e.data,e.id,e.created_at,e.updated_at FROM records AS sdk
             JOIN records AS e ON e.kind='execution' AND e.id=(
               SELECT current.id FROM records AS current
               WHERE current.kind='execution'
                 AND json_extract(current.data,'$.sessionId')=json_extract(sdk.data,'$.sessionId')
               ORDER BY json_extract(current.data,'$.startedAt') DESC,current.id DESC LIMIT 1
             )
             WHERE sdk.kind='sdk_run'
               AND json_type(sdk.data,'$.sessionId')='text'",
        ).map_err(|error| error.to_string())?;
        let executions = latest.query_map([], record_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
        let mut unknown = conn.prepare(
            "SELECT op.data,op.id,op.created_at,op.updated_at
             FROM records AS op
             JOIN records AS execution
               ON execution.kind='execution'
               AND execution.id=json_extract(op.data,'$.runId')
             JOIN records AS sdk
               ON sdk.kind='sdk_run'
               AND json_extract(sdk.data,'$.sessionId')=json_extract(execution.data,'$.sessionId')
             WHERE op.kind='execution_operation'
               AND json_extract(op.data,'$.outcomeUnknown')=1",
        ).map_err(|error| error.to_string())?;
        let operations = unknown.query_map([], record_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
        Ok((executions, operations))
    }

    /// Algorithm dispatch is a hot path (every tool, plan, memory call).
    /// Fetch only the requested domain's records rather than decoding every
    /// registered or historical algorithm on every decision.
    pub fn algorithms_for_kind(&self, kind: &str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='algorithm' AND json_extract(data,'$.kind')=?1
             ORDER BY id"
        ).map_err(|error|error.to_string())?;
        let rows=stmt.query_map([kind],record_from_row)
            .map_err(|error|error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())
    }

    /// Legacy improvement migration processes only rows that still need
    /// compatibility fields. Repeated launches must not decode/rewrite the
    /// entire historical improvement archive.
    pub fn legacy_improvements_to_migrate(&self) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='improvement'
               AND (
                   COALESCE(json_extract(data,'$.scoreVersion'),0)<1
                   OR (json_extract(data,'$.scoreVersion')=1
                       AND (COALESCE(json_type(data,'$.legacyCounters'),'')!='object'
                            OR json_type(data,'$.evidenceGrade') IS NULL))
               )
             ORDER BY id"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Fetch only this run's algorithm usage receipts. Finalization must not
    /// deserialize every historical turn to rate a single execution.
    pub fn algorithm_usages_for_execution(&self, execution_id: &str)
        -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='algorithm_usage'
               AND json_extract(data,'$.executionId')=?1
             ORDER BY id"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([execution_id],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// An approval retry can inspect only *the same owner/task/operation*
    /// candidate decisions, never all historical approvals or executions.
    /// The caller still checks the original execution's interrupted state and
    /// consumes a matching decision exactly once through patch_if.
    pub fn reusable_approvals_for_lineage(
        &self, session_id: &str, task_id: &str, revision: &str,
        operation_key: &str, tool: &str, args_digest: &str,
    ) -> Result<Vec<Value>, String> {
        if task_id.is_empty() || revision.is_empty() || operation_key.is_empty() {
            return Ok(Vec::new());
        }
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='approval'
               AND json_extract(data,'$.sessionId')=?1
               AND json_extract(data,'$.taskId')=?2
               AND json_extract(data,'$.taskRevision')=?3
               AND json_extract(data,'$.operationKey')=?4
               AND json_extract(data,'$.tool')=?5
               AND json_extract(data,'$.argsDigest')=?6
               AND json_extract(data,'$.status')='approved'
               AND json_extract(data,'$.orphaned')=1
               AND COALESCE(json_extract(data,'$.consumed'),0)=0
             ORDER BY updated_at DESC LIMIT 32"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(
            params![session_id,task_id,revision,operation_key,tool,args_digest],
            record_from_row,
        ).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Read only unresolved approvals; old decisions must not be decoded on every idle tick.
    pub fn pending_approvals(&self) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records
             WHERE kind='approval' AND json_extract(data,'$.status')='pending'",
        ).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], record_from_row).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// SQLite filters the due queue by actual timestamps; legacy time-zone offsets stay valid.
    /// Disabled and future automations are not deserialized by the periodic dispatcher.

    /// Indexed existence probe without loading or parsing historical receipts.
    /// Errors are returned to the caller so budget release can fail closed.
    pub fn has_unknown_execution_effect(&self, run_id: &str) -> Result<bool, String> {
        let conn=self.conn.lock().map_err(|error|error.to_string())?;
        let found=conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM records
                 WHERE kind='execution_operation'
                   AND json_extract(data,'$.runId')=?1
                   AND json_extract(data,'$.outcomeUnknown')=1
             )",
            [run_id],|row|row.get::<_,i64>(0)
        ).map_err(|error|error.to_string())?;
        Ok(found!=0)
    }

    /// Task-scoped recovery reads use idx_execution_task rather than
    /// deserializing every historical execution from unrelated tasks.
    pub fn executions_for_task(&self, task_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution' AND json_extract(data,'$.taskId')=?1
             ORDER BY json_extract(data,'$.startedAt') DESC,id DESC",
        ).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([task_id], record_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// Append one immutable evidence summary for a completed room agent turn.
    /// The message ID is the idempotency key across worker restarts. No model
    /// output or raw tool payload is stored in this structured board.
    pub fn publish_room_work_receipt(&self, value:&Value) -> Result<bool,String> {
        let field=|name:&str| -> Result<&str,String> {
            let v=value[name].as_str().ok_or_else(||format!("room board missing {name}"))?;
            if v.trim().is_empty()||v.len()>256{return Err(format!("invalid room board {name}"));}
            Ok(v)
        };
        let room=field("roomId")?;
        let session=field("sessionId")?;
        let source=field("sourceMessageId")?;
        let message=field("messageId")?;
        let agent=field("agentId")?;
        let execution=field("executionId")?;
        let summary=value["summary"].as_object()
            .ok_or("room board summary must be an object")?;
        let receipts=summary.get("receipts").and_then(Value::as_array)
            .ok_or("room board receipts must be an array")?;
        if receipts.is_empty()||receipts.len()>12 {return Err("room board receipt count out of bounds".into());}
        let serialized=Value::Object(summary.clone()).to_string();
        if serialized.len()>8_192{return Err("room board summary exceeds 8192 bytes".into());}
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let changed=conn.execute(
            "INSERT INTO room_work_receipts(
                room_id,session_id,source_message_id,message_id,agent_id,execution_id,summary
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(message_id) DO NOTHING",
            params![room,session,source,message,agent,execution,serialized]
        ).map_err(|e|e.to_string())?;
        Ok(changed==1)
    }

    /// Read the current owner's work board with an indexed, bounded query.
    /// It cannot retrieve another room, session, owner message or in-flight
    /// operation, even if a peer injects an unrelated identifier into text.
    pub fn room_work_receipts(
        &self, room:&str,session:&str,source:&str,limit:usize
    )->Result<Vec<Value>,String>{
        if limit==0{return Ok(Vec::new());}
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT message_id,agent_id,execution_id,summary
               FROM room_work_receipts
              WHERE room_id=?1 AND session_id=?2 AND source_message_id=?3
              ORDER BY created_at DESC,message_id DESC LIMIT ?4"
        ).map_err(|e|e.to_string())?;
        let values=stmt.query_map(params![room,session,source,limit.min(16) as i64],|row|{
            let summary:String=row.get(3)?;
            Ok(json!({"messageId":row.get::<_,String>(0)?,
                "agentId":row.get::<_,String>(1)?,
                "executionId":row.get::<_,String>(2)?,
                "summary":serde_json::from_str::<Value>(&summary).unwrap_or(Value::Null)}))
        }).map_err(|e|e.to_string())?;
        values.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Repeated effect checks must not parse the entire historical tool journal.
    /// Sorted newest-first to retain the previous all(kind) + reverse semantics.
    pub fn session_execution_operations(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.sessionId')=?1
             ORDER BY updated_at DESC"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([session_id],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Child task_results use the most recent successful tool receipts only.
    /// Applying LIMIT in SQLite prevents each task poll from deserializing
    /// every tool ever executed by this agent or even this child session.
    pub fn recent_completed_session_operations(
        &self, session_id: &str, limit: usize,
    ) -> Result<Vec<Value>, String> {
        if limit == 0 { return Ok(Vec::new()); }
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.status')='completed'
               AND json_extract(data,'$.sessionId')=?1
             ORDER BY updated_at DESC,id DESC LIMIT ?2",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map(
            params![session_id, limit.min(20) as i64], record_from_row
        ).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error| error.to_string())
    }

    pub fn resource_execution_operations(&self, resource: &str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.resourceKey')=?1"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([resource],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// SDK cumulative model-call budgets count only this session's prior runs.
    pub fn sdk_used_model_calls(&self, session: &str, excluded_run: &str) -> Result<u64, String> {
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let used=conn.query_row(
            "SELECT COALESCE(SUM(
                CASE WHEN json_type(data,'$.modelCalls')='integer'
                THEN MAX(CAST(json_extract(data,'$.modelCalls') AS INTEGER),0)
                ELSE 0 END),0)
             FROM records WHERE kind='execution'
               AND json_extract(data,'$.sessionId')=?1 AND id<>?2",
            rusqlite::params![session,excluded_run],
            |row|row.get::<_,i64>(0)
        ).map_err(|e|e.to_string())?;
        Ok(used.max(0) as u64)
    }

    /// Only this execution's auxiliary model receipts, including retries.
    /// An unknown/in-flight receipt is returned, never treated as zero usage.
    pub fn auxiliary_model_calls(&self, execution_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records
             WHERE kind='aux_model_call'
               AND json_extract(data,'$.executionId')=?1
             ORDER BY created_at ASC, id",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map([execution_id], record_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error|error.to_string())
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

    /// Fetch only the specific records attached to a completed execution.
    /// This preserves SQL errors instead of converting them into an empty
    /// result, and uses the existing (kind, id) primary key for each lookup.
    /// Historical improvement/operation archives are never materialized.
    pub fn try_records_by_ids(&self, kind: &str, ids: &[String]) -> Result<Vec<Value>, String> {
        if ids.is_empty() { return Ok(Vec::new()); }
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 AND id=?2",
        ).map_err(|error| error.to_string())?;
        let mut found = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            if !seen.insert(id.as_str()) { continue; }
            if let Some(value) = statement
                .query_row(params![kind, id], record_from_row)
                .optional()
                .map_err(|error| error.to_string())?
            {
                found.push(value);
            }
        }
        Ok(found)
    }

    /// Newest execution facts, with deterministic same-second ordering.
    /// Uses idx_records_kind_updated rather than reading the complete archive.
    pub fn try_recent_records(&self, kind: &str, limit: usize) -> Result<Vec<Value>, String> {
        if limit == 0 { return Ok(Vec::new()); }
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind=?1 ORDER BY updated_at DESC,rowid DESC LIMIT ?2",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map(
            params![kind, limit.min(1_000) as i64], record_from_row
        ).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// Select only relevant, live strategy/tool-policy records for this agent.
    /// Historic, rejected and unrelated agent revisions never reach the
    /// per-turn selection algorithm or consume prompt/context preparation.
    pub fn active_improvements_for_scope(
        &self, partition_key: &str, agent_id: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='improvement'
               AND json_extract(data,'$.status') IN ('adopted','validated')
               AND json_extract(data,'$.kind') IN ('strategy','tool_policy')
               AND (
                 json_extract(data,'$.scope')='global'
                 OR (json_extract(data,'$.partitionKey')=?1
                     AND (json_extract(data,'$.scope')='shared'
                          OR json_extract(data,'$.agentId') IS ?2))
               )
             ORDER BY updated_at DESC,rowid DESC",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map(
            params![partition_key, agent_id], record_from_row
        ).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
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

    /// Persist a manual observation and its native execution state together.
    /// No resume can see a resolved effect with an unresolved checkpoint.
    pub fn put_native_observation(&self, records: &[(&str, Value)]) -> Result<(), String> {
        let mut conn=self.conn.lock().unwrap();
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        for (kind,value) in records {upsert_tx(&tx,kind,value)?;}
        tx.commit().map_err(|e|e.to_string())
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

    /// Reconcile an uncertain external effect exactly once. Both the SDK
    /// owner's status and the operation's unknown-effect bit are checked
    /// under the same SQLite transaction before any checkpoint is rewritten.
    pub fn reconcile_sdk_effect_once(
        &self,sdk_id:&str,expected_status:&str,operation_id:&str,run_id:&str,
        records:&[(String,Value)],
    )->Result<bool,String>{
        let mut conn=self.conn.lock().map_err(|error|error.to_string())?;
        let tx=conn.transaction().map_err(|error|error.to_string())?;
        let owner=tx.query_row(
            "SELECT json_extract(data,'$.status') FROM records
             WHERE kind='sdk_run' AND id=?1",
            [sdk_id],|row|row.get::<_,String>(0),
        ).optional().map_err(|error|error.to_string())?;
        if owner.as_deref()!=Some(expected_status){return Ok(false);}
        let unknown=tx.query_row(
            "SELECT 1 FROM records
             WHERE kind='execution_operation' AND id=?1
               AND json_extract(data,'$.runId')=?2
               AND json_extract(data,'$.outcomeUnknown')=1",
            params![operation_id,run_id],|row|row.get::<_,i64>(0),
        ).optional().map_err(|error|error.to_string())?;
        if unknown!=Some(1){return Ok(false);}
        for (kind,value) in records{upsert_tx(&tx,kind,value)?;}
        tx.commit().map_err(|error|error.to_string())?;
        Ok(true)
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

    /// Read/modify one durable aggregate and optional new records under the
    /// same SQLite transaction. The callback must not re-enter this Store.
    pub fn mutate_record<F>(&self,kind:&str,id:&str,initial:Value,mutate:F)->Result<Value,String>
    where F:FnOnce(&mut Value)->Result<Vec<(String,Value)>,String> {
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row("SELECT data FROM records WHERE kind=?1 AND id=?2",
            params![kind,id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
        let mut value=match raw {Some(raw)=>serde_json::from_str(&raw).map_err(|e|e.to_string())?,None=>initial};
        if value["id"].as_str()!=Some(id){return Err("원자적 갱신 기록의 식별자가 다릅니다.".into());}
        let extra=mutate(&mut value)?;
        upsert_tx(&tx,kind,&value)?;
        for (kind,record) in extra {upsert_tx(&tx,&kind,&record)?;}
        tx.commit().map_err(|e|e.to_string())?;
        Ok(value)
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
    pub fn patch_if(
        &self,
        kind: &str,
        id: &str,
        field: &str,
        expected: &Value,
        patch: &Value,
    ) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let mut record = conn
            .query_row(
                "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                record_from_row,
            )
            .ok()?;
        // Reject stale lifecycle writes after a concurrent terminal update.
        // This check and the phase CAS share the same connection lock.
        if kind == "execution" && field == "phase"
            && matches!(record["status"].as_str(), Some("completed" | "failed" | "cancelled" | "interrupted"))
        {
            return None;
        }
        if record.get(field).unwrap_or(&Value::Null) != expected {
            return None;
        }
        for (key, value) in patch.as_object()? {
            if key != "id" {
                record[key] = value.clone();
            }
        }
        conn.execute(
            "UPDATE records SET data=?3, updated_at=datetime('now') WHERE kind=?1 AND id=?2",
            params![kind, id, record.to_string()],
        )
        .ok()?;
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
        let mut conn = self.conn.lock().unwrap();
        let tx=conn.transaction().ok()?;
        let id=insert_message(&tx,message)?;
        let saved=query_message(&tx,&id)?;
        tx.commit().ok()?;
        Some(saved)
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

    /// Read only the final `limit` message tool-call columns for lazy room
    /// catalog selection. Never materialize a long session's full transcript
    /// merely to recover the tool names used in its last few turns.
    pub fn recent_message_tool_names(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<HashSet<String>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut statement = conn
            .prepare(
                "SELECT tool_calls FROM messages WHERE session_id=?1
                 ORDER BY created_at DESC,rowid DESC LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![session_id, limit.min(30) as i64], |row| {
                row.get::<_, Option<String>>(0)
            })
            .map_err(|error| error.to_string())?;
        let mut names = HashSet::new();
        for row in rows {
            let Some(raw) = row.map_err(|error| error.to_string())? else {
                continue;
            };
            let Ok(calls) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            for call in calls.as_array().into_iter().flatten() {
                if let Some(name) = call["function"]["name"].as_str() {
                    names.insert(name.to_lowercase());
                }
            }
        }
        Ok(names)
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

    pub fn append_event(&self, seq: u64, value: &Value) -> Result<(),String> {
        let conn = self.conn.lock().unwrap();
        let seq=i64::try_from(seq).map_err(|_|"event sequence overflow".to_string())?;
        conn.execute(
            "INSERT INTO event_log(seq, data) VALUES(?1, ?2)",
            params![seq, value.to_string()],
        ).map_err(|e|e.to_string())?;
        if seq % 256 == 0 {
            let cutoff = seq.saturating_sub(2_000) as i64;
            if cutoff > 0 {
                let _ = conn.execute(
                    "DELETE FROM event_log WHERE seq <= ?1",
                    params![cutoff],
                );
            }
        }
        Ok(())
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


    /// Indexed by the immutable native execution ID, not SDK/room/job IDs.
    /// SQLite JSON extraction avoids treating unrelated global events as a run.
    pub fn events_for_execution(&self, execution_id:&str, after:u64, limit:usize)->Vec<Value>{
        let conn=self.conn.lock().unwrap();
        let Ok(mut stmt)=conn.prepare(
            "SELECT seq,data FROM event_log WHERE seq > ?1 AND json_extract(data, '$.runRef.executionId') = ?2 ORDER BY seq ASC LIMIT ?3"
        )else{return Vec::new()};
        let rows=stmt.query_map(
            params![after.min(i64::MAX as u64) as i64,execution_id,limit.min(501) as i64],
            |row|{
                let seq:i64=row.get(0)?;
                let data:String=row.get(1)?;
                Ok((seq,data))
            }
        );
        let Ok(rows)=rows else{return Vec::new()};
        rows.filter_map(|row|{
            let (seq,data)=row.ok()?;
            let mut value:Value=serde_json::from_str(&data).ok()?;
            value["seq"]=json!(seq as u64);
            Some(value)
        }).collect()
    }

    /// Earliest surviving global event for explicit retention-gap reporting.
    pub fn earliest_event_seq(&self)->Option<u64>{
        let conn=self.conn.lock().unwrap();
        conn.query_row("SELECT MIN(seq) FROM event_log",[],|row|row.get::<_,Option<i64>>(0))
            .unwrap_or(None).map(|seq|seq.max(0) as u64)
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
                    COALESCE(SUM(CAST(json_extract(detail,'$.cachedPromptTokens') AS INTEGER)),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.cacheCreationTokens') AS INTEGER)),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens')='integer' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_extract(detail,'$.promptTokens') IS NOT NULL
                        AND COALESCE(json_type(detail,'$.cachedPromptTokens'),'null')!='integer' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN json_type(detail,'$.cacheCreationTokens')='integer' THEN 1 ELSE 0 END),0)
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
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        );
        match summary {
            Ok((calls, prompt, completion, total, estimated, cached, cache_writes, reported, unknown, write_reported)) => {
                let known=reported>0 && unknown==0;
                json!({
                    "calls":calls.max(0),
                    "promptTokens":prompt.max(0),
                    "completionTokens":completion.max(0),
                    "totalTokens":total.max(0),
                    "estimatedCalls":estimated.max(0),
                    "cacheReportedCalls":reported.max(0),
                    "cacheUnknownCalls":unknown.max(0),
                    "reportedCachedPromptTokens":cached.max(0),
                    "reportedCacheCreationTokens":cache_writes.max(0),
                    "cachedPromptTokens":known.then_some(cached.max(0)),
                    "uncachedPromptTokens":known.then_some(prompt.max(0).saturating_sub(cached.max(0))),
                    "cacheCreationTokens":(write_reported>0).then_some(cache_writes.max(0)),
                })
            },
            Err(_) => json!({
                "calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,
                "estimatedCalls":0,"cacheReportedCalls":0,"cacheUnknownCalls":0,
                "reportedCachedPromptTokens":0,"reportedCacheCreationTokens":0,
                "cachedPromptTokens":null,"uncachedPromptTokens":null,
                "cacheCreationTokens":null,
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
    // Only actual owner messages in a room-bound transcript create a root
    // room event. This runs in the caller's message/job SQLite transaction.
    if role=="user"&&metadata["authorKind"]=="owner" {
        let room=conn.query_row(
            "SELECT json_extract(data,'$.roomId') FROM records
             WHERE kind='session' AND id=?1",
            [session_id],|row|row.get::<_,Option<String>>(0)
        ).optional().ok()?.flatten();
        if let Some(room_id)=room {
            room_events::insert_room_event(
                conn,&room_id,session_id,&id,"owner_message"
            ).ok()?;
        }
    }
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
    fn room_work_board_is_immutable_scoped_indexed_and_bounded() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..22 {
            let entry=json!({"roomId":"r","sessionId":"room:r",
                "sourceMessageId":"owner","messageId":format!("m{i:02}"),
                "agentId":"a","executionId":format!("exec{i}"),
                "summary":{"receipts":[{"id":format!("op{i}"),"tool":"workspace_read"}]}});
            assert!(store.publish_room_work_receipt(&entry).unwrap());
            assert!(!store.publish_room_work_receipt(&entry).unwrap());
        }
        let foreign=json!({"roomId":"other","sessionId":"room:other",
            "sourceMessageId":"owner","messageId":"foreign","agentId":"z",
            "executionId":"other-run","summary":{"receipts":[{"id":"secret"}]}});
        store.publish_room_work_receipt(&foreign).unwrap();
        let newer=json!({"roomId":"r","sessionId":"room:r",
            "sourceMessageId":"new-owner","messageId":"later","agentId":"a",
            "executionId":"later-run","summary":{"receipts":[{"id":"later-op"}]}});
        store.publish_room_work_receipt(&newer).unwrap();
        let rows=store.room_work_receipts("r","room:r","owner",7).unwrap();
        assert_eq!(rows.len(),7);
        assert!(rows.iter().all(|v|v["messageId"]!="foreign"&&v["messageId"]!="later"));
        assert!(store.room_work_receipts("other","room:r","owner",10).unwrap().is_empty());
        assert!(store.room_work_receipts("r","room:r","owner",0).unwrap().is_empty());
        let plan:String=store.conn.lock().unwrap().query_row(
            "EXPLAIN QUERY PLAN SELECT message_id,agent_id,execution_id,summary
             FROM room_work_receipts
             WHERE room_id='r' AND session_id='room:r' AND source_message_id='owner'
             ORDER BY created_at DESC,message_id DESC LIMIT 6",
            [],|row|row.get(3)
        ).unwrap();
        assert!(plan.contains("idx_room_work_source"),"{plan}");
        let invalid=json!({"roomId":"r","sessionId":"room:r","sourceMessageId":"owner",
            "messageId":"bad","agentId":"a","executionId":"x","summary":{"receipts":[]}});
        assert!(store.publish_room_work_receipt(&invalid).is_err());
    }

    #[test]
    fn recent_room_tool_lookup_is_indexed_bounded_and_session_scoped() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for index in 0..42 {
            let calls = match index {
                0 => json!([{"function":{"name":"OLD_TOOL"}}]),
                40 => json!([{"function":{"name":"SHELL_EXEC"}}]),
                _ => Value::Null,
            };
            store.write_message(&json!({
                "id": format!("room-{index:03}"),
                "session_id": "room-a",
                "role": "assistant",
                "content": "old transcript content should not be loaded",
                "tool_calls": calls
            })).unwrap();
        }
        store.write_message(&json!({
            "id":"foreign","session_id":"room-b","role":"assistant",
            "tool_calls":[{"function":{"name":"FOREIGN_TOOL"}}]
        })).unwrap();
        let names = store.recent_message_tool_names("room-a", 30).unwrap();
        assert!(names.contains("shell_exec"));
        assert!(!names.contains("old_tool"));
        assert!(!names.contains("foreign_tool"));
        assert!(store.recent_message_tool_names("room-a", 0).unwrap().is_empty());
        let plan: String = store.conn.lock().unwrap().query_row(
            "EXPLAIN QUERY PLAN SELECT tool_calls FROM messages
             WHERE session_id='room-a' ORDER BY created_at DESC,rowid DESC LIMIT 30",
            [], |row| row.get(3)
        ).unwrap();
        assert!(plan.contains("idx_messages_session"), "{plan}");
    }

    #[test]
    fn delegated_child_session_lookup_uses_parent_index() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id, parent, delegated) in [
            ("owned", "root", true),
            ("other", "foreign", true),
            ("manual", "root", false),
        ] {
            store.put("session", &json!({
                "id":id, "parentSessionId":parent, "delegated":delegated
            })).unwrap();
        }
        let children = store.delegated_child_sessions("root").unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["id"], "owned");
        let plan: String = store.conn.lock().unwrap().query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records
             WHERE kind='session' AND json_extract(data,'$.delegated')=1
               AND json_extract(data,'$.parentSessionId')='root'",
            [], |row| row.get(3)
        ).unwrap();
        assert!(plan.contains("idx_delegated_session_parent"), "{plan}");
    }

    #[test]
    fn dispatch_reads_indexed_current_jobs_without_touching_history() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..500 {
            store.put("job",&json!({"id":format!("j{i:03}"),"status":if i==10||i==20 {"queued"}else if i==30 {"running"}else{"completed"},"queuedAt":format!("{i:04}")})).unwrap();
        }
        // A historical, unrelated record may not be JSON; the partial index
        // must never evaluate its payload or break migration/dispatch.
        store.conn.lock().unwrap().execute("INSERT INTO records(kind,id,data) VALUES('legacy','opaque','not json')",[]).unwrap();
        let active=store.active_jobs().unwrap();
        assert_eq!(active.iter().map(|v|v["id"].as_str().unwrap()).collect::<Vec<_>>(),["j010","j020","j030"]);
        assert_eq!(store.active_job_count().unwrap(),3,
            "status polling must exclude historical completed jobs");
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row("EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='job' AND json_extract(data,'$.status') IN ('queued','running')",[],|r|r.get(3)).unwrap();
        assert!(plan.contains("idx_job_active_status"),"{plan}");
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM records WHERE kind='job'",[],|r|r.get::<_,u64>(0)).unwrap(),500);
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
    fn session_cache_ledger_never_turns_unknown_hits_into_zero() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.audit("model.usage",json!({
            "sessionId":"cache-session","providerId":"p","model":"m","purpose":"chat",
            "promptTokens":1000,"completionTokens":50,"totalTokens":1050,
            "cachedPromptTokens":600,"cacheCreationTokens":300,"estimated":false
        }));
        let known=store.session_usage("cache-session");
        assert_eq!(known["calls"],1);
        assert_eq!(known["cachedPromptTokens"],600);
        assert_eq!(known["uncachedPromptTokens"],400);
        assert_eq!(known["cacheCreationTokens"],300);
        assert_eq!(known["cacheReportedCalls"],1);
        assert_eq!(known["cacheUnknownCalls"],0);
        store.audit("model.usage",json!({
            "sessionId":"cache-session","providerId":"p","model":"m","purpose":"chat",
            "promptTokens":500,"completionTokens":20,"totalTokens":520,
            "estimated":false
        }));
        let partial=store.session_usage("cache-session");
        assert_eq!(partial["calls"],2);
        assert_eq!(partial["promptTokens"],1500);
        assert_eq!(partial["cacheUnknownCalls"],1);
        assert_eq!(partial["cachedPromptTokens"],Value::Null,
            "missing provider cache telemetry must never appear as zero");
        let empty=store.session_usage("other-session");
        assert_eq!(empty["cachedPromptTokens"],Value::Null);
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
    fn remote_queue_uses_indexed_fifo_and_skips_closed_remote_history(){
        let root=std::env::temp_dir().join(format!("dominion-remote-index-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store=Store::open(&root.join("state.sqlite")).unwrap();
        store.put("remote_job",&json!({"id":"finished","status":"owner_accepted"})).unwrap();
        store.put("remote_job",&json!({"id":"ready","status":"queued"})).unwrap();
        let rows=store.queued_remote_jobs().unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0]["id"],"ready");
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data,id,created_at,updated_at FROM records
             WHERE kind='remote_job' AND json_extract(data,'$.status')='queued'
             ORDER BY created_at ASC,id ASC",[],|row|row.get(3)).unwrap();
        assert!(plan.contains("idx_remote_job_queued"),"{plan}");
        drop(conn);drop(store);
        let _=std::fs::remove_dir_all(root);
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


    #[test]
    fn external_effect_reconciliation_is_single_claim_and_does_not_reopen_owner(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("sdk_run",&json!({"id":"sdk","status":"interrupted"}));
        store.put("execution_operation",&json!({
            "id":"op","runId":"run","outcomeUnknown":true
        }));
        let records=[
            ("execution_operation".to_string(),json!({
                "id":"op","runId":"run","outcomeUnknown":false,
                "effectState":"applied"
            })),
            ("sdk_run".to_string(),json!({
                "id":"sdk","status":"interrupted","reconciledAt":"now"
            }))
        ];
        assert!(store.reconcile_sdk_effect_once("sdk","interrupted",
            "op","run",&records).unwrap());
        assert!(!store.reconcile_sdk_effect_once("sdk","interrupted",
            "op","run",&records).unwrap(),
            "a second operator cannot overwrite an already observed effect");
        assert_eq!(store.get("execution_operation","op").unwrap()["effectState"],"applied");
        assert!(!store.reconcile_sdk_effect_once("sdk","interrupted",
            "op","wrong-run",&records).unwrap());
    }

    #[test]
    fn indexed_effect_reads_and_sdk_budget_are_isolated(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,session,resource) in [("op-a","s-a","file:a"),("op-b","s-b","file:b"),
            ("op-c","s-a","file:b")] {
            store.put("execution_operation",&json!({"id":id,
                "sessionId":session,"resourceKey":resource,"runId":id}));
        }
        assert_eq!(store.session_execution_operations("s-a").unwrap().len(),2);
        assert_eq!(store.session_execution_operations("s-b").unwrap().len(),1);
        assert_eq!(store.resource_execution_operations("file:b").unwrap().len(),2);
        assert!(store.resource_execution_operations("file:x").unwrap().is_empty());
        for (id,session,calls) in [("e1","s-a",3),("e2","s-a",5),("e3","s-b",90)] {
            store.put("execution",&json!({"id":id,"sessionId":session,"modelCalls":calls}));
        }
        assert_eq!(store.sdk_used_model_calls("s-a","e2").unwrap(),3);
        assert_eq!(store.sdk_used_model_calls("s-a","missing").unwrap(),8);
        assert_eq!(store.sdk_used_model_calls("s-b","missing").unwrap(),90);
    }

    #[test]
    fn session_running_probe_avoids_global_history_and_uses_session_index() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..100 {
            store.put("execution",&json!({"id":format!("old-{i}"),
                "sessionId":"other","status":"completed"})).unwrap();
        }
        store.put("execution",&json!({"id":"active",
            "sessionId":"target","status":"running"})).unwrap();
        assert!(store.has_running_execution_for_session("target").unwrap());
        assert!(!store.has_running_execution_for_session("other").unwrap());
        assert!(!store.has_running_execution_for_session("unknown").unwrap());
        let conn=store.conn.lock().unwrap();
        let plans=conn.prepare(
            "EXPLAIN QUERY PLAN SELECT EXISTS(SELECT 1 FROM records
             WHERE kind='execution'
               AND json_extract(data,'$.sessionId')=?1
               AND json_extract(data,'$.status')='running'
             LIMIT 1)",
        ).unwrap().query_map(["target"],|row|row.get::<_,String>(3))
            .unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert!(plans.iter().any(|line|line.contains("idx_active_execution_session")),
            "session cancellation must use its running-session partial index: {plans:?}");
    }

    #[test]
    fn delegated_results_use_recent_index_without_loading_unrelated_history() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for n in 0..45 {
            store.put("execution_operation",&json!({
                "id":format!("child-{n:03}"),"runId":"child-run",
                "sessionId":"child","status":"completed","opSeq":n,
                "tool":"workspace_read","result":{"content":format!("output {n}")}
            })).unwrap();
            store.put("execution_operation",&json!({
                "id":format!("other-{n:03}"),"runId":"other-run",
                "sessionId":"stranger","status":"completed","opSeq":n,
                "tool":"workspace_read","result":{"content":"not visible"}
            })).unwrap();
        }
        store.put("execution_operation",&json!({
            "id":"failed-999","sessionId":"child","status":"failed",
            "result":{"content":"not successful"}
        })).unwrap();
        let recent=store.recent_completed_session_operations("child",20).unwrap();
        assert_eq!(recent.len(),20);
        assert_eq!(recent[0]["id"],"child-044");
        assert_eq!(recent[19]["id"],"child-025");
        assert!(recent.iter().all(|row|row["sessionId"]=="child"&&row["status"]=="completed"));
        assert!(store.recent_completed_session_operations("child",0).unwrap().is_empty());
        let conn=store.conn.lock().unwrap();
        let plan=conn.prepare(
            "EXPLAIN QUERY PLAN SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution_operation'
               AND json_extract(data,'$.status')='completed'
               AND json_extract(data,'$.sessionId')=?1
             ORDER BY updated_at DESC,id DESC LIMIT ?2"
        ).unwrap().query_map(params!["child",20i64],
            |row|row.get::<_,String>(3)
        ).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert!(plan.iter().any(|step|step.contains("idx_completed_operation_session_recent")),
            "recent child results must use partial indexed reads: {plan:?}");
    }

    #[test]
    fn indexed_unknown_effect_probe_is_run_scoped(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("execution_operation",&json!({"id":"old",
            "runId":"unrelated","outcomeUnknown":true}));
        store.put("execution_operation",&json!({"id":"checked",
            "runId":"target","outcomeUnknown":false}));
        assert!(!store.has_unknown_execution_effect("target").unwrap());
        assert!(store.has_unknown_execution_effect("unrelated").unwrap());
        store.put("execution_operation",&json!({"id":"current",
            "runId":"target","outcomeUnknown":true}));
        assert!(store.has_unknown_execution_effect("target").unwrap());
        store.put("execution_operation",&json!({"id":"current",
            "runId":"target","outcomeUnknown":false}));
        assert!(!store.has_unknown_execution_effect("target").unwrap());
    }

    #[test]
    fn sdk_single_run_projection_reads_only_its_session(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for session in ["a","b"]{
            for (id,at) in [("first","2026-01-01"),("last","2026-11-17")]{
                store.put("execution",&json!({
                    "id":format!("{session}-{id}"),"sessionId":session,"startedAt":at
                }));
            }
            store.put("approval",&json!({"id":format!("{session}-pending"),
                "sessionId":session,"status":"pending"}));
            store.put("approval",&json!({"id":format!("{session}-closed"),
                "sessionId":session,"status":"approved"}));
            store.put("execution_operation",&json!({
                "id":format!("{session}-unknown"),"runId":format!("{session}-last"),
                "outcomeUnknown":true
            }));
        }
        let (runs,pending,unknown)=store.sdk_evidence_for_session("a").unwrap();
        assert_eq!(runs.len(),1);
        assert_eq!(runs[0]["id"],"a-last");
        assert_eq!(pending.len(),1);
        assert_eq!(pending[0]["id"],"a-pending");
        assert_eq!(unknown.len(),1);
        assert_eq!(unknown[0]["id"],"a-unknown");
        let (missing,pending,unknown)=store.sdk_evidence_for_session("none").unwrap();
        assert!(missing.is_empty()&&pending.is_empty()&&unknown.is_empty());
        assert_eq!(store.all("execution").len(),4);
    }

    #[test]
    fn sdk_session_and_limits_roll_back_together_when_run_persistence_fails() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        let session=json!({"id":"sdk-session","tokenBudget":4096,
            "budgetOwnerId":"sdk:run","nativeSdkRunId":"run"});
        assert!(store.transaction(&[
            ("session".to_string(),session.clone()),
            ("sdk_run".to_string(),json!({"status":"queued"}))
        ]).is_err());
        assert!(store.get("session","sdk-session").is_none(),
            "a failed run insert must not leave an orphaned or unbounded session");
        store.transaction(&[
            ("session".to_string(),session),
            ("sdk_run".to_string(),json!({"id":"run","sessionId":"sdk-session","status":"queued"}))
        ]).unwrap();
        assert_eq!(store.get("session","sdk-session").unwrap()["tokenBudget"],4096);
        assert_eq!(store.get("sdk_run","run").unwrap()["sessionId"],"sdk-session");
    }

    #[test]
    fn sdk_projection_only_loads_related_latest_executions_and_unknown_effects() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("sdk_run",&json!({"id":"sdk","sessionId":"owned"}));
        for (id,session,started) in [
            ("first","owned","2026-01-01"),("latest","owned","2026-11-17"),
            ("unrelated","other","2026-11-18")
        ] {
            store.put("execution",&json!({"id":id,"sessionId":session,"startedAt":started}));
        }
        store.put("execution_operation",&json!({
            "id":"owned-effect","runId":"latest","outcomeUnknown":true
        }));
        store.put("execution_operation",&json!({
            "id":"foreign-effect","runId":"unrelated","outcomeUnknown":true
        }));
        store.put("execution_operation",&json!({
            "id":"known-effect","runId":"latest","outcomeUnknown":false
        }));
        let (runs,effects)=store.sdk_evidence_rows().unwrap();
        assert_eq!(runs.len(),1,"only one latest SDK execution is needed");
        assert_eq!(runs[0]["id"],"latest");
        assert_eq!(effects.len(),1);
        assert_eq!(effects[0]["id"],"owned-effect");
        assert_eq!(store.all("execution").len(),3);
    }

    #[test]
    fn recovery_queries_only_decode_running_records_and_uncertain_sessions() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for (kind, id, status) in [
            ("execution", "run-live", "running"),
            ("execution", "run-old", "completed"),
            ("execution_operation", "op-live", "running"),
            ("execution_operation", "op-old", "completed"),
        ] {
            let mut record = json!({"id":id,"status":status});
            if kind == "execution" && id == "run-live" {
                record["sessionId"] = json!("session-1");
            }
            if kind == "execution_operation" {
                record["runId"] = json!(if id == "op-live" {"run-live"} else {"run-old"});
            }
            store.put(kind, &record);
        }
        assert_eq!(store.running_recovery_records("execution").unwrap().len(), 1);
        assert_eq!(store.running_recovery_records("execution_operation").unwrap().len(), 1);
        let sessions = store.uncertain_effect_sessions().unwrap();
        assert!(sessions.contains("session-1"));
        assert_eq!(sessions.len(), 1);
        assert!(store.running_recovery_records("job").is_err());
    }

    #[test]
    fn active_improvement_lookup_uses_partial_index_in_query_plan() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("improvement", &json!({"id":"active","kind":"strategy",
            "scope":"shared","partitionKey":"p","status":"adopted"})).unwrap();
        store.put("improvement", &json!({"id":"archived","kind":"strategy",
            "scope":"shared","partitionKey":"p","status":"rejected"})).unwrap();
        let selected=store.active_improvements_for_scope("p",Some("a")).unwrap();
        assert_eq!(selected.len(),1);
        assert_eq!(selected[0]["id"],"active");
        let conn=store.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "EXPLAIN QUERY PLAN
             SELECT data,id,created_at,updated_at FROM records
             WHERE kind='improvement'
               AND json_extract(data,'$.status') IN ('adopted','validated')
               AND json_extract(data,'$.kind') IN ('strategy','tool_policy')
               AND (json_extract(data,'$.scope')='global'
                    OR (json_extract(data,'$.partitionKey')=?1
                        AND (json_extract(data,'$.scope')='shared'
                             OR json_extract(data,'$.agentId') IS ?2)))
             ORDER BY updated_at DESC,rowid DESC",
        ).unwrap();
        let plans=stmt.query_map(params!["p",Some("a")],
            |row|row.get::<_,String>(3)
        ).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert!(plans.iter().any(|p|p.contains("idx_improvement_active_v2")),
            "active strategy search must use its partial index: {plans:?}");
    }

    #[test]
    fn auxiliary_model_receipts_are_scoped_to_one_execution() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,execution,phase) in [
            ("a-0","run-a","planning"),("a-1","run-a","planning"),
            ("b-0","run-b","acceptance_review"),
        ] {
            store.put("aux_model_call",&json!({
                "id":id,"executionId":execution,"logicalId":"plan-1",
                "phase":phase,"status":"completed","totalTokens":100
            })).unwrap();
        }
        let rows=store.auxiliary_model_calls("run-a").unwrap();
        assert_eq!(rows.len(),2);
        assert!(rows.iter().all(|row|row["executionId"]=="run-a"));
        assert!(store.auxiliary_model_calls("missing").unwrap().is_empty());
    }

    #[test]
    fn periodic_queries_skip_historical_approvals() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id, status) in [
            ("waiting", "pending"),
            ("approved", "approved"),
            ("expired", "expired"),
        ] {
            store.put("approval", &json!({"id":id,"status":status}));
        }
        let pending = store.pending_approvals().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["id"], "waiting");

    }
}
