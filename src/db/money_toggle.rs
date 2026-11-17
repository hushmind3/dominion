//! Money module lifecycle on the shared SQLite store (no second worker/DB).
use super::*;

impl Store {
    /// Atomically switch Money and stop only its owned jobs/autopilots.
    /// Independently approved SDK work is never silently stopped.
    pub fn money_set_module_enabled(&self, enabled:bool)
        -> Result<(Value,Vec<Value>,Vec<Value>),String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='money_module' AND id='global'",
            [],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let mut setting=raw.map(|s|serde_json::from_str::<Value>(&s)
            .map_err(|e|e.to_string())).transpose()?
            .unwrap_or(json!({"id":"global"}));
        setting["enabled"]=json!(enabled);
        setting["source"]=json!("owner");
        setting["updatedAt"]=json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx,"money_module",&setting)?;
        let mut stopped=Vec::new();
        let mut paused=Vec::new();
        if !enabled {
            let autos={
                let mut stmt=tx.prepare(
                    "SELECT data,id,created_at,updated_at FROM records
                     WHERE kind='money_autopilot'
                     AND json_extract(data,'$.enabled')=1"
                ).map_err(|e|e.to_string())?;
                let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
                rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
            };
            for mut item in autos {
                item["enabled"]=json!(false);
                item["stopReason"]=json!("module_disabled");
                item["updatedAt"]=json!(chrono::Utc::now().to_rfc3339());
                upsert_tx(&tx,"money_autopilot",&item)?;
                paused.push(item);
            }
            let jobs={
                let mut stmt=tx.prepare(
                    "SELECT data,id,created_at,updated_at FROM records
                     WHERE kind='job'
                     AND json_extract(data,'$.status') IN ('queued','running')
                     AND json_extract(data,'$.kind') IN
                      ('money_discover','money_ingest','money_catalog','money_fold')"
                ).map_err(|e|e.to_string())?;
                let rows=stmt.query_map([],record_from_row).map_err(|e|e.to_string())?;
                rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
            };
            for mut item in jobs {
                item["status"]=json!("cancelled");
                item["cancelRequested"]=json!(true);
                item["finishedAt"]=json!(chrono::Utc::now().to_rfc3339());
                item["error"]=json!("머니 도구가 비활성화돼 작업을 중단했습니다.");
                upsert_tx(&tx,"job",&item)?;
                stopped.push(item);
            }
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok((setting,paused,stopped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_money_is_off_and_user_can_enable_it(){
        let db=Store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(db.get("money_module","global").unwrap()["enabled"],false);
        assert_eq!(db.money_set_module_enabled(true).unwrap().0["enabled"],true);
    }
    #[test]
    fn disable_cancels_owned_work_without_touching_sdk_or_evidence(){
        let db=Store::open(std::path::Path::new(":memory:")).unwrap();
        db.money_set_module_enabled(true).unwrap();
        db.put("money_signal",&json!({"id":"evidence","title":"preserved"})).unwrap();
        db.put("money_autopilot",&json!({"id":"auto","enabled":true})).unwrap();
        db.put("job",&json!({"id":"collector","kind":"money_ingest","status":"queued"})).unwrap();
        db.put("job",&json!({"id":"inference","kind":"money_discover",
            "status":"running","sessionId":"s"})).unwrap();
        db.put("job",&json!({"id":"ordinary","kind":"prompt","status":"running"})).unwrap();
        db.put("sdk_run",&json!({"id":"approved","status":"running"})).unwrap();
        let (row,paused,stopped)=db.money_set_module_enabled(false).unwrap();
        assert_eq!(row["enabled"],false);
        assert_eq!((paused.len(),stopped.len()),(1,2));
        assert_eq!(db.get("job","collector").unwrap()["status"],"cancelled");
        assert_eq!(db.get("job","inference").unwrap()["cancelRequested"],true);
        assert_eq!(db.get("job","ordinary").unwrap()["status"],"running");
        assert_eq!(db.get("sdk_run","approved").unwrap()["status"],"running");
        assert_eq!(db.get("money_signal","evidence").unwrap()["title"],"preserved");
        db.money_set_module_enabled(true).unwrap();
        assert_eq!(db.get("money_autopilot","auto").unwrap()["enabled"],false);
    }
}
