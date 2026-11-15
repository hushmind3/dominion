//! First-party durable sequential workflows.
//! A workflow is *only* a sequence of owner-authored native TaskInputs.
//! All model turns, tool dispatch, permissions and completion use DominionSdk.
use crate::{runtime::{DominionSdk,TaskInput},store::Store};
use rusqlite::params;
use serde_json::{json,Value};
use std::time::Duration;
use uuid::Uuid;

fn record_workflow(row:&rusqlite::Row<'_>)->rusqlite::Result<Value>{
    let steps:String=row.get(2)?;
    Ok(json!({"id":row.get::<_,String>(0)?,
        "name":row.get::<_,String>(1)?,
        "steps":serde_json::from_str::<Value>(&steps).unwrap_or(Value::Null),
        "createdAt":row.get::<_,String>(3)?}))
}
fn record_run(row:&rusqlite::Row<'_>)->rusqlite::Result<Value>{
    let tasks:String=row.get(6)?;
    let options:String=row.get(3)?;
    Ok(json!({"id":row.get::<_,String>(0)?,"workflowId":row.get::<_,String>(1)?,
        "requestId":row.get::<_,String>(2)?,
        "options":serde_json::from_str::<Value>(&options).unwrap_or(Value::Null),
        "status":row.get::<_,String>(4)?,"currentStep":row.get::<_,i64>(5)?,
        "tasks":serde_json::from_str::<Value>(&tasks).unwrap_or(Value::Null),
        "error":row.get::<_,Option<String>>(7)?,
        "createdAt":row.get::<_,String>(8)?,"updatedAt":row.get::<_,String>(9)?}))
}
impl Store {
    pub fn workflow_create(&self,name:&str,steps:&[String])->Result<Value,String>{
        let name=name.trim();
        if name.is_empty() || name.len()>100 || steps.is_empty() || steps.len()>12 ||
            steps.iter().any(|p|p.trim().is_empty()||p.len()>8000) {
            return Err("Workflow requires a name and 1..12 bounded owner steps".into());
        }
        let id=Uuid::new_v4().to_string();
        let at=chrono::Utc::now().to_rfc3339();
        let input=json!(steps.iter().map(String::as_str).collect::<Vec<_>>());
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute("INSERT INTO workflows(id,name,steps,created_at) VALUES(?1,?2,?3,?4)",
            params![id,name,input.to_string(),at]).map_err(|e|e.to_string())?;
        Ok(json!({"id":id,"name":name,"steps":steps,"createdAt":at}))
    }
    pub fn workflows(&self)->Vec<Value>{
        let Ok(conn)=self.conn.lock() else{return vec![]};
        let Ok(mut statement)=conn.prepare(
            "SELECT id,name,steps,created_at FROM workflows ORDER BY created_at DESC LIMIT 100"
        ) else{return vec![]};
        let Ok(rows)=statement.query_map([],record_workflow) else{return vec![]};
        rows.filter_map(Result::ok).collect()
    }
    pub fn workflow_get(&self,id:&str)->Option<Value>{
        let conn=self.conn.lock().ok()?;
        conn.query_row("SELECT id,name,steps,created_at FROM workflows WHERE id=?1",
            [id],record_workflow).ok()
    }
    pub fn workflow_runs(&self,id:&str)->Vec<Value>{
        let Ok(conn)=self.conn.lock() else{return vec![]};
        let Ok(mut statement)=conn.prepare("SELECT id,workflow_id,request_id,options,status,current_step,
                    tasks,error,created_at,updated_at FROM workflow_runs
                    WHERE workflow_id=?1 ORDER BY created_at DESC LIMIT 40")
            else{return vec![]};
        let Ok(rows)=statement.query_map([id],record_run) else{return vec![]};
        rows.filter_map(Result::ok).collect()
    }
    pub fn workflow_run(&self,id:&str)->Option<Value>{
        let conn=self.conn.lock().ok()?;
        conn.query_row("SELECT id,workflow_id,request_id,options,status,current_step,
                tasks,error,created_at,updated_at FROM workflow_runs WHERE id=?1",
            [id],record_run).ok()
    }
    pub fn workflow_admit(&self,id:&str,request_id:&str,options:&Value)
        ->Result<(Value,bool),String>{
        if !(8..=128).contains(&request_id.len()) ||
            !request_id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_')){
            return Err("Invalid workflow run request key".into());
        }
        let workflow=self.workflow_get(id).ok_or("Workflow not found")?;
        let key=Uuid::new_v4().to_string();
        let now=chrono::Utc::now().to_rfc3339();
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        let created=conn.execute(
            "INSERT OR IGNORE INTO workflow_runs
             (id,workflow_id,request_id,options,status,created_at,updated_at)
             VALUES(?1,?2,?3,?4,'queued',?5,?5)",
            params![key,id,request_id,options.to_string(),now]
        ).map_err(|e|e.to_string())?==1;
        let run=conn.query_row(
            "SELECT id,workflow_id,request_id,options,status,current_step,
                tasks,error,created_at,updated_at FROM workflow_runs WHERE request_id=?1",
            [request_id],record_run).map_err(|e|e.to_string())?;
        if run["workflowId"]!=workflow["id"]||run["options"]!=*options {
            return Err("Request key was reused with different workflow parameters".into());
        }
        Ok((run,created))
    }
    pub fn workflow_progress(&self,id:&str,status:&str,index:usize,tasks:&Value,
        error:Option<&str>)->Result<(),String>{
        if !matches!(status,"running"|"completed"|"failed"|"needs_review"){
            return Err("Invalid workflow status".into());
        }
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute(
            "UPDATE workflow_runs SET status=?2,current_step=?3,tasks=?4,error=?5,
             updated_at=?6 WHERE id=?1 AND status IN('queued','running')",
            params![id,status,index as i64,tasks.to_string(),error,chrono::Utc::now().to_rfc3339()]
        ).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn workflow_recover(&self)->Result<(),String>{
        let conn=self.conn.lock().map_err(|e|e.to_string())?;
        conn.execute("UPDATE workflow_runs
            SET status='needs_review',
                error='Workflow interrupted; inspect native task records before starting again'
            WHERE status IN ('running','queued')",[])
            .map_err(|e|e.to_string())?;
        Ok(())
    }
}
/// Parse and validate run-level owner permissions. There is no implicit
/// shell, write or MCP authority in a stored workflow definition.
fn run_options(raw:&Value)->Result<TaskInput,String>{
    let mut payload=raw.clone();
    payload["prompt"]=json!("workflow step");
    let mut options:TaskInput=serde_json::from_value(payload)
        .map_err(|e|e.to_string())?;
    options.prompt="workflow step".into();
    options.request_id=None;
    options.session_id=None;
    options.allow_subagents=false;
    options.validate().map_err(|e|e.to_string())?;
    Ok(options)
}
impl DominionSdk {
    pub fn workflow_create(&self,name:&str,steps:&[String])->Result<Value,String>{
        self.state.store.workflow_create(name,steps)
    }
    pub fn workflows(&self)->Vec<Value>{self.state.store.workflows()}
    pub fn workflow_runs(&self,id:&str)->Vec<Value>{self.state.store.workflow_runs(id)}
    pub fn workflow_run(&self,id:&str)->Option<Value>{self.state.store.workflow_run(id)}

    pub async fn workflow_start(&self,id:&str,request_id:&str,raw:Value)
        ->Result<Value,String>{
        let options=run_options(&raw)?;
        let stored_options=json!({
            "allowWrite":options.allow_write,
            "allowShell":options.allow_shell,
            "allowMcp":options.allow_mcp,
            "requireEvidence":options.require_evidence,
            "modelTier":options.model_tier,
            "planningMode":options.planning_mode,
            "tokenBudget":options.token_budget,
            "maxSteps":options.max_steps,
            "costBudgetKrw":options.cost_budget_krw,
            "agentId":options.agent_id
        });
        let (run,created)=self.state.store.workflow_admit(id,request_id,&stored_options)?;
        if created {
            let sdk=self.clone();
            let id=run["id"].as_str().unwrap_or_default().to_string();
            tokio::spawn(async move {sdk.workflow_execute(&id).await;});
        }
        Ok(run)
    }
    async fn workflow_execute(&self,id:&str){
        let Some(run)=self.state.store.workflow_run(id) else{return};
        let Some(workflow)=self.state.store.workflow_get(
            run["workflowId"].as_str().unwrap_or("")
        ) else {return};
        let mut tasks=Vec::<Value>::new();
        let steps=workflow["steps"].as_array().cloned().unwrap_or_default();
        for (index,step) in steps.iter().enumerate(){
            let prompt=step.as_str().unwrap_or("").to_string();
            let key=format!("workflow-{}-{}",id,index);
            let data=run["options"].clone();
            let Ok(mut input)=run_options(&data) else {break};
            input.prompt=prompt;
            input.request_id=Some(key);
            input.session_id=Some(format!("workflow-{id}"));
            // Persist the state transition before the next native model turn.
            if self.state.store.workflow_progress(id,"running",index,&json!(tasks),None).is_err(){
                return;
            }
            let result=match self.submit(input).await{
                Ok(record)=>record,
                Err(e)=>{
                    let _=self.state.store.workflow_progress(id,"failed",index,&json!(tasks),
                        Some(&e.to_string()));
                    return;
                }
            };
            tasks.push(json!(result.id));
            let finished=self.wait(&result.id,Duration::from_secs(900)).await;
            match finished {
                Ok(done) if done.status=="completed"=>{},
                Ok(done)=>{
                    let message=done.error.unwrap_or_else(||format!("Step {} ended {}",index+1,done.status));
                    let _=self.state.store.workflow_progress(id,"needs_review",index,&json!(tasks),Some(&message));
                    return;
                }
                Err(e)=>{
                    let _=self.state.store.workflow_progress(id,"needs_review",index,&json!(tasks),
                        Some(&e.to_string()));
                    return;
                }
            }
        }
        let _=self.state.store.workflow_progress(id,"completed",steps.len(),&json!(tasks),None);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn workflow_definition_is_durable_and_run_idempotent(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        let flow=store.workflow_create("review",&["Read README".into(),"Summarize findings".into()]).unwrap();
        let id=flow["id"].as_str().unwrap();
        let (one,created)=store.workflow_admit(id,"repeat-workflow-2026",&json!({"allowWrite":false})).unwrap();
        assert!(created);
        let (again,created)=store.workflow_admit(id,"repeat-workflow-2026",&json!({"allowWrite":false})).unwrap();
        assert!(!created);assert_eq!(one["id"],again["id"]);
        assert!(store.workflow_admit(id,"repeat-workflow-2026",&json!({"allowWrite":true})).is_err());
        assert_eq!(store.workflow_runs(id).len(),1);
        store.workflow_recover().unwrap();
        assert_eq!(store.workflow_run(one["id"].as_str().unwrap()).unwrap()["status"],"needs_review");
    }
}
