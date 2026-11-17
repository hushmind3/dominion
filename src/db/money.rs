//! Money-specific SQLite records and indexed market research.
use super::*;

/// Money-owned schema extensions on the existing shared SQLite connection.
/// No separate DB file, migration service, or domain-specific storage engine.
pub(super) const SCHEMA: &str = r#"
             CREATE INDEX IF NOT EXISTS idx_money_signal_created
                 ON records(kind, created_at DESC) WHERE kind='money_signal';
             CREATE INDEX IF NOT EXISTS idx_money_source_unfolded
                 ON records(created_at) WHERE kind='money_signal'
                 AND COALESCE(json_extract(data,'$.foldStatus'),'new')='new';
             -- Money Lab knowledge is indexed independently from chat memory.
             CREATE INDEX IF NOT EXISTS idx_money_campaign
                 ON records(json_extract(data, '$.campaignId'), updated_at DESC)
                 WHERE kind='money_candidate';
             CREATE INDEX IF NOT EXISTS idx_money_experiment_candidate
                 ON records(json_extract(data,'$.candidateId')) WHERE kind='money_experiment';
             CREATE INDEX IF NOT EXISTS idx_money_experiment_campaign
                 ON records(json_extract(data, '$.campaignId'), updated_at DESC)
                 WHERE kind='money_experiment';
             CREATE INDEX IF NOT EXISTS idx_money_metric_entity
                 ON records(json_extract(data,'$.entityId'),
                    json_extract(data,'$.metric'),json_extract(data,'$.observedAt') DESC)
                 WHERE kind='money_metric';
             CREATE INDEX IF NOT EXISTS idx_money_metric_change
                 ON records(json_extract(data,'$.observedAt') DESC)
                 WHERE kind='money_metric_change';
             CREATE INDEX IF NOT EXISTS idx_money_event_entity
                 ON records(json_extract(data,'$.entityKey'),
                            json_extract(data,'$.marketRegion'),
                            json_extract(data,'$.effectiveDate'))
                 WHERE kind='money_event';
             CREATE INDEX IF NOT EXISTS idx_money_event_state
                 ON records(json_extract(data,'$.status'), created_at DESC)
                 WHERE kind='money_event';
             CREATE INDEX IF NOT EXISTS idx_money_asym_state
                 ON records(json_extract(data,'$.status'),
                            json_extract(data,'$.researchPriority') DESC)
                 WHERE kind='money_asymmetry';
             CREATE INDEX IF NOT EXISTS idx_money_tension_priority
                 ON records(json_extract(data,'$.researchPriority') DESC) WHERE kind='money_tension';
             CREATE INDEX IF NOT EXISTS idx_money_relation_from
                 ON records(json_extract(data,'$.from')) WHERE kind='money_relation';
             CREATE INDEX IF NOT EXISTS idx_money_relation_to
                 ON records(json_extract(data,'$.to')) WHERE kind='money_relation';
             CREATE INDEX IF NOT EXISTS idx_money_signal_captured
                 ON records(json_extract(data,'$.capturedAt')) WHERE kind='money_signal';
             CREATE INDEX IF NOT EXISTS idx_money_signal_source_url
                 ON records(json_extract(data,'$.url')) WHERE kind='money_signal';
             CREATE VIRTUAL TABLE IF NOT EXISTS money_fact_fts
                 USING fts5(id UNINDEXED, claim, tokenize='trigram');
             CREATE TABLE IF NOT EXISTS money_fact_term(
                 term TEXT NOT NULL, fact_id TEXT NOT NULL, PRIMARY KEY(term,fact_id));
             CREATE INDEX IF NOT EXISTS idx_money_term_fact ON money_fact_term(fact_id);
"#;

/// Korean two-character terms and compound market vocabulary need a separate
/// index because SQLite's trigram FTS cannot match <3-character queries.
fn money_index_terms(text:&str)->Vec<String>{
    let mut seen=std::collections::HashSet::new();
    let mut output=Vec::new();
    for part in text.to_lowercase().split(|c:char|!c.is_alphanumeric()){
        if part.is_empty(){continue;}
        let chars=part.chars().take(70).collect::<Vec<_>>();
        let mut words=Vec::new();
        if (2..=45).contains(&chars.len()){words.push(chars.iter().copied().collect::<String>());}
        for window in chars.windows(2){words.push(window.iter().copied().collect::<String>());}
        for word in words {
            if seen.insert(word.clone()){output.push(word);}
            if output.len()>=120{return output;}
        }
    }
    output
}


/// Identified source fact insertion is immutable and updates the lexical index
/// only when the record is new. Replaying a page scrape cannot multiply data.
fn persist_money_fact_tx(tx:&rusqlite::Transaction<'_>,record:&Value)->Result<bool,String>{
    let id=record["id"].as_str().ok_or("Money fact id missing")?;
    let claim=record["claim"].as_str().filter(|s|!s.trim().is_empty())
        .ok_or("Money fact claim missing")?;
    let added=tx.execute(
        "INSERT OR IGNORE INTO records(kind,id,data) VALUES('money_fact',?1,?2)",
        params![id,record.to_string()]).map_err(|e|e.to_string())?;
    if added>0 {
        tx.execute("INSERT INTO money_fact_fts(id,claim) VALUES(?1,?2)",
            params![id,claim]).map_err(|e|e.to_string())?;
        for term in money_index_terms(claim){
            tx.execute(
                "INSERT OR IGNORE INTO money_fact_term(term,fact_id) VALUES(?1,?2)",
                params![term,id]).map_err(|e|e.to_string())?;
        }
    }
    Ok(added>0)
}

