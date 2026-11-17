//! Bounded searchable Money candidate archive on existing indexed SQLite records.
use super::*;

impl Store {
    /// Search all historical candidates, including rejected/deferred records.
    /// Only one page is deserialized; full JSON payloads never go to an LLM.
    pub fn money_candidate_archive(&self, query:&str, status:&str,
        limit:usize, offset:usize)->Result<(Vec<Value>,bool),String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let take=limit.clamp(1,50);
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_candidate'
               AND (?1='' OR COALESCE(json_extract(data,'$.status'),'hypothesis')=?1)
               AND (?2='' OR instr(lower(COALESCE(json_extract(data,'$.title'),'')),lower(?2))>0
                 OR instr(lower(COALESCE(json_extract(data,'$.customer'),'')),lower(?2))>0
                 OR instr(lower(COALESCE(json_extract(data,'$.offer'),'')),lower(?2))>0
                 OR instr(lower(COALESCE(json_extract(data,'$.whyPay'),'')),lower(?2))>0)
             ORDER BY updated_at DESC,id DESC LIMIT ?3 OFFSET ?4"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![status,query,(take+1) as i64,offset.min(100_000) as i64],
            record_from_row).map_err(|e|e.to_string())?;
        let mut items=rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        let has_more=items.len()>take;
        items.truncate(take);
        Ok((items,has_more))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_searches_deferred_and_rejected_candidates_with_bounded_paging(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,status,title,customer) in [
            ("1","hypothesis","자동화 서비스","기업"),
            ("2","deferred","물류 자동화","창고"),
            ("3","rejected","기존 제안","물류 구매자"),
            ("4","validated","산업 장비","공장"),
        ]{
            store.put("money_candidate",&json!({"id":id,"title":title,
                "customer":customer,"status":status})).unwrap();
        }
        let (items,more)=store.money_candidate_archive("", "",2,0).unwrap();
        assert_eq!(items.len(),2);
        assert!(more);
        let (page,more)=store.money_candidate_archive("", "",2,2).unwrap();
        assert_eq!(page.len(),2);
        assert!(!more);
        let (deferred,_)=store.money_candidate_archive("", "deferred",20,0).unwrap();
        assert_eq!(deferred.len(),1);
        assert_eq!(deferred[0]["id"],"2");
        let (search,_)=store.money_candidate_archive("물류","",20,0).unwrap();
        assert_eq!(search.len(),2);
        assert!(search.iter().any(|r|r["status"]=="rejected"));
    }
}
