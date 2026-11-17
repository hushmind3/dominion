//! Atomic approval and job admission within the existing Dominion SQLite Store.
use super::*;

impl Store {
    /// Returns candidate, job and whether this request admitted a new job.
    /// Both records commit under one SQLite lock, including on concurrent clicks.
    pub fn money_approve_implementation(&self,id:&str,job:&Value,agent:&str,budget:u64)
        ->Result<(Value,Value,bool),String>{
        let job_id=job["id"].as_str().filter(|s|!s.is_empty()).ok_or("invalid_job")?;
        if job["kind"]!="prompt" || job["status"]!="queued"
            || job["input"]["moneyCandidateId"].as_str()!=Some(id){
            return Err("invalid_job".into());
        }
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='money_candidate' AND id=?1",
            [id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else{return Err("candidate_missing".into());};
        let mut candidate:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        if candidate["status"]=="rejected"{return Err("candidate_rejected".into());}
        if let Some(previous)=candidate["implementationJobId"].as_str(){
            let raw:Option<String>=tx.query_row(
                "SELECT data FROM records WHERE kind='job' AND id=?1",
                [previous],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
            let Some(raw)=raw else{return Err("approval_orphan_job".into());};
            let stored:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
            if stored["kind"]!="prompt" || stored["input"]["moneyCandidateId"].as_str()!=Some(id){
                return Err("approval_invalid_link".into());
            }
            return Ok((candidate,stored,false));
        }
        // INSERT OR IGNORE prevents reuse of a deterministic job ID that
        // belongs to a pre-existing, unlinked or historical job.
        if tx.execute("INSERT OR IGNORE INTO records(kind,id,data) VALUES('job',?1,?2)",
            params![job_id,job.to_string()]).map_err(|e|e.to_string())?!=1{
            return Err("approval_conflict".into());
        }
        let now=chrono::Utc::now().to_rfc3339();
        candidate["implementationJobId"]=json!(job_id);
        candidate["implementationApprovedAt"]=json!(now);
        candidate["implementationAgentId"]=json!(agent);
        candidate["implementationTokenBudget"]=json!(budget);
        candidate["status"]=json!("implementation_requested");
        candidate["updatedAt"]=json!(now);
        upsert_tx(&tx,"money_candidate",&candidate)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok((candidate,job.clone(),true))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn setup()->Store{
        let s=Store::open(std::path::Path::new(":memory:")).unwrap();
        s.put("money_candidate",&json!({"id":"c1","status":"hypothesis"})).unwrap();s
    }
    fn job()->Value{json!({"id":"money:implement:c1","kind":"prompt","status":"queued",
        "input":{"moneyCandidateId":"c1","limits":{"maxEstimatedTokens":24000}}})}
    #[test] fn approval_and_job_are_atomic_and_idempotent(){
        let s=setup();
        let (candidate,first,new)=s.money_approve_implementation("c1",&job(),"a",24000).unwrap();
        assert!(new);assert_eq!(candidate["implementationJobId"],first["id"]);
        let (_,again,new)=s.money_approve_implementation("c1",&job(),"b",12000).unwrap();
        assert!(!new);assert_eq!(again,first);assert_eq!(s.count_kind("job"),1);
        assert_eq!(s.get("money_candidate","c1").unwrap()["implementationTokenBudget"],24000);
    }
    #[test] fn rejection_and_orphaned_job_do_not_create_second_job(){
        let s=setup();let mut c=s.get("money_candidate","c1").unwrap();
        c["status"]=json!("rejected");s.put("money_candidate",&c).unwrap();
        assert_eq!(s.money_approve_implementation("c1",&job(),"a",24000).unwrap_err(),"candidate_rejected");
        assert_eq!(s.count_kind("job"),0);
        c["status"]=json!("hypothesis");s.put("money_candidate",&c).unwrap();
        s.put("job",&json!({"id":"money:implement:c1","kind":"prompt","status":"completed"})).unwrap();
        assert_eq!(s.money_approve_implementation("c1",&job(),"a",24000).unwrap_err(),"approval_conflict");
        assert_eq!(s.get("job","money:implement:c1").unwrap()["status"],"completed");
    }
    #[test] fn racing_approvals_enqueue_only_once(){
        let s=setup();
        let handles=(0..8).map(|_|{
            let cloned=s.clone();std::thread::spawn(move||
                cloned.money_approve_implementation("c1",&job(),"a",24000).unwrap().2)
        }).collect::<Vec<_>>();
        assert_eq!(handles.into_iter().filter(|h|h.join().unwrap()).count(),1);
        assert_eq!(s.count_kind("job"),1);
    }
}