fn money_signal_fact(signal:&Value)->Value{
    json!({"id":format!("source:{}",signal["id"].as_str().unwrap_or("")),
        "claim":format!("{} {} {}",signal["title"].as_str().unwrap_or(""),
            signal["description"].as_str().unwrap_or(""),
            signal["articleExcerpt"].as_str().unwrap_or("").chars().take(300).collect::<String>()),
        "url":signal["url"],"kind":"market_source",
        "verification":"observed_headline_not_verified_claim",
        "sourceName":signal["sourceName"],"createdAt":signal["capturedAt"]})
}

/// Refresh only source-derived facts when a stored headline gains page text.
/// The data and both search indexes are updated in one transaction.
fn refresh_money_source_fact_tx(tx:&rusqlite::Transaction<'_>,record:&Value)->Result<(),String>{
    let id=record["id"].as_str().filter(|id|id.starts_with("source:"))
        .ok_or("Invalid source fact id")?;
    let claim=record["claim"].as_str().filter(|s|!s.trim().is_empty())
        .ok_or("Source fact claim missing")?;
    upsert_tx(tx,"money_fact",record)?;
    tx.execute("DELETE FROM money_fact_fts WHERE id=?1",[id]).map_err(|e|e.to_string())?;
    tx.execute("DELETE FROM money_fact_term WHERE fact_id=?1",[id]).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO money_fact_fts(id,claim) VALUES(?1,?2)",params![id,claim])
        .map_err(|e|e.to_string())?;
    for term in money_index_terms(claim){
        tx.execute("INSERT OR IGNORE INTO money_fact_term(term,fact_id) VALUES(?1,?2)",
            params![term,id]).map_err(|e|e.to_string())?;
    }
    Ok(())
}


impl Store {
    pub fn money_fact_put(&self, record: &Value) -> Result<Value, String> {
        let id = record["id"].as_str().filter(|s| !s.is_empty()).ok_or("money fact ID missing")?;
        let claim = record["claim"].as_str().filter(|s| !s.trim().is_empty()).ok_or("money claim missing")?;
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        upsert_tx(&tx, "money_fact", record)?;
        tx.execute("DELETE FROM money_fact_fts WHERE id=?1", [id]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO money_fact_fts(id,claim) VALUES(?1,?2)", params![id,claim]).map_err(|e|e.to_string())?;
        tx.execute("DELETE FROM money_fact_term WHERE fact_id=?1", [id]).map_err(|e|e.to_string())?;
        for term in money_index_terms(claim){
            tx.execute("INSERT OR IGNORE INTO money_fact_term(term,fact_id) VALUES(?1,?2)",params![term,id])
                .map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e|e.to_string())?;
        drop(conn);
        self.get("money_fact",id).ok_or("fact lookup failed after write".into())
    }

    /// Bounded recent market evidence without loading the entire records table.
    pub fn money_fact_recent(&self, limit: usize) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|e|e.to_string())?;
        let mut statement=conn.prepare("SELECT data,id,created_at,updated_at FROM records WHERE kind='money_fact' ORDER BY updated_at DESC LIMIT ?1")
            .map_err(|e|e.to_string())?;
        let rows=statement.query_map([limit.clamp(1,500) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Resolve a claimed buyer URL only through the already archived source.
    /// This keeps buyer proof checks indexed as the corpus grows.
    pub fn money_signal_by_url(&self,url:&str)->Result<Option<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_signal' AND json_extract(data,'$.url')=?1 LIMIT 1",
            [url],record_from_row)
            .optional().map_err(|e|e.to_string())
    }

