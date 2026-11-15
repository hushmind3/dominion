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

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum EvolutionStatus { Running, Completed, NeedsReview, Failed }

#[derive(Clone,Debug,Deserialize)]
pub struct NewEvolution {
    pub max_generations:usize,
    pub cost_budget_krw:f64,
    #[serde(default)] pub target_files:Vec<String>,
}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct EvolutionRun {
    pub id:Uuid,
    pub starting_trial:Uuid,
    pub status:EvolutionStatus,
    pub max_generations:usize,
    pub target_files:Vec<String>,
    pub descendants:Vec<Uuid>,
    pub completed_generations:usize,
    pub cost_budget_krw:f64,
    pub spent_krw:f64,
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
            let bytes=serde_json::to_vec(&run).map_err(|e|e.to_string())?;
            fs::write(entry.path(),bytes).await.map_err(|e|e.to_string())?;
        }
        runs.insert(run.id,run);
    }
    Ok(runs)
}

impl Improvements {
    async fn put_evolution(&self,run:EvolutionRun)->Result<EvolutionRun,String>{
        let dir=self.root.join("evolutions");
        let temp=dir.join(format!("{}.tmp",run.id));
        let destination=dir.join(format!("{}.json",run.id));
        fs::write(&temp,serde_json::to_vec(&run).map_err(|e|e.to_string())?)
            .await.map_err(|e|e.to_string())?;
        fs::rename(temp,destination).await.map_err(|e|e.to_string())?;
        self.lineages.lock().await.insert(run.id,run.clone());
        Ok(run)
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
        if !(1..=4).contains(&request.max_generations){
            return Err("Evolution must request 1..4 bounded generations".into());
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
            target_files:targets,descendants:Vec::new(),completed_generations:0,
            cost_budget_krw:request.cost_budget_krw,spent_krw:0.0,
            stop_reason:None,created_at_ms:now,updated_at_ms:now};
        if let Err(error)=self.put_evolution(run.clone()).await{
            self.repository_active.lock().await.remove(&parent.repository);
            return Err(error);
        }
        let service=Arc::clone(self);
        tokio::spawn(async move {
            let mut run=run;
            if let Err(error)=service.advance_evolution(&model,&parent,&mut run).await {
                run.status=EvolutionStatus::Failed;
                run.stop_reason=Some(error);
            }
            run.updated_at_ms=now_ms();
            let _=service.put_evolution(run).await;
            service.repository_active.lock().await.remove(&parent.repository);
        });
        self.evolution(run.id).await.ok_or("Evolution record not committed".into())
    }

    async fn advance_evolution(
        &self, model:&ModelClient,seed:&Improvement,run:&mut EvolutionRun
    )->Result<(),String>{
        let mut adopted=seed.clone();
        for target in run.target_files.clone(){
            if run.spent_krw>=run.cost_budget_krw{
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Cumulative measured model spending reached the approved ceiling".into());
                return Ok(());
            }
            let descendant=self.create(NewImprovement{
                repository:adopted.repository.clone(),target_file:target,
                instruction:adopted.instruction.clone(),test_command:adopted.test_command.clone(),
                score_command:adopted.score_command.clone(),parent:Some(adopted.id)
            }).await?;
            run.descendants.push(descendant.id);
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
            let mut current=descendant;
            current.status=ImprovementStatus::Running;
            current.updated_at_ms=now_ms();
            self.put(current.clone()).await?;
            if let Err(error)=self.evaluate(model,current).await {
                if let Some(mut failed)=self.get(*run.descendants.last().unwrap()).await{
                    failed.status=ImprovementStatus::Failed;
                    failed.error=Some(error.clone());
                    failed.updated_at_ms=now_ms();
                    let _=self.put(failed).await;
                }
                return Err(error);
            }
            let proposal_id=*run.descendants.last().ok_or("No descendant recorded")?;
            let assessed=self.get(proposal_id).await.ok_or("Candidate evaluation disappeared")?;
            run.spent_krw+=assessed.cost_krw;
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
            // An upstream provider might charge during a request. Never auto-
            // adopt after measuring that the fixed ceiling was exceeded.
            if run.spent_krw>run.cost_budget_krw{
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Generation exceeded measured KRW ceiling; candidate not adopted".into());
                return Ok(());
            }
            if assessed.status!=ImprovementStatus::Evaluated || !assessed.accepted{
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Independent evaluator found no verified gain; chain stopped".into());
                return Ok(());
            }
            adopted=self.adopt_internal(proposal_id,true).await?;
            run.completed_generations+=1;
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
        }
        run.status=EvolutionStatus::Completed;
        Ok(())
    }
}
