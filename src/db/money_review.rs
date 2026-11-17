//! Money decision journal and bounded archive lookup on the shared SQLite Store.
//! Review operations and append-only history commit atomically. No model needed.
use super::*;

impl Store {
    pub fn money_review_candidate(
        &self,id:&str,new_status:&str,reason:&str,
    )->Result<Value,String>{
        if !matches!(new_status,"hypothesis"|"deferred"|"rejected"){
            return Err("unsupported_review_status".into());
        }
        if reason.chars().count()>1500{return Err("review_reason_too_long".into());}
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='money_candidate' AND id=?1",
            [id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else{return Err("candidate_not_found".into());};
        let mut candidate:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        if candidate["implementationJobId"].is_string(){
            return Err("implementation_already_requested".into());
        }
        let before=candidate["status"].as_str().unwrap_or("hypothesis");
        if before==new_status && candidate["reviewReason"].as_str()==Some(reason){
            return Ok(candidate);
        }
        let now=chrono::Utc::now().to_rfc3339();
        let revision=candidate["reviewRevision"].as_u64().unwrap_or(0)+1;
        let event=json!({
            "id":uuid::Uuid::new_v4().to_string(),"candidateId":id,
            "fromStatus":before,"toStatus":new_status,
            "reason":reason,"revision":revision,"reviewedAt":now,
            "source":"explicit_user_review"
        });
        candidate["status"]=json!(new_status);
        candidate["reviewReason"]=json!(reason);
        candidate["reviewedAt"]=json!(now);
        candidate["reviewRevision"]=json!(revision);
        candidate["updatedAt"]=json!(now);
        upsert_tx(&tx,"money_candidate",&candidate)?;
        upsert_tx(&tx,"money_review",&event)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(candidate)
    }

    /// Includes low-ranked, rejected and deferred records. Query and status
    /// filters run in SQLite before pagination; no full archive materialization.
    pub fn money_candidate_archive(
        &self,status:&str,query:&str,limit:usize,offset:usize
    )->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let statement=if status.is_empty(){
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_candidate' AND (?1='' OR
                instr(lower(coalesce(json_extract(data,'$.title'),'')||' '||
                            coalesce(json_extract(data,'$.offer'),'')||' '||
                            coalesce(json_extract(data,'$.customer'),'')),lower(?1))>0)
             ORDER BY updated_at DESC,id DESC LIMIT ?2 OFFSET ?3"
        }else{
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_candidate' AND json_extract(data,'$.status')=?4
             AND (?1='' OR instr(lower(coalesce(json_extract(data,'$.title'),'')||' '||
                            coalesce(json_extract(data,'$.offer'),'')||' '||
                            coalesce(json_extract(data,'$.customer'),'')),lower(?1))>0)
             ORDER BY updated_at DESC,id DESC LIMIT ?2 OFFSET ?3"
        };
        let mut stmt=conn.prepare(statement).map_err(|e|e.to_string())?;
        let limit=limit.clamp(1,50) as i64;
        let offset=offset.min(100_000) as i64;
        let results=if status.is_empty(){
            let rows=stmt.query_map(params![query,limit,offset],record_from_row)
                .map_err(|e|e.to_string())?;
            rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
        }else{
            let rows=stmt.query_map(params![query,limit,offset,status],record_from_row)
                .map_err(|e|e.to_string())?;
            rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
        };
        Ok(results)
    }

    pub fn money_review_history(&self,id:&str,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_review' AND json_extract(data,'$.candidateId')=?1
             ORDER BY json_extract(data,'$.revision') DESC LIMIT ?2")
            .map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![id,limit.clamp(1,100) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    pub fn money_source_revisions(&self,id:&str,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_source_revision'
             AND json_extract(data,'$.sourceId')=?1
             ORDER BY created_at DESC,id DESC LIMIT ?2")
            .map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![id,limit.clamp(1,100) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn sample()->Store{
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,status) in [("c1","hypothesis"),("c2","deferred"),("c3","rejected")]{
            store.put("money_candidate",&json!({"id":id,"status":status,
                "title":format!("Buyer hypothesis {id}"),"offer":"Market service",
                "customer":"Regional buyer"})).unwrap();
        }
        store
    }
    #[test] fn deferral_and_restoration_preserve_old_hypotheses_and_journal(){
        let s=sample();
        let deferred=s.money_review_candidate("c1","deferred","Check purchasing original").unwrap();
        assert_eq!(deferred["status"],"deferred");
        assert_eq!(s.money_candidate_archive("deferred","",20,0).unwrap().len(),2);
        let restored=s.money_review_candidate("c1","hypothesis","New buyer evidence").unwrap();
        assert_eq!(restored["status"],"hypothesis");
        let history=s.money_review_history("c1",10).unwrap();
        assert_eq!(history.len(),2);
        assert_eq!(history[0]["fromStatus"],"deferred");
        assert_eq!(history[1]["toStatus"],"deferred");
        assert_eq!(s.count_kind("money_candidate"),3);
        // No model calls, and a repeated review has no duplicate event.
        s.money_review_candidate("c1","hypothesis","New buyer evidence").unwrap();
        assert_eq!(s.money_review_history("c1",10).unwrap().len(),2);
    }
    #[test] fn archived_low_rank_and_rejected_candidates_remain_searchable(){
        let s=sample();
        assert_eq!(s.money_candidate_archive("rejected","",10,0).unwrap()[0]["id"],"c3");
        assert_eq!(s.money_candidate_archive("","regional",10,1).unwrap().len(),2);
        assert_eq!(s.money_candidate_archive("","not-matched",10,0).unwrap().len(),0);
    }
    #[test] fn reviewed_candidates_cannot_silently_detach_implementation(){
        let s=sample();let mut c=s.get("money_candidate","c1").unwrap();
        c["implementationJobId"]=json!("running-job");
        s.put("money_candidate",&c).unwrap();
        assert_eq!(s.money_review_candidate("c1","deferred","").unwrap_err(),
            "implementation_already_requested");
        assert!(s.money_review_history("c1",10).unwrap().is_empty());
    }
}
