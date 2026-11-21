//! Owner-scoped remote compute leases and settlement.
//!
//! Keep remote claims, expiry, receipt, and cancellation within one storage
//! boundary. Expired paid operations never silently become queued jobs.
use super::*;

impl Store {
    /// Reconcile remote paid inference whose owner-bound lease expired.
    /// Run on startup and periodically even if no workers reconnect.
    /// This UPDATE is atomic with claims and settlements and cannot restart
    /// speculative remote model calls or overwrite completed/cancelled tasks.
    pub fn review_expired_remote_tasks(&self)->Result<usize,String>{
        let now=chrono::Utc::now().timestamp_millis();
        let ended=chrono::Utc::now().to_rfc3339();
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute(
            "UPDATE records
             SET data=json_set(data,
                '$.status','needs_review',
                '$.phase','needs_review',
                '$.reviewReason',?2,
                '$.finishedAt',?3),
                updated_at=datetime('now')
             WHERE kind='remote_task'
               AND json_extract(data,'$.status')='running'
               AND CAST(json_extract(data,'$.leaseExpiresAtMs') AS INTEGER)<=?1",
            params![now,
                "Remote lease expired with unknown model outcome; owner review required, no replay",
                ended]
        ).map_err(|e|e.to_string())
    }

    /// Claim a remote compute-only task under the same SQLite transaction as
    /// stale-lease reconciliation. An expired paid inference is NEVER queued
    /// again: its outcome is unknown and must be reviewed by the owner.
    pub fn claim_remote_task(&self,worker:&str)->Result<Option<Value>,String>{
        let now=chrono::Utc::now().timestamp_millis();
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        let expired={
            let mut stmt=tx.prepare(
                "SELECT data FROM records WHERE kind='remote_task'
                 AND json_extract(data,'$.status')='running'
                 AND CAST(json_extract(data,'$.leaseExpiresAtMs') AS INTEGER)<=?1"
            ).map_err(|e|e.to_string())?;
            let rows=stmt.query_map([now],|r|r.get::<_,String>(0))
                .map_err(|e|e.to_string())?;
            rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
        };
        for raw in expired {
            let mut item:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
            item["status"]=json!("needs_review");
            item["phase"]=json!("needs_review");
            item["reviewReason"]=json!("Remote lease expired with unknown model/effect outcome; no auto replay");
            item["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
            upsert_tx(&tx,"remote_task",&item)?;
        }
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='remote_task'
             AND json_extract(data,'$.status')='queued'
             ORDER BY json_extract(data,'$.queuedAt'),id LIMIT 1",
            [],|r|r.get(0)
        ).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else {tx.commit().map_err(|e|e.to_string())?;return Ok(None);};
        let mut item:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        let id=item["id"].as_str().ok_or("Remote task missing ID")?.to_string();
        let lease=uuid::Uuid::new_v4().to_string();
        let attempt=item["attempts"].as_u64().unwrap_or(0).saturating_add(1);
        item["status"]=json!("running");
        item["phase"]=json!("running");
        item["workerId"]=json!(worker);
        item["leaseId"]=json!(lease);
        item["attempts"]=json!(attempt);
        item["leaseExpiresAtMs"]=json!(now.saturating_add(180_000));
        item["startedAt"]=json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx,"remote_task",&item)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some(json!({
            "taskId":id,"leaseId":lease,"attempt":attempt,
            "prompt":item["prompt"],"maxTokens":item["maxTokens"],
            "capability":"text_inference"
        })))
    }

    /// Heartbeat extends only the same still-live remote attempt.
    pub fn heartbeat_remote_task(&self,id:&str,worker:&str,lease:&str)
        ->Result<bool,String>{
        let now=chrono::Utc::now().timestamp_millis();
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='remote_task' AND id=?1",
            [id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else{return Ok(false)};
        let mut item:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        if item["status"]!="running" || item["workerId"]!=worker ||
            item["leaseId"]!=lease ||
            item["leaseExpiresAtMs"].as_i64().unwrap_or(0)<=now {
            return Ok(false);
        }
        item["leaseExpiresAtMs"]=json!(now.saturating_add(180_000));
        upsert_tx(&tx,"remote_task",&item)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(true)
    }

    /// One terminal receipt from the exact worker+lease, never authority for
    /// native workspace effects. This result remains externally unverified.
    pub fn finish_remote_task(&self,id:&str,worker:&str,lease:&str,
        status:&str,answer:&str,usage:&Value)->Result<Option<Value>,String>{
        if !matches!(status,"completed"|"needs_review") {
            return Err("Invalid remote terminal status".into());
        }
        if answer.chars().count()>16_384||usage.to_string().len()>2048 {
            return Err("Remote receipt exceeds bounds".into());
        }
        let now=chrono::Utc::now().timestamp_millis();
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='remote_task' AND id=?1",
            [id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else{return Ok(None)};
        let mut item:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        if item["workerId"]!=worker || item["leaseId"]!=lease{
            return Ok(None);
        }
        if matches!(item["status"].as_str(),Some("completed"|"needs_review")){
            // A lost HTTP acknowledgement can safely be repeated; the old
            // committed receipt stays the only result.
            return Ok(Some(item));
        }
        if item["status"]!="running"||
            item["leaseExpiresAtMs"].as_i64().unwrap_or(0)<=now{
            return Ok(None);
        }
        item["status"]=json!(status);
        item["phase"]=json!(status);
        item["result"]=json!({"answer":answer,"usage":usage,
            "verified":false,"evidenceGrade":"remote_unverified"});
        item["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx,"remote_task",&item)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some(item))
    }

    /// Owner cancellation revokes a live lease in one transaction; any late
    /// model result is rejected even when the HTTP response arrives afterward.
    pub fn cancel_remote_task(&self,id:&str)->Result<Option<Value>,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='remote_task' AND id=?1",
            [id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else{return Ok(None)};
        let mut item:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        if !matches!(item["status"].as_str(),Some("queued"|"running")){
            return Ok(None);
        }
        item["status"]=json!("cancelled");
        item["phase"]=json!("cancelled");
        item["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx,"remote_task",&item)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some(item))
    }

}
