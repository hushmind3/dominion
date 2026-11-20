//! Parent-session scoped, indexed work graph read model.
use super::*;
impl Store {
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
}
