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

impl Store {
    /// Admit a plugin request, its retry receipt and its native session in
    /// the same transaction as the credential/status check. No request can
    /// cross a disable/rotation boundary or overwrite a previous retry.
    pub(crate) fn claim_plugin_sdk_run(&self, plugin:&str, hash:&str, key:&str,
        fingerprint:&str, session:&Value, run:&Value) -> Result<(String,bool),String> {
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let row:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='sdk_plugin' AND id=?1",
            [plugin],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let mut row:Value=serde_json::from_str(&row.ok_or("credential_changed")?)
            .map_err(|e|e.to_string())?;
        if row["status"]!="enabled" || row["tokenSha256"].as_str()!=Some(hash) {
            return Err("credential_changed".into());
        }
        let prior:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='sdk_plugin_claim' AND id=?1",
            [key],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        if let Some(prior)=prior {
            let claim:Value=serde_json::from_str(&prior).map_err(|e|e.to_string())?;
            if claim["fingerprint"]!=fingerprint || claim["pluginId"]!=plugin {
                return Err("retry_conflict".into());
            }
            return Ok((claim["runId"].as_str().ok_or("invalid_claim")?.into(),false));
        }
        // One active task per plugin and a bounded daily allowance. These
        // limits are checked under the same transaction as admission.
        let active:i64=tx.query_row(
            "SELECT COUNT(*) FROM records WHERE kind='sdk_run'
             AND json_extract(data,'$.sdkPluginId')=?1
             AND json_extract(data,'$.status') IN ('queued','running','needs_review')",
            [plugin],|r|r.get(0)).map_err(|e|e.to_string())?;
        if active>=1 { return Err("active_limit".into()); }
        let daily:i64=tx.query_row(
            "SELECT COUNT(*) FROM records WHERE kind='sdk_run'
             AND json_extract(data,'$.sdkPluginId')=?1
             AND created_at>=datetime('now','-1 day')",
            [plugin],|r|r.get(0)).map_err(|e|e.to_string())?;
        if daily>=20 { return Err("daily_limit".into()); }
        let id=run["id"].as_str().ok_or("missing_run_id")?.to_string();
        if run["sdkPluginId"]!=plugin || session["nativeSdkRunId"]!=id {
            return Err("invalid_plugin_binding".into());
        }
        upsert_tx(&tx,"session",session)?;
        upsert_tx(&tx,"sdk_run",run)?;
        upsert_tx(&tx,"sdk_plugin_claim",&json!({"id":key,"pluginId":plugin,
            "fingerprint":fingerprint,"runId":id}))?;
        row["lastSeenAt"]=json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx,"sdk_plugin",&row)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok((id,true))
    }
}