    /// Use indexed campaign lookup, not a scan of every opportunity ever seen.
    pub fn money_recent_candidates(&self, campaign: &str, limit: usize) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut statement=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='money_candidate'
             AND json_extract(data,'$.campaignId')=?1 ORDER BY updated_at DESC LIMIT ?2")
            .map_err(|e|e.to_string())?;
        let rows=statement.query_map(params![campaign, limit.clamp(1,200) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    pub fn money_recent_experiments(&self,campaign:&str,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut statement=conn.prepare("SELECT data,id,created_at,updated_at FROM records WHERE kind='money_experiment'
            AND json_extract(data,'$.campaignId')=?1 ORDER BY updated_at DESC LIMIT ?2")
            .map_err(|e|e.to_string())?;
        let rows=statement.query_map(params![campaign,limit.clamp(1,200) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Top-K indexed recall. Query text is quoted to prevent FTS operators
    /// from turning untrusted market prose into query syntax.
    pub fn money_fact_search(&self, query: &str, limit: usize) -> Result<Vec<Value>, String> {
        let tokens=money_index_terms(query).into_iter().take(32).collect::<Vec<_>>();
        if tokens.is_empty(){return Ok(Vec::new())}
        let mut scores=std::collections::HashMap::<String,i32>::new();
        {
            let conn=self.conn.lock().map_err(|e|e.to_string())?;
            let mut statement=conn.prepare("SELECT fact_id FROM money_fact_term WHERE term=?1 LIMIT 100")
                .map_err(|e|e.to_string())?;
            for token in tokens {
                let rows=statement.query_map([token],|row|row.get::<_,String>(0)).map_err(|e|e.to_string())?;
                for id in rows {let id=id.map_err(|e|e.to_string())?;*scores.entry(id).or_default()+=1;}
            }
        }
        // FTS5 trigram contributes exact longer-phrase relevance, while the
        // indexed bigram path handles Korean two-character technical terms.
        let fts_terms=query.split(|c:char|!c.is_alphanumeric())
            .filter(|word|word.chars().count()>=3)
            .take(8).map(|word|format!("\"{}\"",word.replace('"',"\"\"")))
            .collect::<Vec<_>>();
        if !fts_terms.is_empty(){
            let fts_query=fts_terms.join(" OR ");
            let conn=self.conn.lock().map_err(|e|e.to_string())?;
            let mut statement=conn.prepare("SELECT id FROM money_fact_fts WHERE money_fact_fts MATCH ?1 ORDER BY rank LIMIT 60")
                .map_err(|e|e.to_string())?;
            let rows=statement.query_map([fts_query],|row|row.get::<_,String>(0)).map_err(|e|e.to_string())?;
            for id in rows {let id=id.map_err(|e|e.to_string())?;*scores.entry(id).or_default()+=3;}
        }
        let mut ranked=scores.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|a,b|b.1.cmp(&a.1).then_with(||a.0.cmp(&b.0)));
        Ok(ranked.into_iter().take(limit.clamp(1,40))
            .filter_map(|(id,_)|self.get("money_fact",&id)).collect())
    }
    


    pub fn money_claim_job(
        &self,campaign_id:&str,job:&Value,auto_guard:Option<(&str,u64,&Value)>
    )->Result<Option<Value>,String>{
        let job_id=job["id"].as_str().ok_or("Money job id missing")?;
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='money_campaign' AND id=?1",
            [campaign_id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
        let Some(raw)=raw else{return Err("Money campaign missing".into());};
        let mut campaign:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        let auto_raw:Option<String>=tx.query_row(
            "SELECT data FROM records WHERE kind='money_autopilot' AND id=?1",
            [campaign_id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
        let auto=auto_raw.map(|raw|serde_json::from_str::<Value>(&raw)).transpose()
            .map_err(|e|e.to_string())?;
        if let Some((run_id,round,_))=auto_guard{
            if auto.as_ref().is_none_or(|a|a["enabled"]!=true
                ||a["runId"].as_str()!=Some(run_id)
                ||a["scheduledRounds"].as_u64()!=Some(round)){
                return Ok(None);
            }
        }else if auto.as_ref().is_some_and(|a|a["enabled"]==true){
            return Ok(None);
        }
        if let Some(previous_id)=campaign["lastJobId"].as_str(){
            let raw:Option<String>=tx.query_row(
                "SELECT data FROM records WHERE kind='job' AND id=?1",
                [previous_id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
            if raw.as_deref().is_some_and(|value|serde_json::from_str::<Value>(value)
                .ok().is_some_and(|v|matches!(v["status"].as_str(),Some("queued"|"running")))){
                return Ok(None);
            }
        }
        let existing:Option<String>=tx.query_row(
            "SELECT id FROM records WHERE kind='job' AND id=?1",
            [job_id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
        if existing.is_some(){return Ok(None);}
        campaign["lastJobId"]=json!(job_id);
        campaign["lastMode"]=job["input"]["mode"].clone();
        campaign["updatedAt"]=json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx,"money_campaign",&campaign)?;
        if let Some((_,_,next))=auto_guard{upsert_tx(&tx,"money_autopilot",next)?;}
        upsert_tx(&tx,"job",job)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some(job.clone()))
    }

    /// Ingestion is all-or-nothing. A model retry may not reset user edits or
    /// create half a dossier (candidates without sources / report).
    pub fn money_commit_discovery(
        &self,report:&Value,candidates:&[Value],facts:&[Value],edges:&[Value],
        assessed_asymmetry_ids:&[String]
    )->Result<bool,String>{
        let report_id=report["id"].as_str().ok_or("Money report id missing")?;
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let present:Option<String>=tx.query_row(
            "SELECT id FROM records WHERE kind='money_report' AND id=?1",
            [report_id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
        if present.is_some(){return Ok(false);}
        for candidate in candidates{
            let id=candidate["id"].as_str().ok_or("Money candidate id missing")?;
            tx.execute(
                "INSERT OR IGNORE INTO records(kind,id,data) VALUES('money_candidate',?1,?2)",
                params![id,candidate.to_string()]).map_err(|e|e.to_string())?;
        }
        for fact in facts{
            persist_money_fact_tx(&tx,fact)?;
        }
        for edge in edges{
            let id=edge["id"].as_str().ok_or("Money graph edge id missing")?;
            if edge["from"].as_str().is_none()||edge["to"].as_str().is_none() {
                return Err("Money graph edge endpoint missing".into());
            }
            tx.execute("INSERT OR IGNORE INTO records(kind,id,data) VALUES('money_relation',?1,?2)",
                params![id,edge.to_string()]).map_err(|e|e.to_string())?;
        }
        // The evaluated state is committed with the validated candidate and
        // report. A retry can never consume a hypothesis without its output.
        for id in assessed_asymmetry_ids.iter().take(24){
            let raw:Option<String>=tx.query_row(
                "SELECT data FROM records WHERE kind='money_asymmetry' AND id=?1",
                [id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
            let Some(raw)=raw else{return Err(format!("referenced asymmetry missing: {id}"));};
            let mut record:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
            if record["status"]=="pending"{
                record["status"]=json!("evaluated");
                record["evaluatedJobId"]=report["jobId"].clone();
                record["evaluatedAt"]=json!(chrono::Utc::now().to_rfc3339());
                upsert_tx(&tx,"money_asymmetry",&record)?;
            }
        }
        upsert_tx(&tx,"money_report",report)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(true)
    }

    /// Ingest a batch of headlines and article excerpts from configured RSS
    /// feeds without model tokens. Every source retains original provenance.
    pub fn money_ingest_signals(&self,records:&[Value])->Result<usize,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let mut added=0;
        for signal in records {
            let id=signal["id"].as_str().ok_or("Signal id missing")?;
            let existing:Option<String>=tx.query_row(
                "SELECT data FROM records WHERE kind='money_signal' AND id=?1",
                [id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
            if let Some(existing)=existing{
                let mut stored:Value=serde_json::from_str(&existing).map_err(|e|e.to_string())?;
                let old_len=stored["articleExcerpt"].as_str().unwrap_or("").chars().count();
                let new_len=signal["articleExcerpt"].as_str().unwrap_or("").chars().count();
                // Preserve original capture time; never overwrite an existing body.
                if old_len<=100 && new_len>100 {
                    stored["articleExcerpt"]=signal["articleExcerpt"].clone();
                    stored["contentStatus"]=json!("page_fetched");
                    stored["lastFetchedAt"]=signal["capturedAt"].clone();
                    stored["foldStatus"]=json!("new");
                    upsert_tx(&tx,"money_signal",&stored)?;
                    refresh_money_source_fact_tx(&tx,&money_signal_fact(&stored))?;
                }
                continue;
            }
            let mut signal=signal.clone();
            signal["foldStatus"]=json!("new");
            upsert_tx(&tx,"money_signal",&signal)?;
            persist_money_fact_tx(&tx,&money_signal_fact(&signal))?;
            for edge in crate::engine::money_intelligence::signal_edges(&signal){
                let edge_id=edge["id"].as_str().ok_or("market graph id missing")?;
                tx.execute("INSERT OR IGNORE INTO records(kind,id,data) VALUES('money_relation',?1,?2)",
                    params![edge_id,edge.to_string()]).map_err(|e|e.to_string())?;
            }
            added+=1;
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(added)
    }

    /// Complete commercial ledger from SQLite, independent of dashboard
    /// sample limits. Never label these self-reported amounts as verified.
    /// Daily snapshot time series. A repeated ingest in the same UTC day
    /// updates the counter but never adds an identical sample. Only observed
    /// platform counters are accepted, not invented market prices.
    /// Only fetch sources never folded, or enriched with a longer page
    /// excerpt since folding. No full-corpus load; SQLite executes indexed
    /// anti-join on (kind,id). The fold marker stores the body-fetch version.
    pub fn money_unprocessed_source_count(&self)->Result<u64,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT COUNT(*) FROM records s
             WHERE s.kind='money_signal'
               AND COALESCE(json_extract(s.data,'$.foldStatus'),'new')='new'",
            [],|row|row.get(0)).map_err(|e|e.to_string())
    }
    pub fn money_unprocessed_sources(&self,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT s.data,s.id,s.created_at,s.updated_at FROM records s
             WHERE s.kind='money_signal'
               AND COALESCE(json_extract(s.data,'$.foldStatus'),'new')='new'
             ORDER BY s.created_at ASC LIMIT ?1").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([limit.clamp(1,128) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
    /// Events and a source's last-folded version commit together. Crash or
    /// retry cannot leave a folded source without its extracted events.
    pub fn money_fold_sources(&self,sources:&[String],events:&[Value])->Result<usize,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let mut added=0usize;
        for event in events{
            let id=event["id"].as_str().ok_or("event ID missing")?;
            let source=event["sourceId"].as_str().ok_or("event source missing")?;
            if !sources.iter().any(|s|s==source){
                return Err("event source was not selected for this batch".into());
            }
            let written=tx.execute(
                "INSERT OR IGNORE INTO records(kind,id,data) VALUES('money_event',?1,?2)",
                params![id,event.to_string()]).map_err(|e|e.to_string())?;
            added+=written;
        }
        for id in sources{
            let source:Option<String>=tx.query_row(
                "SELECT data FROM records WHERE kind='money_signal' AND id=?1",
                [id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
            let Some(raw)=source else{continue;};
            let source:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
            let mut updated=source.clone();
            updated["foldStatus"]=json!("folded");
            updated["lastFoldedAt"]=json!(chrono::Utc::now().to_rfc3339());
            upsert_tx(&tx,"money_signal",&updated)?;
            upsert_tx(&tx,"money_source_fold",&json!({
                "id":id,"lastFetchedAt":source["lastFetchedAt"],
                "sourceId":id,"foldedAt":chrono::Utc::now().to_rfc3339()
            }))?;
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(added)
    }

    /// Indexed entity join, not an all-pairs scan across arbitrary articles.
    /// The runtime only sees the bounded, grouped event working set.
    pub fn money_pair_events(&self,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT a.data,b.data FROM records a
             JOIN records b
             ON b.kind='money_event'
               AND json_extract(a.data,'$.entityKey')=json_extract(b.data,'$.entityKey')
               AND json_extract(a.data,'$.sourceName')<>json_extract(b.data,'$.sourceName')
               AND a.id<>b.id
             WHERE a.kind='money_event'
               AND json_extract(a.data,'$.status')='new'
             ORDER BY a.created_at DESC,b.created_at DESC LIMIT ?1")
                .map_err(|e|e.to_string())?;
        let rows=stmt.query_map([(limit.clamp(1,64)*8) as i64],|row|{
            let a:String=row.get(0)?;
            let b:String=row.get(1)?;
            Ok((a,b))
        }).map_err(|e|e.to_string())?;
        let mut material=std::collections::BTreeMap::<String,Value>::new();
        for row in rows{
            let (raw_a,raw_b)=row.map_err(|e|e.to_string())?;
            for raw in [raw_a,raw_b]{
                let record:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
                if let Some(id)=record["id"].as_str(){material.insert(id.to_string(),record);}
            }
        }
        Ok(crate::engine::money_asymmetry::discover(
            &material.into_values().collect::<Vec<_>>(),limit.clamp(1,32)))
    }

    /// Save pairs as PENDING questions only, and mark source events paired.
    /// A newly captured event can still join a historical paired event.
    pub fn money_store_asymmetries(&self,pairs:&[Value])->Result<usize,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let mut added=0usize;
        for value in pairs.iter().take(64){
            let id=value["id"].as_str().ok_or("asymmetry ID missing")?;
            let left=value["sourceA"].as_str().ok_or("left event ID missing")?;
            let right=value["sourceB"].as_str().ok_or("right event ID missing")?;
            if left==right{return Err("asymmetry cannot self-pair".into());}
            let mut to_save=value.clone();
            to_save["status"]=json!("pending");
            let inserted=tx.execute(
                "INSERT OR IGNORE INTO records(kind,id,data) VALUES('money_asymmetry',?1,?2)",
                params![id,to_save.to_string()]).map_err(|e|e.to_string())?;
            if inserted==0{continue;}
            added+=1;
            for event_id in [left,right]{
                let raw:Option<String>=tx.query_row(
                    "SELECT data FROM records WHERE kind='money_event' AND id=?1",
                    [event_id],|row|row.get(0))
                    .optional().map_err(|e|e.to_string())?;
                if let Some(raw)=raw{
                    let mut event:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
                    if event["status"]=="new"{
                        event["status"]=json!("paired");
                        upsert_tx(&tx,"money_event",&event)?;
                    }
                }
            }
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(added)
    }
    pub fn money_pending_asymmetries(&self,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_asymmetry' AND json_extract(data,'$.status')='pending'
             ORDER BY CAST(json_extract(data,'$.researchPriority') AS INTEGER) DESC,
                      created_at ASC LIMIT ?1").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([limit.clamp(1,30) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
    /// Report-driven state transition. Pending -> evaluated is forward-only,
    /// scoped to the exact IDs observed in a successful completed report.
    pub fn money_resolve_asymmetries(&self,ids:&[String],job_id:&str)->Result<usize,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let mut moved=0;
        for id in ids.iter().take(30){
            let raw:Option<String>=tx.query_row(
                "SELECT data FROM records WHERE kind='money_asymmetry' AND id=?1",
                [id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
            let Some(raw)=raw else{continue;};
            let mut record:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
            if record["status"]!="pending"{continue;}
            record["status"]=json!("evaluated");
            record["evaluatedJobId"]=json!(job_id);
            record["evaluatedAt"]=json!(chrono::Utc::now().to_rfc3339());
            upsert_tx(&tx,"money_asymmetry",&record)?;
            moved+=1;
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(moved)
    }

    pub fn money_store_market_metrics(&self,records:&[Value])->Result<usize,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let mut added=0usize;
        for value in records.iter().take(200){
            let entity=value["entityId"].as_str().ok_or("metric entity missing")?;
            let name=value["metric"].as_str().ok_or("metric name missing")?;
            let id=value["id"].as_str().ok_or("metric id missing")?;
            let numeric=value["value"].as_u64().ok_or("metric counter missing")?;
            if !matches!(name,"stars"|"rolling_downloads"|"likes")||entity.is_empty() {
                return Err("unsupported market counter".into());
            }
            let previous:Option<Value>=tx.query_row(
                "SELECT data FROM records WHERE kind='money_metric'
                 AND json_extract(data,'$.entityId')=?1
                 AND json_extract(data,'$.metric')=?2
                 AND id<>?3 ORDER BY json_extract(data,'$.observedAt') DESC LIMIT 1",
                params![entity,name,id],|row|row.get::<_,String>(0))
                .optional().map_err(|e|e.to_string())?
                .and_then(|raw|serde_json::from_str::<Value>(&raw).ok());
            let present:Option<String>=tx.query_row(
                "SELECT id FROM records WHERE kind='money_metric' AND id=?1",
                [id],|row|row.get(0)).optional().map_err(|e|e.to_string())?;
            if present.is_none(){added+=1;}
            upsert_tx(&tx,"money_metric",value)?;
            if let Some(prior)=previous{
                if let Some(previous_value)=prior["value"].as_u64(){
                    let delta=numeric as i128-previous_value as i128;
                    let fraction=(numeric as f64+1.0)/(previous_value as f64+1.0)-1.0;
                    let change=json!({"id":format!("change:{id}"),
                        "entityId":entity,"metric":name,
                        "latestValue":numeric,"previousValue":previous_value,
                        "absoluteDelta":delta as i64,"relativeDeltaPct":(fraction*1000.0).round()/10.0,
                        "fromObservedAt":prior["observedAt"],"observedAt":value["observedAt"],
                        "url":value["url"],
                        "verification":"platform_metric_change_not_purchase_demand"});
                    upsert_tx(&tx,"money_metric_change",&change)?;
                }
            }
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(added)
    }
    pub fn money_recent_metric_changes(&self,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='money_metric_change'
             ORDER BY updated_at DESC LIMIT ?1").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([limit.clamp(1,200) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
    /// Materialize ONLY a top-K researched cross-market set. This is an
    /// indexable, replay-safe evidence ledger, not a list of verified profits.
    pub fn money_store_tensions(&self,tensions:&[Value])->Result<usize,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let mut added=0;
        for value in tensions.iter().take(60){
            let id=value["id"].as_str().ok_or("tension id missing")?;
            let source=value["supplySignalId"].as_str().ok_or("supply evidence missing")?;
            let buyer=value["buyerSignalId"].as_str().ok_or("buyer evidence missing")?;
            if source==buyer||value["purchaseEvidence"]!=false{
                return Err("tension cannot claim verified purchase".into());
            }
            added+=tx.execute(
                "INSERT OR IGNORE INTO records(kind,id,data) VALUES('money_tension',?1,?2)",
                params![id,value.to_string()]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(added)
    }
    pub fn money_best_tensions(&self,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut statement=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_tension'
             ORDER BY CAST(json_extract(data,'$.researchPriority') AS INTEGER) DESC,
                      updated_at DESC LIMIT ?1").map_err(|e|e.to_string())?;
        let rows=statement.query_map([limit.clamp(1,100) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    pub fn money_candidate_ledger(&self,candidate:&str)->Result<Value,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT COUNT(*),
             COALESCE(SUM(CAST(json_extract(data,'$.reportedRevenueWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.reportedCostWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.marketingCostWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.modelCostWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.laborCostWon') AS REAL)),0)
             FROM records WHERE kind='money_experiment'
             AND json_extract(data,'$.candidateId')=?1",
            [candidate],|row|{
                let count=row.get::<_,u64>(0)?;
                let revenue=row.get::<_,f64>(1)?;
                let total_cost=(2..=5).map(|i|row.get::<_,f64>(i)).collect::<Result<Vec<_>,_>>()?
                    .iter().sum::<f64>();
                Ok(json!({"experiments":count,"reportedRevenueWon":revenue,
                    "reportedCostWon":total_cost,"reportedNetWon":revenue-total_cost,
                    "source":"user_reported_not_audited"}))
            }).map_err(|e|e.to_string())
    }

    pub fn money_ledger_totals(&self,campaign:&str)->Result<Value,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT COUNT(*),
             COALESCE(SUM(CAST(json_extract(data,'$.reportedRevenueWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.reportedCostWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.marketingCostWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.modelCostWon') AS REAL)),0),
             COALESCE(SUM(CAST(json_extract(data,'$.laborCostWon') AS REAL)),0),
             COALESCE(SUM(CASE WHEN json_extract(data,'$.status')='finished' THEN 1 ELSE 0 END),0)
             FROM records WHERE kind='money_experiment'
                 AND json_extract(data,'$.campaignId')=?1",
            [campaign],|row|{
                Ok(json!({"experiments":row.get::<_,i64>(0)?,
                    "revenue":row.get::<_,f64>(1)?,"direct":row.get::<_,f64>(2)?,
                    "marketing":row.get::<_,f64>(3)?,"model":row.get::<_,f64>(4)?,
                    "labor":row.get::<_,f64>(5)?,"finished":row.get::<_,i64>(6)?}))
            }).map_err(|e|e.to_string())
    }

    /// Last 14 days of SOURCE MENTIONS; not customer demand or measured revenue.
    /// This stays in SQLite, never materializes the whole corpus in memory.
    pub fn money_topic_trends(&self)->Result<Vec<Value>,String>{
        let now=chrono::Utc::now();
        let recent=(now-chrono::Duration::days(7)).to_rfc3339();
        let earlier=(now-chrono::Duration::days(14)).to_rfc3339();
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut statement=conn.prepare(
            "SELECT j.value,
                SUM(CASE WHEN json_extract(r.data,'$.capturedAt')>=?1 THEN 1 ELSE 0 END),
                SUM(CASE WHEN json_extract(r.data,'$.capturedAt')<?1 THEN 1 ELSE 0 END),
                COUNT(DISTINCT json_extract(r.data,'$.sourceName'))
             FROM records r, json_each(r.data,'$.topics') j
             WHERE r.kind='money_signal'
               AND json_extract(r.data,'$.capturedAt')>=?2
             GROUP BY j.value ORDER BY 2 DESC, 1 ASC LIMIT 15")
             .map_err(|e|e.to_string())?;
        let rows=statement.query_map(params![recent,earlier],|row|{
            let topic:String=row.get(0)?;
            let current:i64=row.get(1)?;
            let previous:i64=row.get(2)?;
            let source_count:i64=row.get(3)?;
            // Explicit mention change with a smoothed prior. This is NOT a
            // price or demand growth rate.
            let delta=current-previous;
            Ok(json!({"topic":topic,"current7d":current,"previous7d":previous,
                "mentionDelta":delta,"sourceDiversity":source_count,
                "observation":"news_mention_frequency_not_demand",
                "relativeChange":((current as f64+1.0)/(previous as f64+1.0)*100.0).round()/100.0}))
        }).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Indexed one-hop graph traversal. Useful for discovering that sectors
    /// from different articles or inferred products share a relationship.
    pub fn money_graph_neighbors(&self,seeds:&[String],limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records
             WHERE kind='money_relation'
             AND (json_extract(data,'$.from')=?1 OR json_extract(data,'$.to')=?1)
             ORDER BY updated_at DESC LIMIT ?2").map_err(|e|e.to_string())?;
        let mut result=Vec::new();
        let mut seen=std::collections::HashSet::new();
        for seed in seeds.iter().take(6){
            let found=stmt.query_map(params![seed,limit.clamp(1,50) as i64],record_from_row)
                .map_err(|e|e.to_string())?;
            for edge in found{
                let edge=edge.map_err(|e|e.to_string())?;
                if let Some(id)=edge["id"].as_str(){
                    if seen.insert(id.to_string()){result.push(edge);}
                }
                if result.len()>=limit.clamp(1,100){return Ok(result);}
            }
        }
        Ok(result)
    }
    pub fn money_graph_recent(&self,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='money_relation'
             ORDER BY updated_at DESC LIMIT ?1").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([limit.clamp(1,200) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    pub fn money_corpus_stats(&self)->Result<Value,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut values=serde_json::Map::new();
        for kind in ["money_fact","money_signal","money_candidate","money_experiment","money_relation","money_tension","money_metric"]{
            let count=conn.query_row("SELECT COUNT(*) FROM records WHERE kind=?1",
                [kind],|row|row.get::<_,u64>(0)).map_err(|e|e.to_string())?;
            values.insert(kind.to_string(),json!(count));
        }
        Ok(Value::Object(values))
    }
    /// No full-corpus materialization: measurable 24-hour Money funnel from
    /// the existing indexed SQLite job, research and user-outcome records.
    /// The observation window is not proof of a continuously running model.
    pub fn money_operational_scorecard(&self)->Result<Value,String>{
        let cutoff=(chrono::Utc::now()-chrono::Duration::hours(24))
            .format("%Y-%m-%d %H:%M:%S").to_string();
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let jobs:(i64,i64,i64,i64,i64,i64)=conn.query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN json_extract(data,'$.status')='completed' THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN json_extract(data,'$.status')='failed' THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN json_extract(data,'$.status')='needs_review' THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN json_extract(data,'$.status')='queued' THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN json_extract(data,'$.status')='running' THEN 1 ELSE 0 END),0)
             FROM records WHERE kind='job' AND updated_at>=?1 AND
             json_extract(data,'$.kind') IN
             ('money_discover','money_ingest','money_catalog','money_fold')",
            [&cutoff],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))
        ).map_err(|e|e.to_string())?;
        let ingestion:(i64,i64,i64,i64)=conn.query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CAST(json_extract(data,'$.found') AS INTEGER)),0),
                COALESCE(SUM(CAST(json_extract(data,'$.new') AS INTEGER)),0),
                COALESCE(SUM(CAST(json_extract(data,'$.scrapedPages') AS INTEGER)),0)
             FROM records WHERE kind='money_ingest_report' AND created_at>=?1",
            [&cutoff],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))
        ).map_err(|e|e.to_string())?;
        let catalog:(i64,i64)=conn.query_row(
            "SELECT COUNT(*),COALESCE(SUM(CAST(json_extract(data,'$.importedSignals') AS INTEGER)),0)
             FROM records WHERE kind='money_catalog_report' AND created_at>=?1",
            [&cutoff],|r|Ok((r.get(0)?,r.get(1)?))
        ).map_err(|e|e.to_string())?;
        let sources:(i64,i64)=conn.query_row(
            "SELECT COUNT(*),COALESCE(SUM(CASE WHEN
                 json_extract(data,'$.contentStatus')='page_fetched' THEN 1 ELSE 0 END),0)
             FROM records WHERE kind='money_signal' AND created_at>=?1",
            [&cutoff],|r|Ok((r.get(0)?,r.get(1)?))
        ).map_err(|e|e.to_string())?;
        let research:(i64,i64,i64)=conn.query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN json_extract(data,'$.parseError') IS NOT NULL THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CAST(json_extract(data,'$.matchedBuyerQuoteCount') AS INTEGER)),0)
             FROM records WHERE kind='money_report' AND created_at>=?1",
            [&cutoff],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))
        ).map_err(|e|e.to_string())?;
        let candidates:i64=conn.query_row(
            "SELECT COUNT(*) FROM records WHERE kind='money_candidate' AND created_at>=?1",
            [&cutoff],|r|r.get(0)).map_err(|e|e.to_string())?;
        let fetches:(i64,i64)=conn.query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN json_extract(data,'$.status')='archived'
                    THEN 1 ELSE 0 END),0)
             FROM records WHERE kind='money_buyer_fetch' AND updated_at>=?1",
            [&cutoff],|r|Ok((r.get(0)?,r.get(1)?))
        ).map_err(|e|e.to_string())?;
        let outcomes:(i64,f64,f64)=conn.query_row(
            "SELECT COUNT(*),
              COALESCE(SUM(CAST(json_extract(data,'$.reportedRevenueWon') AS REAL)),0),
              COALESCE(SUM(
                COALESCE(CAST(json_extract(data,'$.reportedCostWon') AS REAL),0)
               +COALESCE(CAST(json_extract(data,'$.marketingCostWon') AS REAL),0)
               +COALESCE(CAST(json_extract(data,'$.modelCostWon') AS REAL),0)
               +COALESCE(CAST(json_extract(data,'$.laborCostWon') AS REAL),0)),0)
             FROM records WHERE kind='money_experiment' AND created_at>=?1
                AND json_extract(data,'$.status')='finished'",
            [&cutoff],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))
        ).map_err(|e|e.to_string())?;
        let heartbeats:i64=conn.query_row(
            "SELECT COUNT(*) FROM records WHERE kind='money_heartbeat' AND created_at>=?1",
            [&cutoff],|r|r.get(0)).map_err(|e|e.to_string())?;
        let terminal=jobs.1+jobs.2+jobs.3;
        let completed_pct=if terminal>0{
            json!(((jobs.1 as f64/terminal as f64)*1000.0).round()/10.0)
        }else{Value::Null};
        // 288 five-minute buckets in 24h. Missing buckets are observable,
        // but process samples do not prove continuous model inference.
        let coverage=if heartbeats>0{
            json!(((heartbeats.min(288) as f64/288.0)*1000.0).round()/10.0)
        }else{Value::Null};
        Ok(json!({
            "windowHours":24,"computedAt":chrono::Utc::now().to_rfc3339(),
            "recordSource":"persisted_sqlite_not_live_runtime_probe",
            "jobs":{"tracked":jobs.0,"completed":jobs.1,"failed":jobs.2,
                "needsReview":jobs.3,"queued":jobs.4,"running":jobs.5,
                "terminalCompletionPct":completed_pct},
            "sources":{"rssScans":ingestion.0,"headlinesSeen":ingestion.1,
                "newRssRecords":ingestion.2,"scrapedPages":ingestion.3,
                "catalogScans":catalog.0,"newCatalogRecords":catalog.1,
                "newArchiveRecords":sources.0,"fetchedArchivePages":sources.1},
            "research":{"reports":research.0,"parseErrors":research.1,
                "candidates":candidates,"sourceMatchedBuyerQuotes":research.2,
                "buyerFetchAttempts":fetches.0,"buyerDocumentsArchived":fetches.1},
            "userReportedOutcomes":{"finishedExperiments":outcomes.0,
                "revenueWon":outcomes.1,"totalCostWon":outcomes.2,
                "netWon":outcomes.1-outcomes.2,
                "source":"user_reported_not_payment_verified"},
            "processHeartbeatSamples":heartbeats,
            "expectedHeartbeatSamples":288,
            "processSampleCoveragePct":coverage,
            "modelUptimeVerified":false,
            "modelTokensMeasured":false,
            "independentPaymentVerification":false
        }))
    }

    pub fn money_latest_ingestion(&self)->Result<Option<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='money_ingest_report'
             ORDER BY created_at DESC LIMIT 1",[],record_from_row)
            .optional().map_err(|e|e.to_string())
    }

    pub fn money_recent_records(&self,kind:&str,limit:usize)->Result<Vec<Value>,String>{
        if !matches!(kind,"money_candidate"|"money_experiment"|"money_report"|"money_campaign"){
            return Err("unsupported money record collection".into());
        }
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1
             ORDER BY updated_at DESC LIMIT ?2").map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![kind,limit.clamp(1,500) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Exact row count for a kind without deserializing every record.
    pub fn count_kind(&self,kind:&str)->usize{
        let Ok(conn)=self.conn.lock() else{return 0;};
        conn.query_row("SELECT COUNT(*) FROM records WHERE kind=?1",[kind],
            |row|row.get::<_,i64>(0)).map(|n|n.max(0) as usize).unwrap_or(0)
    }

    pub fn money_recent_signals(&self,limit:usize)->Result<Vec<Value>,String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let mut stmt=conn.prepare(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='money_signal'
             ORDER BY created_at DESC LIMIT ?1")
            .map_err(|e|e.to_string())?;
        let rows=stmt.query_map([limit.clamp(1,300) as i64],record_from_row)
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }


}
