//! Bounded, durable, same-evaluator code evolution. A chain runs only after
//! a separate explicit owner authorization; no model can lower its scorer or
//! expand its file targets. Each approved generation passes the same frozen
//! test/score commands before it becomes its successor's baseline.
use super::{git,now_ms,relative,Improvement,ImprovementStatus,Improvements,NewImprovement};
use crate::provider::ModelClient;
use serde::{Deserialize,Serialize};
use std::{collections::HashMap,path::Path,sync::Arc};
use tokio::fs;
use uuid::Uuid;
mod tournament;

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum EvolutionStatus { Running, Completed, NeedsReview, Failed }

#[derive(Clone,Debug,Deserialize)]
pub struct NewEvolution {
    pub max_generations:usize,
    pub cost_budget_krw:f64,
    #[serde(default)] pub target_files:Vec<String>,
    #[serde(default="super::default_candidate_count")] pub candidates_per_generation:usize,
    #[serde(default)] pub validation_command:Option<String>,
}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CandidateRecord {
    pub trial_id:Uuid,
    pub generation:usize,
    pub proposal_index:usize,
    pub training_score:Option<f64>,
    pub validation_baseline_score:Option<f64>,
    pub validation_score:Option<f64>,
    pub eligible:bool,
    pub reason:Option<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct EvolutionRun {
    pub id:Uuid,
    pub starting_trial:Uuid,
    pub status:EvolutionStatus,
    pub max_generations:usize,
    pub target_files:Vec<String>,
    #[serde(default="super::default_candidate_count")] pub candidates_per_generation:usize,
    #[serde(default)] pub validation_command:Option<String>,
    #[serde(default)] pub candidate_groups:Vec<Vec<Uuid>>,
    #[serde(default)] pub candidates:Vec<CandidateRecord>,
    pub descendants:Vec<Uuid>,
    pub completed_generations:usize,
    pub cost_budget_krw:f64,
    pub spent_krw:f64,
    #[serde(default)] pub stop_requested:bool,
    pub stop_reason:Option<String>,
    pub created_at_ms:u64,
    pub updated_at_ms:u64,
}

pub(super) async fn load(root:&Path)->Result<HashMap<Uuid,EvolutionRun>,String>{
    let dir=root.join("evolutions");
    fs::create_dir_all(&dir).await.map_err(|e|e.to_string())?;
    let mut entries=fs::read_dir(dir).await.map_err(|e|e.to_string())?;
    let mut runs=HashMap::new();
    while let Some(entry)=entries.next_entry().await.map_err(|e|e.to_string())? {
        if entry.path().extension().and_then(|s|s.to_str())!=Some("json"){continue;}
        let mut run:EvolutionRun=serde_json::from_slice(
            &fs::read(entry.path()).await.map_err(|e|e.to_string())?
        ).map_err(|e|e.to_string())?;
        if run.status==EvolutionStatus::Running {
            // No crash-time replay of unknown Git effects, shells, or paid calls.
            run.status=EvolutionStatus::NeedsReview;
            run.stop_reason=Some("Evolution interrupted; inspect committed descendants before a new run".into());
            run.updated_at_ms=now_ms();
            save(root,&run).await?;
        }
        runs.insert(run.id,run);
    }
    Ok(runs)
}

async fn save(root:&Path,run:&EvolutionRun)->Result<(),String>{
    let dir=root.join("evolutions");
    let temp=dir.join(format!("{}.tmp",run.id));
    let destination=dir.join(format!("{}.json",run.id));
    use tokio::io::AsyncWriteExt;
    let bytes=serde_json::to_vec(run).map_err(|e|e.to_string())?;
    let mut file=fs::OpenOptions::new().create(true).truncate(true)
        .write(true).open(&temp).await.map_err(|e|e.to_string())?;
    file.write_all(&bytes).await.map_err(|e|e.to_string())?;
    file.sync_all().await.map_err(|e|e.to_string())?;
    drop(file);
    fs::rename(&temp,&destination).await.map_err(|e|e.to_string())?;
    #[cfg(unix)]
    std::fs::File::open(&dir).and_then(|file|file.sync_all())
        .map_err(|e|e.to_string())?;
    Ok(())
}

impl Improvements {
    async fn put_evolution(&self,mut run:EvolutionRun)->Result<EvolutionRun,String>{
        // Serialize disk+memory updates; never erase a newer stop request
        // with a worker's older progress snapshot.
        let mut records=self.lineages.lock().await;
        if let Some(previous)=records.get(&run.id){
            run.stop_requested|=previous.stop_requested;
            if previous.status!=EvolutionStatus::Running && run.status==EvolutionStatus::Running{
                return Err("Cannot overwrite terminal evolution with running progress".into());
            }
        }
        save(&self.root,&run).await?;
        records.insert(run.id,run.clone());
        Ok(run)
    }
    pub async fn evolution_stop(&self,id:Uuid)->Result<EvolutionRun,String>{
        let mut records=self.lineages.lock().await;
        let mut run=records.get(&id).cloned().ok_or("Evolution not found")?;
        if run.status!=EvolutionStatus::Running{return Ok(run);}
        run.stop_requested=true;
        run.updated_at_ms=now_ms();
        save(&self.root,&run).await?;
        records.insert(run.id,run.clone());
        Ok(run)
    }
    async fn should_stop_evolution(&self,run:&mut EvolutionRun)->bool{
        let requested=self.lineages.lock().await.get(&run.id)
            .is_some_and(|stored|stored.stop_requested);
        if requested {
            run.stop_requested=true;
            run.status=EvolutionStatus::NeedsReview;
            run.stop_reason=Some("Owner requested stop; no further candidate is adopted".into());
        }
        requested
    }
    pub async fn evolutions(&self)->Vec<EvolutionRun>{
        let mut list:Vec<_>=self.lineages.lock().await.values().cloned().collect();
        list.sort_by_key(|run|std::cmp::Reverse(run.created_at_ms));
        list
    }
    pub async fn evolution(&self,id:Uuid)->Option<EvolutionRun>{
        self.lineages.lock().await.get(&id).cloned()
    }

    /// A single explicit owner action authorizes 1..4 generations against a
    /// fixed scoring contract. Files may alternate A -> B -> A, but all stages
    /// use the same repository, objective, tests and scoring command.
    pub async fn evolution_start(
        self:&Arc<Self>, model:ModelClient, parent_id:Uuid, request:NewEvolution,
        shell_enabled:bool
    )->Result<EvolutionRun,String>{
        if !shell_enabled{return Err("DOMINION_ENABLE_SHELL=true is required for approved evolution".into());}
        if !model.can_meter_paid_cost(){
            return Err("Remote model token KRW rates must be configured to meter evolution spending".into());
        }
        if !(1..=4).contains(&request.max_generations){
            return Err("Evolution must request 1..4 bounded generations".into());
        }
        if !(1..=4).contains(&request.candidates_per_generation) {
            return Err("Each generation must evaluate 1..4 candidate proposals".into());
        }
        if let Some(command)=&request.validation_command {
            if command.trim().is_empty()||command.len()>2048 {
                return Err("Validation score command must be 1..2048 characters".into());
            }
        }
        if !request.cost_budget_krw.is_finite() ||
            !(1.0..=1_000_000.0).contains(&request.cost_budget_krw){
            return Err("Evolution requires an explicit positive KRW cost ceiling".into());
        }
        let parent=self.get(parent_id).await.ok_or("Evolution parent not found")?;
        if parent.status!=ImprovementStatus::Adopted{
            return Err("Evolution requires an independently evaluated and adopted seed".into());
        }
        if parent.score_command.is_none(){
            return Err("Recursive evolution needs a frozen numeric score command".into());
        }
        let targets=if request.target_files.is_empty(){
            vec![parent.target_file.clone();request.max_generations]
        }else{
            if request.target_files.len()!=request.max_generations {
                return Err("One explicit target file is needed for each generation".into());
            }
            request.target_files
        };
        let workspace=self.workspace.canonicalize().map_err(|e|e.to_string())?;
        let repo=workspace.join(relative(&parent.repository)?).canonicalize().map_err(|e|e.to_string())?;
        if !repo.starts_with(&workspace){
            return Err("Evolution repository escapes the configured workspace".into());
        }
        let checkout=git(&repo,&["rev-parse","HEAD"]).await?;
        if parent.adopted_commit.as_deref()!=Some(checkout.trim()){
            return Err("Seed checkout moved since adoption; refusing unverified lineage".into());
        }
        if !git(&repo,&["status","--porcelain"]).await?.is_empty(){
            return Err("Evolution requires a clean owner checkout".into());
        }
        for name in &targets {
            let file=repo.join(relative(name)?).canonicalize().map_err(|e|format!("Evolution target {name}: {e}"))?;
            if !file.starts_with(&repo) || !file.is_file(){
                return Err(format!("Evolution target is not a regular source file inside checkout: {name}"));
            }
        }
        {
            let mut leased=self.repository_active.lock().await;
            if !leased.insert(parent.repository.clone()){
                return Err("Repository already has an active evaluation or evolution".into());
            }
        }
        let now=now_ms();
        let run=EvolutionRun{id:Uuid::new_v4(),starting_trial:parent_id,
            status:EvolutionStatus::Running,max_generations:request.max_generations,
            target_files:targets,candidates_per_generation:request.candidates_per_generation,
            validation_command:request.validation_command,
            candidate_groups:Vec::new(),candidates:Vec::new(),
            descendants:Vec::new(),completed_generations:0,
            cost_budget_krw:request.cost_budget_krw,spent_krw:0.0,
            stop_requested:false,stop_reason:None,created_at_ms:now,updated_at_ms:now};
        if let Err(error)=self.put_evolution(run.clone()).await{
            self.repository_active.lock().await.remove(&parent.repository);
            return Err(error);
        }
        let run_id=run.id;
        let service=Arc::clone(self);
        tokio::spawn(async move {
            let mut run=run;
            // Hold one native workspace writer lease from the first candidate
            // through final adoption, including every real scoring process.
            let _workspace_lease=service.workspace_write_lock.lock().await;
            if let Err(error)=service.advance_evolution(&model,&parent,&mut run).await {
                run.status=EvolutionStatus::Failed;
                run.stop_reason=Some(error);
            }
            run.updated_at_ms=now_ms();
            drop(_workspace_lease);
            service.repository_active.lock().await.remove(&parent.repository);
            let _=service.put_evolution(run).await;
        });
        self.evolution(run_id).await.ok_or("Evolution record not committed".into())
    }

    async fn advance_evolution(
        &self, model:&ModelClient,seed:&Improvement,run:&mut EvolutionRun
    )->Result<(),String>{
        let workspace=self.workspace.canonicalize().map_err(|e|e.to_string())?;
        let repo=workspace.join(relative(&seed.repository)?).canonicalize().map_err(|e|e.to_string())?;
        let head=git(&repo,&["rev-parse","HEAD"]).await?;
        if seed.adopted_commit.as_deref()!=Some(head.trim()) {
            return Err("Seed checkout moved before acquiring native workspace lease".into());
        }
        let mut adopted=seed.clone();
        for target in run.target_files.clone(){
            if self.should_stop_evolution(run).await{return Ok(());}
            if run.spent_krw>=run.cost_budget_krw{
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Cumulative metered model spending reached the approved ceiling".into());
                return Ok(());
            }
            let selected=self.tournament_generation(model,&adopted,&target,run).await?;
            if run.status!=EvolutionStatus::Running{return Ok(());}
            if self.should_stop_evolution(run).await{return Ok(());}
            let Some(proposal_id)=selected else {
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("No independently verified winning candidate in this generation".into());
                return Ok(());
            };
            adopted=self.adopt_internal(proposal_id,true).await?;
            run.descendants.push(proposal_id);
            run.completed_generations+=1;
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
        }
        run.status=EvolutionStatus::Completed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::improvement::{execute,git,ImprovementStatus,NewImprovement};
    use axum::{routing::post,Json,Router};
    use serde_json::{json,Value};
    use tokio::process::Command;

    async fn incremental_model(Json(request):Json<Value>)->Json<Value>{
        let content=request["messages"].as_array()
            .and_then(|messages|messages.iter().rev().find(|msg|msg["role"]=="user"))
            .and_then(|message|message["content"].as_str()).unwrap_or("");
        let previous=content.split("Current contents:\n").last()
            .unwrap_or("").trim().parse::<u32>().unwrap_or(0);
        Json(json!({"choices":[{"message":{"role":"assistant",
            "content":format!("{}\n",previous+1)}}],
            "usage":{"prompt_tokens":5,"completion_tokens":2}}))
    }
    #[tokio::test]
    async fn bounded_evolution_can_improve_a_then_b_then_a_with_real_git_evaluation(){
        if Command::new("git").arg("--version").output().await.is_err(){return;}
        let root=std::env::temp_dir().join(format!("dominion-evolution-{}",Uuid::new_v4()));
        let workspace=root.join("workspace");
        let repo=workspace.join("repo");
        fs::create_dir_all(&repo).await.unwrap();
        git(&repo,&["init"]).await.unwrap();
        fs::write(repo.join("a.txt"),"0\n").await.unwrap();
        fs::write(repo.join("b.txt"),"0\n").await.unwrap();
        git(&repo,&["add","a.txt","b.txt"]).await.unwrap();
        execute("git",&["-c","user.name=Test","-c","user.email=test@example.com",
            "commit","-m","initial"],&repo,30).await.unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(incremental_model))).await.unwrap();
        });
        let model=crate::provider::ModelClient::new(
            crate::provider::ModelConfig::basic(
                format!("http://{addr}/chat/completions"),"fixture".into(),None
            ),None
        ).unwrap();
        let dir=root.join("records");
        let lease=Arc::new(tokio::sync::Mutex::new(()));
        let service=Arc::new(Improvements::open_with_lease(
            dir.clone(),workspace.clone(),Arc::clone(&lease)
        ).await.unwrap());
        let seed=service.create(NewImprovement{
            repository:"repo".into(),target_file:"a.txt".into(),
            instruction:"Improve the objective without changing the evaluator".into(),
            test_command:"test -f a.txt && test -f b.txt".into(),
            score_command:Some("echo $(( $(cat a.txt) + $(cat b.txt) ))".into()),
            parent:None
        }).await.unwrap();
        service.start(model.clone(),seed.id,true).await.unwrap();
        for _ in 0..150 {
            if service.get(seed.id).await.unwrap().status!=ImprovementStatus::Running{break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(service.get(seed.id).await.unwrap().status,ImprovementStatus::Evaluated);
        let seed=service.adopt(seed.id).await.unwrap();
        assert!(seed.adopted_commit.is_some());
        let blocked=lease.lock().await;
        let run=service.evolution_start(model.clone(),seed.id,NewEvolution{
            max_generations:2,cost_budget_krw:100.0,
            target_files:vec!["b.txt".into(),"a.txt".into()],candidates_per_generation:1,validation_command:None
        },true).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        assert!(service.evolution(run.id).await.unwrap().descendants.is_empty(),
            "Evolution must not start while another native writer holds the lease");
        drop(blocked);
        let mut completed=None;
        for _ in 0..180 {
            let current=service.evolution(run.id).await.unwrap();
            if current.status!=EvolutionStatus::Running{completed=Some(current);break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let result=completed.expect("Evolution did not settle within test deadline");
        assert_eq!(result.status,EvolutionStatus::Completed,"{:?}",result.stop_reason);
        assert_eq!(result.completed_generations,2);
        assert_eq!(result.descendants.len(),2);
        assert_eq!(fs::read_to_string(repo.join("a.txt")).await.unwrap(),"2\n");
        assert_eq!(fs::read_to_string(repo.join("b.txt")).await.unwrap(),"1\n");
        let first=service.get(result.descendants[0]).await.unwrap();
        let second=service.get(result.descendants[1]).await.unwrap();
        assert_eq!(first.parent,Some(seed.id));
        assert_eq!(second.parent,Some(first.id));
        assert_eq!(first.target_file,"b.txt");
        assert_eq!(second.target_file,"a.txt");
        assert_eq!(second.base_commit,first.adopted_commit);
        assert_eq!(first.status,ImprovementStatus::Adopted);
        assert_eq!(second.status,ImprovementStatus::Adopted);
        // Stop is an explicit, durable owner action. Holding the same global
        // lease prevents any candidate from starting before stop is seen.
        let hold=lease.lock().await;
        let stopped=service.evolution_start(model.clone(),second.id,NewEvolution{
            max_generations:1,cost_budget_krw:100.0,
            target_files:vec!["a.txt".into()],candidates_per_generation:1,validation_command:None
        },true).await.unwrap();
        let stop_receipt=service.evolution_stop(stopped.id).await.unwrap();
        assert!(stop_receipt.stop_requested);
        drop(hold);
        let mut stopped_result=None;
        for _ in 0..80 {
            let current=service.evolution(stopped.id).await.unwrap();
            if current.status!=EvolutionStatus::Running{stopped_result=Some(current);break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let stopped_result=stopped_result.expect("Stopped evolution failed to settle");
        assert_eq!(stopped_result.status,EvolutionStatus::NeedsReview);
        assert_eq!(stopped_result.completed_generations,0);
        assert!(stopped_result.descendants.is_empty());
        assert_eq!(fs::read_to_string(repo.join("a.txt")).await.unwrap(),"2\n");
        // A paid provider's single request may overshoot the allowed
        // ceiling. Persist and show the candidate but never auto-adopt it.
        let mut rate=crate::provider::ModelConfig::basic(
            format!("http://{addr}/chat/completions"),"priced".into(),None
        );
        rate.input_krw_per_million=1_000_000.0;
        rate.output_krw_per_million=1_000_000.0;
        let priced=crate::provider::ModelClient::new(rate,None).unwrap();
        let limited=service.evolution_start(priced,second.id,NewEvolution{
            max_generations:1,cost_budget_krw:1.0,target_files:vec!["a.txt".into()],candidates_per_generation:1,validation_command:None
        },true).await.unwrap();
        let mut bounded=None;
        for _ in 0..150 {
            let current=service.evolution(limited.id).await.unwrap();
            if current.status!=EvolutionStatus::Running{bounded=Some(current);break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let bounded=bounded.expect("Priced evolution did not settle");
        assert_eq!(bounded.status,EvolutionStatus::NeedsReview);
        assert_eq!(bounded.completed_generations,0);
        assert!(bounded.spent_krw>bounded.cost_budget_krw);
        assert_eq!(fs::read_to_string(repo.join("a.txt")).await.unwrap(),"2\n");
        let proposal=service.get(bounded.descendants[0]).await.unwrap();
        assert_eq!(proposal.status,ImprovementStatus::Evaluated);
        let restored=Improvements::open(dir,workspace).await.unwrap();
        assert_eq!(restored.evolution(run.id).await.unwrap().completed_generations,2);
        server.abort();
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn unpriced_remote_provider_is_blocked_before_any_generation(){
        let root=std::env::temp_dir().join(format!("dominion-evolution-rate-{}",Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let service=Arc::new(Improvements::open(root.join("records"),root.clone()).await.unwrap());
        let model=crate::provider::ModelClient::new(
            crate::provider::ModelConfig::basic("https://example.org/v1/chat/completions".into(),
                "unpriced".into(),None),None
        ).unwrap();
        let error=service.evolution_start(model,Uuid::new_v4(),NewEvolution{
            max_generations:1,cost_budget_krw:10.0,target_files:vec![],candidates_per_generation:1,validation_command:None
        },true).await.unwrap_err();
        assert!(error.contains("KRW rates"));
        fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn interrupted_evolution_is_not_replayed_on_reopen(){
        let root=std::env::temp_dir().join(format!("dominion-evolution-recover-{}",Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let service=Improvements::open(root.join("records"),root.clone()).await.unwrap();
        let now=now_ms();
        let run=EvolutionRun{id:Uuid::new_v4(),starting_trial:Uuid::new_v4(),
            status:EvolutionStatus::Running,max_generations:2,
            target_files:vec!["a.txt".into(),"b.txt".into()],
            candidates_per_generation:1,validation_command:None,
            candidate_groups:Vec::new(),candidates:Vec::new(),descendants:Vec::new(),
            completed_generations:0,cost_budget_krw:100.0,spent_krw:0.0,
            stop_requested:false,stop_reason:None,created_at_ms:now,updated_at_ms:now};
        service.put_evolution(run.clone()).await.unwrap();
        let restored=Improvements::open(root.join("records"),root.clone()).await.unwrap();
        let state=restored.evolution(run.id).await.unwrap();
        assert_eq!(state.status,EvolutionStatus::NeedsReview);
        assert_eq!(state.completed_generations,0);
        assert_eq!(state.descendants.len(),0);
        fs::remove_dir_all(root).await.unwrap();
    }
}
