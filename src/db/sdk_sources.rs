//! Indexed lookup for generic SDK run provenance; optional tools share SQLite.
use super::*;

impl Store {
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
