//! Indexed lookup for generic SDK run provenance; optional tools share SQLite.
use super::*;

impl Store {
    /// The latest execution for a single SDK session, with the same ordering
    /// as the SDK public view. Never deserialize unrelated historical runs.
    pub fn sdk_latest_execution(&self,session:&str)->Result<Option<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='execution' AND json_extract(data,'$.sessionId')=?1
             ORDER BY json_extract(data,'$.startedAt') DESC,id DESC LIMIT 1",
            [session],record_from_row
        ).optional().map_err(|e|e.to_string())
    }
    /// The operator reconciliation path must load only snapshots owned by
    /// this exact native execution, never every frame in the database.
    pub fn sdk_frames_for_execution(&self,run_id:&str)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='native_frame'
               AND json_extract(data,'$.parentExecutionId')=?1
             ORDER BY id"
        ).map_err(|e|e.to_string())?;
        let frames=stmt.query_map([run_id],record_from_row).map_err(|e|e.to_string())?;
        frames.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
    /// Apply the exact source ID before LIMIT: a busy neighboring app cannot
    /// push this app's recent history out of the result.
    pub fn sdk_runs_for_source_id(&self, source_kind:&str, source_id:&str, limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='sdk_run' AND json_extract(data,'$.source.kind')=?1
               AND json_extract(data,'$.source.id')=?2
             ORDER BY created_at DESC,id DESC LIMIT ?3"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![source_kind,source_id,limit.clamp(1,100) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Only deserialize bounded runs for one source kind, not all historic SDK jobs.
    pub fn sdk_runs_by_source(&self, source_kind:&str, limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='sdk_run' AND json_extract(data,'$.source.kind')=?1
             ORDER BY created_at DESC LIMIT ?2"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![source_kind,limit.clamp(1,100) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
}
