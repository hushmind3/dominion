//! Independently score sibling proposals from the same adopted base before
//! electing a successor. Model prompts never receive the hold-out verifier
//! command or its observations. Only Rust sees that additional evidence.
use super::*;
use crate::improvement::{git,relative,Measurement,NewImprovement,ImprovementStatus};
use std::path::PathBuf;

fn choose<'a>(items:&'a [CandidateRecord])->Option<&'a CandidateRecord>{
    items.iter().filter(|c|c.eligible && c.training_score.is_some()).max_by(|a,b|{
        let aval=a.validation_score.or(a.training_score).unwrap_or(f64::NEG_INFINITY);
        let bval=b.validation_score.or(b.training_score).unwrap_or(f64::NEG_INFINITY);
        aval.total_cmp(&bval)
            .then_with(||a.training_score.unwrap_or(f64::NEG_INFINITY)
                .total_cmp(&b.training_score.unwrap_or(f64::NEG_INFINITY)))
            .then_with(||b.proposal_index.cmp(&a.proposal_index))
    })
}
impl Improvements {
    pub(super) async fn tournament_generation(
        &self, model:&ModelClient,adopted:&Improvement,target:&str,
        run:&mut EvolutionRun
    )->Result<Option<Uuid>,String>{
        let generation=run.completed_generations;
        run.candidate_groups.push(Vec::new());
        self.put_evolution(run.clone()).await?;
        let mut evaluated=Vec::<CandidateRecord>::new();
        for proposal_index in 0..run.candidates_per_generation {
            if self.should_stop_evolution(run).await{return Ok(None);}
            if run.spent_krw>=run.cost_budget_krw {
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Candidate tournament reached measured KRW ceiling".into());
                return Ok(None);
            }
            // Every sibling has the same adopted parent and original base HEAD.
            // No candidate is adopted until the entire tournament has been
            // independently checked and a winner selected.
            let mut trial=self.create(NewImprovement{
                repository:adopted.repository.clone(),target_file:target.into(),
                instruction:adopted.instruction.clone(),test_command:adopted.test_command.clone(),
                score_command:adopted.score_command.clone(),parent:Some(adopted.id)
            }).await?;
            trial.proposal_index=proposal_index;
            trial.proposal_count=run.candidates_per_generation;
            trial.status=ImprovementStatus::Running;
            trial.updated_at_ms=now_ms();
            self.put(trial.clone()).await?;
            run.candidate_groups[generation].push(trial.id);
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
            let result=self.evaluate(model,trial.clone()).await;
            if let Err(error)=&result {
                if let Some(mut record)=self.get(trial.id).await {
                    record.status=ImprovementStatus::Failed;
                    record.error=Some(error.clone());
                    record.updated_at_ms=now_ms();
                    self.put(record).await?;
                }
            }
            let result_trial=self.get(trial.id).await.ok_or("Candidate record lost")?;
            run.spent_krw+=result_trial.cost_krw;
            let mut candidate=CandidateRecord{
                trial_id:trial.id,generation,proposal_index,
                training_score:result_trial.candidate.as_ref().and_then(|x|x.score),
                validation_baseline_score:None,validation_score:None,
                eligible:result_trial.status==ImprovementStatus::Evaluated && result_trial.accepted,
                reason:result.err(),
            };
            // Keep the numeric holdout command out of the model-facing
            // candidate prompts and reflection. It is operator-frozen per run.
            if candidate.eligible {
                if let Some(command)=run.validation_command.as_deref() {
                    match self.holdout(&result_trial,command).await {
                        Ok((baseline,proposed))=>{
                            candidate.validation_baseline_score=baseline.score;
                            candidate.validation_score=proposed.score;
                            if !super::super::is_better(&baseline,&proposed) {
                                candidate.eligible=false;
                                candidate.reason=Some("Independent validation did not improve".into());
                            }
                        }
                        Err(error)=>{
                            candidate.eligible=false;
                            candidate.reason=Some(format!("Validation error: {error}"));
                        }
                    }
                }
            }
            if !candidate.eligible && candidate.reason.is_none() {
                candidate.reason=Some("Training score did not improve".into());
            }
            // A candidate rejected by independent validation is no longer
            // independently adoptable through the ordinary trial endpoint.
            if !candidate.eligible && result_trial.status==ImprovementStatus::Evaluated {
                let mut rejected=result_trial.clone();
                rejected.accepted=false;
                rejected.status=ImprovementStatus::Rejected;
                rejected.error=candidate.reason.clone();
                rejected.updated_at_ms=now_ms();
                self.put(rejected).await?;
            }
            evaluated.push(candidate.clone());
            run.candidates.push(candidate);
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
            // No candidate may be adopted after a stop, including a pending
            // evaluation request or a single over-budget provider response.
            if self.should_stop_evolution(run).await{return Ok(None);}
            if run.spent_krw>run.cost_budget_krw {
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Tournament exceeded measured KRW budget; no adoption".into());
                return Ok(None);
            }
        }
        let winner=choose(&evaluated).map(|item|item.trial_id);
        // Candidates that passed a threshold but lost the competition must
        // not stay marked as independently adoptable through the public API.
        for item in evaluated.iter().filter(|item|item.eligible && Some(item.trial_id)!=winner) {
            if let Some(mut lost)=self.get(item.trial_id).await {
                lost.accepted=false;
                lost.status=ImprovementStatus::Rejected;
                lost.reflection=Some(format!(
                    "{} Tournament selection: higher-scored sibling was preferred.",
                    lost.reflection.unwrap_or_default()
                ));
                lost.updated_at_ms=now_ms();
                self.put(lost).await?;
            }
        }
        Ok(winner)
    }

