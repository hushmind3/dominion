//! Parent-session scoped, indexed work graph read model.
use super::*;
impl Store {
    /// Bounded owner-session overview for Room/Team UI. Use two indexed
    /// projections so a room refresh never deserializes unrelated job history.
    /// The original unfiltered /api/jobs contract remains unchanged.
    pub fn session_job_overview(&self,session_id:&str,limit:usize)->Result<Vec<Value>,String>{
        if session_id.is_empty()||limit==0{return Ok(Vec::new());}
        let limit=limit.min(256);
        let direct:Vec<Value>={
            let conn=self.conn.lock().map_err(|e|e.to_string())?;
            let mut stmt=conn.prepare(
                "SELECT data,id,created_at,updated_at FROM records
                 WHERE kind='job' AND json_extract(data,'$.input.sessionId')=?1
                 ORDER BY updated_at DESC,id ASC LIMIT ?2"
            ).map_err(|e|e.to_string())?;
            let rows=stmt.query_map(params![session_id,limit as i64],
                record_from_row).map_err(|e|e.to_string())?;
            rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
        };
        let mut rows=direct;
        rows.extend(self.team_jobs(session_id,limit)?);
        rows.sort_by(|a,b|b["updatedAt"].as_str().cmp(&a["updatedAt"].as_str())
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str())));
        // A busy session can have >128 workers. Preserve the newest Team
        // roots in the bounded overview; opening a root reads its actual
        // children by the frozen plan rather than guessing from this page.
        let pinned=self.recent_team_roots(session_id,limit.min(8))?;
        let mut seen=pinned.iter().filter_map(|row|row["id"].as_str()
            .map(str::to_string)).collect::<std::collections::HashSet<_>>();
        let mut extra=Vec::new();
        for row in rows {
            if row["id"].as_str().is_some_and(|id|seen.insert(id.to_string())) {
                extra.push(row);
            }
        }
        extra.truncate(limit.saturating_sub(pinned.len()));
        let mut overview=pinned;
        overview.extend(extra);
        overview.sort_by(|a,b|b["updatedAt"].as_str().cmp(&a["updatedAt"].as_str())
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str())));
        Ok(overview)
    }
    /// Keep newest native Team roots visible even when hundreds of worker
    /// receipts are newer. Query only the owner's indexed Team rows.
    fn recent_team_roots(&self,session_id:&str,limit:usize)->Result<Vec<Value>,String>{
        if limit==0{return Ok(Vec::new());}
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='job' AND json_extract(data,'$.input.sessionId')=?1
               AND json_extract(data,'$.kind')='team'
             ORDER BY json_extract(data,'$.queuedAt') DESC,id DESC LIMIT ?2"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![session_id,limit.min(8) as i64],
            record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
    /// Only jobs delegated by this parent; never decode every unrelated job.
    pub fn team_jobs(&self,parent_session_id:&str,limit:usize)->Result<Vec<Value>,String>{
        if parent_session_id.is_empty()||limit==0{return Ok(Vec::new());}
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='job' AND json_extract(data,'$.input.parentSessionId')=?1
             ORDER BY updated_at DESC,id ASC LIMIT ?2"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![parent_session_id,limit.min(128) as i64],
            record_from_row).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn team_graph_is_indexed_bounded_and_parent_scoped(){
        let store=Store::open(Path::new(":memory:")).unwrap();
        for i in 0..24{
            store.put("job",&json!({"id":format!("j{i:02}"),"kind":"prompt",
                "status":"queued","input":{"parentSessionId":"owner",
                    "dependsOnJobIds":if i==0{json!([])}else{json!([format!("j{:02}",i-1)])}
                }})).unwrap();
        }
        store.put("job",&json!({"id":"foreign","kind":"prompt",
            "input":{"parentSessionId":"someone-else"}})).unwrap();
        let rows=store.team_jobs("owner",7).unwrap();
        assert_eq!(rows.len(),7);
        assert!(rows.iter().all(|row|row["id"]!="foreign"));
        assert!(store.team_jobs("someone-else",12).unwrap().iter()
            .all(|row|row["id"]=="foreign"));
        let plan:String=store.conn.lock().unwrap().query_row(
            "EXPLAIN QUERY PLAN SELECT data,id,created_at,updated_at
             FROM records WHERE kind='job'
             AND json_extract(data,'$.input.parentSessionId')='owner'
             ORDER BY updated_at DESC,id ASC LIMIT 7",
             [],|row|row.get(3)).unwrap();
        assert!(plan.contains("idx_team_parent_jobs"),"{plan}");
    }

    #[test]
    fn owner_session_overview_is_bounded_scoped_and_uses_both_indexes(){
        let store=Store::open(Path::new(":memory:")).unwrap();
        store.put("job",&json!({"id":"team","kind":"team","status":"running",
            "input":{"sessionId":"owner","plan":[]}})).unwrap();
        store.put("job",&json!({"id":"child","kind":"prompt","status":"completed",
            "input":{"sessionId":"isolated-worker","parentSessionId":"owner",
                "delegation":true}})).unwrap();
        store.put("job",&json!({"id":"foreign","kind":"team","status":"running",
            "input":{"sessionId":"other"}})).unwrap();
        store.put("job",&json!({"id":"foreign-child","kind":"prompt",
            "input":{"parentSessionId":"other","delegation":true}})).unwrap();
        let scoped=store.session_job_overview("owner",128).unwrap();
        assert_eq!(scoped.len(),2);
        assert!(scoped.iter().any(|row|row["id"]=="team"));
        assert!(scoped.iter().any(|row|row["id"]=="child"));
        assert!(!scoped.iter().any(|row|row["id"]=="foreign"));
        assert_eq!(store.session_job_overview("owner",1).unwrap().len(),1);
        assert!(store.session_job_overview("",128).unwrap().is_empty());
        let plan:String=store.conn.lock().unwrap().query_row(
            "EXPLAIN QUERY PLAN SELECT data,id,created_at,updated_at FROM records
             WHERE kind='job' AND json_extract(data,'$.input.sessionId')='owner'
             ORDER BY updated_at DESC,id ASC LIMIT 128",
            [],|row|row.get(3)).unwrap();
        assert!(plan.contains("idx_job_input_session"),"{plan}");
    }

    #[test]
    fn recent_team_root_survives_worker_history_truncation(){
        let store=Store::open(Path::new(":memory:")).unwrap();
        store.put("job",&json!({"id":"team-old","kind":"team","status":"needs_review",
            "queuedAt":"2026-11-20T00:00:00Z",
            "input":{"sessionId":"owner","plan":[]}})).unwrap();
        for index in 0..160{
            store.put("job",&json!({"id":format!("worker-{index:03}"),
                "kind":"prompt","status":"completed",
                "queuedAt":"2026-11-21T00:00:00Z",
                "input":{"sessionId":format!("private-{index}"),
                    "parentSessionId":"owner","teamJobId":"team-old"}})).unwrap();
        }
        let result=store.session_job_overview("owner",128).unwrap();
        assert_eq!(result.len(),128);
        assert!(result.iter().any(|j|j["id"]=="team-old"),
            "the Team root must remain reachable after the last 128 workers");
        assert!(!result.iter().any(|j|j["id"]=="foreign"));
        let query:String=store.conn.lock().unwrap().query_row(
            "EXPLAIN QUERY PLAN SELECT data,id,created_at,updated_at FROM records
             WHERE kind='job' AND json_extract(data,'$.input.sessionId')='owner'
               AND json_extract(data,'$.kind')='team'
             ORDER BY json_extract(data,'$.queuedAt') DESC,id DESC LIMIT 8",
            [],|row|row.get(3)).unwrap();
        assert!(query.contains("idx_team_owner_roots"),"{query}");
    }
}