    async fn holdout(&self,trial:&Improvement,command:&str)
        ->Result<(Measurement,Measurement),String>{
        let workspace=self.workspace.canonicalize().map_err(|e|e.to_string())?;
        let repo=workspace.join(relative(&trial.repository)?).canonicalize()
            .map_err(|e|e.to_string())?;
        if !repo.starts_with(&workspace){return Err("Holdout repository escaped workspace".into());}
        let base=trial.base_commit.as_deref().ok_or("Missing candidate base")?;
        let proposed=trial.candidate_commit.as_deref().ok_or("Missing pinned candidate")?;
        let pinned=git(&repo,&["rev-parse",&format!("refs/dominion/improvements/{}",trial.id)]).await?;
        if pinned.trim()!=proposed {return Err("Holdout candidate differs from pinned Git commit".into());}
        let root=self.root.join(trial.id.to_string());
        tokio::fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        let baseline=root.join("holdout_baseline");
        let candidate=root.join("holdout_candidate");
        let mut created=Vec::<PathBuf>::new();
        for (path,sha) in [(&baseline,base),(&candidate,proposed)] {
            let name=path.to_str().ok_or("Invalid validation worktree path")?;
            if let Err(error)=git(&repo,&["worktree","add","--detach",name,sha]).await {
                for existing in &created {
                    if let Some(dir)=existing.to_str(){let _=git(&repo,&["worktree","remove","--force",dir]).await;}
                }
                return Err(error);
            }
            created.push(path.to_path_buf());
        }
        let result=async {
            let a=self.measure(&baseline,&trial.test_command,Some(command)).await?;
            let b=self.measure(&candidate,&trial.test_command,Some(command)).await?;
            Ok::<_,String>((a,b))
        }.await;
        for dir in [&baseline,&candidate] {
            if let Some(name)=dir.to_str(){
                let _=git(&repo,&["worktree","remove","--force",name]).await;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_validation_beats_training_and_ties_are_stable(){
        let a=CandidateRecord{trial_id:Uuid::new_v4(),generation:0,proposal_index:0,
            training_score:Some(9.0),validation_baseline_score:Some(0.0),
            validation_score:Some(1.0),eligible:true,reason:None};
        let b=CandidateRecord{trial_id:Uuid::new_v4(),generation:0,proposal_index:1,
            training_score:Some(5.0),validation_baseline_score:Some(0.0),
            validation_score:Some(3.0),eligible:true,reason:None};
        assert_eq!(choose(&[a.clone(),b.clone()]).unwrap().trial_id,b.trial_id);
        let mut invalid=b.clone();
        invalid.eligible=false;
        assert_eq!(choose(&[a.clone(),invalid]).unwrap().trial_id,a.trial_id);
        let tied=CandidateRecord{validation_score:Some(1.0),training_score:Some(9.0),..b};
        assert_eq!(choose(&[a.clone(),tied]).unwrap().trial_id,a.trial_id);
    }
}
